use super::error;
use crate::audio_buffer::AudioMut;
use crate::backends::wasapi::util::WasapiMMDevice;
use crate::channel_map::Bitset;
use crate::prelude::{AudioRef, Timestamp};
use crate::{
    AudioCallbackContext, AudioInput, AudioInputCallback, AudioOutput, AudioOutputCallback,
    AudioStreamHandle, StreamConfig,
};
use duplicate::duplicate_item;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{ops, ptr, slice};
use windows::core::imp::CoTaskMemFree;
use windows::core::Interface;
use windows::Win32::Foundation;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::{Audio, KernelStreaming, Multimedia};
use windows::Win32::System::Threading;

type EjectSignal = Arc<AtomicBool>;

#[duplicate_item(
name                 ty;
[AudioCaptureBuffer] [IAudioCaptureClient];
[AudioRenderBuffer]  [IAudioRenderClient];
)]
struct name<'a, T> {
    interface: &'a Audio::ty,
    data: NonNull<u8>,
    frame_size: usize,
    channels: usize,
    __type: PhantomData<T>,
}

#[duplicate_item(
name;
[AudioCaptureBuffer];
[AudioRenderBuffer];
)]
impl<'a, T> ops::Deref for name<'a, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.data.cast().as_ptr(), self.channels * self.frame_size) }
    }
}

#[duplicate_item(
name;
[AudioCaptureBuffer];
[AudioRenderBuffer];
)]
impl<'a, T> ops::DerefMut for name<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            slice::from_raw_parts_mut(self.data.cast().as_ptr(), self.channels * self.frame_size)
        }
    }
}

impl<T> Drop for AudioCaptureBuffer<'_, T> {
    fn drop(&mut self) {
        unsafe { self.interface.ReleaseBuffer(self.frame_size as _).unwrap() };
    }
}

impl<T> Drop for AudioRenderBuffer<'_, T> {
    fn drop(&mut self) {
        unsafe {
            self.interface
                .ReleaseBuffer(self.frame_size as _, 0)
                .unwrap();
        }
    }
}

impl<'a, T> AudioRenderBuffer<'a, T> {
    fn from_client(
        render_client: &'a Audio::IAudioRenderClient,
        channels: usize,
        frame_size: usize,
    ) -> Result<Self, error::WasapiError> {
        let data = NonNull::new(unsafe { render_client.GetBuffer(frame_size as _) }?)
            .expect("Audio buffer data is null");
        Ok(Self {
            interface: render_client,
            data,
            frame_size,
            channels,
            __type: PhantomData,
        })
    }
}
impl<'a, T> AudioCaptureBuffer<'a, T> {
    fn from_client(
        capture_client: &'a Audio::IAudioCaptureClient,
        channels: usize,
    ) -> Result<Option<Self>, error::WasapiError> {
        let mut buf_ptr = ptr::null_mut();
        let mut frame_size = 0;
        let mut flags = 0;
        unsafe { capture_client.GetBuffer(&mut buf_ptr, &mut frame_size, &mut flags, None, None) }?;
        let Some(data) = NonNull::new(buf_ptr as _) else {
            return Ok(None);
        };
        Ok(Some(Self {
            interface: capture_client,
            data,
            frame_size: frame_size as _,
            channels,
            __type: PhantomData,
        }))
    }
}

struct AudioThread<Callback, Interface> {
    audio_client: Audio::IAudioClient,
    interface: Interface,
    audio_clock: Audio::IAudioClock,
    stream_config: StreamConfig,
    eject_signal: EjectSignal,
    frame_size: usize,
    callback: Callback,
    event_handle: HANDLE,
    clock_start: Duration,
}

impl<Callback, Interface> AudioThread<Callback, Interface> {
    fn finalize(self) -> Result<Callback, error::WasapiError> {
        if !self.event_handle.is_invalid() {
            unsafe { CloseHandle(self.event_handle) }?;
        }
        let _ = unsafe {
            self.audio_client
                .Stop()
                .inspect_err(|err| eprintln!("Cannot stop audio thread: {err}"))
        };
        Ok(self.callback)
    }
}

impl<Callback, Iface: Interface> AudioThread<Callback, Iface> {
    fn new(
        device: WasapiMMDevice,
        eject_signal: EjectSignal,
        mut stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, error::WasapiError> {
        unsafe {
            let audio_client: Audio::IAudioClient = device.activate()?;
            let sharemode = if stream_config.exclusive {
                Audio::AUDCLNT_SHAREMODE_EXCLUSIVE
            } else {
                Audio::AUDCLNT_SHAREMODE_SHARED
            };
            let format = {
                let mut format = config_to_waveformatextensible(&stream_config);
                let mut actual_format = ptr::null_mut();
                audio_client
                    .IsFormatSupported(
                        sharemode,
                        &format.Format,
                        (!stream_config.exclusive).then_some(&mut actual_format),
                    )
                    .ok()?;
                if !stream_config.exclusive {
                    assert!(!actual_format.is_null());
                    format.Format = actual_format.read_unaligned();
                    CoTaskMemFree(actual_format.cast());
                    let sample_rate = format.Format.nSamplesPerSec;
                    stream_config.channels = 0u32.with_indices(0..format.Format.nChannels as _);
                    stream_config.samplerate = sample_rate as _;
                }
                format
            };
            let frame_size = stream_config
                .buffer_size_range
                .0
                .or(stream_config.buffer_size_range.1);
            let buffer_duration = frame_size
                .map(|frame_size| {
                    buffer_size_to_duration(frame_size, stream_config.samplerate as _)
                })
                .unwrap_or(0);
            audio_client.Initialize(
                sharemode,
                Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | Audio::AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                buffer_duration,
                0,
                &format.Format,
                None,
            )?;
            let buffer_size = audio_client.GetBufferSize()? as usize;
            let event_handle = {
                let event_handle =
                    Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))?;
                audio_client.SetEventHandle(event_handle)?;
                event_handle
            };
            let interface = audio_client.GetService::<Iface>()?;
            let audio_clock = audio_client.GetService::<Audio::IAudioClock>()?;
            let frame_size = buffer_size;
            Ok(Self {
                audio_client,
                interface,
                audio_clock,
                event_handle,
                frame_size,
                eject_signal,
                stream_config: StreamConfig {
                    buffer_size_range: (Some(frame_size), Some(frame_size)),
                    ..stream_config
                },
                clock_start: Duration::ZERO,
                callback,
            })
        }
    }

    fn await_frame(&mut self) -> Result<(), error::WasapiError> {
        let _ = unsafe {
            let result = Threading::WaitForSingleObject(self.event_handle, Threading::INFINITE);
            if result == Foundation::WAIT_FAILED {
                let err = Foundation::GetLastError();
                let description = format!("Waiting for event handle failed: {:?}", err);
                return Err(error::WasapiError::FoundationError(description));
            }
            result
        };
        Ok(())
    }

    fn output_timestamp(&self) -> Result<Timestamp, error::WasapiError> {
        let clock = stream_instant(&self.audio_clock)?;
        let diff = clock - self.clock_start;
        Ok(Timestamp::from_duration(
            self.stream_config.samplerate,
            diff,
        ))
    }
}

impl<Callback: AudioInputCallback> AudioThread<Callback, Audio::IAudioCaptureClient> {
    fn run(mut self) -> Result<Callback, error::WasapiError> {
        set_thread_priority();
        unsafe {
            self.audio_client.Start()?;
        }
        self.clock_start = stream_instant(&self.audio_clock)?;
        loop {
            if self.eject_signal.load(Ordering::Relaxed) {
                break self.finalize();
            }
            self.await_frame()?;
            self.process()?;
        }
        .inspect_err(|err| eprintln!("Render thread process error: {err}"))
    }

    fn process(&mut self) -> Result<(), error::WasapiError> {
        let frames_available = unsafe { self.interface.GetNextPacketSize()? as usize };
        if frames_available == 0 {
            return Ok(());
        }
        let Some(mut buffer) = AudioCaptureBuffer::<f32>::from_client(
            &self.interface,
            self.stream_config.channels.count(),
        )?
        else {
            eprintln!("Null buffer from WASAPI");
            return Ok(());
        };
        let timestamp = self.output_timestamp()?;
        let context = AudioCallbackContext {
            stream_config: self.stream_config,
            timestamp,
        };
        let buffer =
            AudioRef::from_interleaved(&mut buffer, self.stream_config.channels.count()).unwrap();
        let output = AudioInput { timestamp, buffer };
        self.callback.on_input_data(context, output);
        Ok(())
    }
}

impl<Callback: AudioOutputCallback> AudioThread<Callback, Audio::IAudioRenderClient> {
    fn run(mut self) -> Result<Callback, error::WasapiError> {
        set_thread_priority();
        unsafe {
            self.audio_client.Start()?;
        }
        self.clock_start = stream_instant(&self.audio_clock)?;
        loop {
            if self.eject_signal.load(Ordering::Relaxed) {
                break self.finalize();
            }
            self.await_frame()?;
            self.process()?;
        }
        .inspect_err(|err| eprintln!("Render thread process error: {err}"))
    }

    fn process(&mut self) -> Result<(), error::WasapiError> {
        let frames_available = unsafe {
            let padding = self.audio_client.GetCurrentPadding()? as usize;
            self.frame_size - padding
        };
        if frames_available == 0 {
            return Ok(());
        }
        let frames_requested = if let Some(max_frames) = self.stream_config.buffer_size_range.1 {
            frames_available.min(max_frames)
        } else {
            frames_available
        };
        let mut buffer = AudioRenderBuffer::<f32>::from_client(
            &self.interface,
            self.stream_config.channels.count(),
            frames_requested,
        )?;
        let timestamp = self.output_timestamp()?;
        let context = AudioCallbackContext {
            stream_config: self.stream_config,
            timestamp,
        };
        let buffer =
            AudioMut::from_interleaved_mut(&mut buffer, self.stream_config.channels.count())
                .unwrap();
        let output = AudioOutput { timestamp, buffer };
        self.callback.on_output_data(context, output);
        Ok(())
    }
}

/// Type representing a WASAPI audio stream.
pub struct WasapiStream<Callback> {
    join_handle: JoinHandle<Result<Callback, error::WasapiError>>,
    eject_signal: EjectSignal,
}

impl<Callback> AudioStreamHandle<Callback> for WasapiStream<Callback> {
    type Error = error::WasapiError;

    fn eject(self) -> Result<Callback, Self::Error> {
        self.eject_signal.store(true, Ordering::Relaxed);
        self.join_handle
            .join()
            .expect("Audio output thread panicked")
    }
}

impl<Callback: 'static + Send + AudioInputCallback> WasapiStream<Callback> {
    pub(crate) fn new_input(
        device: WasapiMMDevice,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Self {
        let eject_signal = EjectSignal::default();
        let join_handle = std::thread::Builder::new()
            .name("interflow_wasapi_output_stream".to_string())
            .spawn({
                let eject_signal = eject_signal.clone();
                move || {
                    let inner: AudioThread<Callback, Audio::IAudioCaptureClient> =
                        AudioThread::new(device, eject_signal, stream_config, callback)
                            .inspect_err(|err| {
                                eprintln!("Failed to create render thread: {err}")
                            })?;
                    inner.run()
                }
            })
            .expect("Cannot spawn audio output thread");
        Self {
            join_handle,
            eject_signal,
        }
    }
}

impl<Callback: 'static + Send + AudioOutputCallback> WasapiStream<Callback> {
    pub(crate) fn new_output(
        device: WasapiMMDevice,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Self {
        let eject_signal = EjectSignal::default();
        let join_handle = std::thread::Builder::new()
            .name("interflow_wasapi_output_stream".to_string())
            .spawn({
                let eject_signal = eject_signal.clone();
                move || {
                    let inner: AudioThread<Callback, Audio::IAudioRenderClient> =
                        AudioThread::new(device, eject_signal, stream_config, callback)
                            .inspect_err(|err| {
                                eprintln!("Failed to create render thread: {err}")
                            })?;
                    inner.run()
                }
            })
            .expect("Cannot spawn audio output thread");
        Self {
            join_handle,
            eject_signal,
        }
    }
}

fn set_thread_priority() {
    unsafe {
        let thread_id = Threading::GetCurrentThreadId();

        let _ = Threading::SetThreadPriority(
            HANDLE(thread_id as isize as _),
            Threading::THREAD_PRIORITY_TIME_CRITICAL,
        );
    }
}

pub fn buffer_size_to_duration(buffer_size: usize, sample_rate: u32) -> i64 {
    (buffer_size as i64 / sample_rate as i64) * (1_000_000_000 / 100)
}

fn stream_instant(audio_clock: &Audio::IAudioClock) -> Result<Duration, error::WasapiError> {
    let mut position: u64 = 0;
    let mut qpc_position: u64 = 0;
    unsafe {
        audio_clock.GetPosition(&mut position, Some(&mut qpc_position))?;
    };
    // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
    let qpc_nanos = qpc_position * 100;
    let instant = Duration::from_nanos(qpc_nanos);
    Ok(instant)
}

pub(crate) fn config_to_waveformatextensible(config: &StreamConfig) -> Audio::WAVEFORMATEXTENSIBLE {
    let format_tag = KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
    let channels = config.channels as u16;
    let sample_rate = config.samplerate as u32;
    let sample_bytes = size_of::<f32>() as u16;
    let avg_bytes_per_sec = u32::from(channels) * sample_rate * u32::from(sample_bytes);
    let block_align = channels * sample_bytes;
    let bits_per_sample = 8 * sample_bytes;

    let cb_size = {
        let extensible_size = size_of::<Audio::WAVEFORMATEXTENSIBLE>();
        let ex_size = size_of::<Audio::WAVEFORMATEX>();
        (extensible_size - ex_size) as u16
    };

    let waveformatex = Audio::WAVEFORMATEX {
        wFormatTag: format_tag as u16,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    let channel_mask = KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;

    let waveformatextensible = Audio::WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: Audio::WAVEFORMATEXTENSIBLE_0 {
            wSamplesPerBlock: bits_per_sample,
        },
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    waveformatextensible
}

pub(crate) fn is_output_config_supported(
    device: WasapiMMDevice,
    stream_config: &StreamConfig,
) -> bool {
    let try_ = || unsafe {
        let audio_client: Audio::IAudioClient = device.activate()?;
        let sharemode = if stream_config.exclusive {
            Audio::AUDCLNT_SHAREMODE_EXCLUSIVE
        } else {
            Audio::AUDCLNT_SHAREMODE_SHARED
        };
        let mut format = config_to_waveformatextensible(&stream_config);
        let mut actual_format = ptr::null_mut();
        audio_client
            .IsFormatSupported(
                sharemode,
                &format.Format,
                (!stream_config.exclusive).then_some(&mut actual_format),
            )
            .ok()?;
        if !stream_config.exclusive {
            assert!(!actual_format.is_null());
            format.Format = actual_format.read_unaligned();
            CoTaskMemFree(actual_format.cast());
            let sample_rate = format.Format.nSamplesPerSec;
            let new_channels = 0u32.with_indices(0..format.Format.nChannels as _);
            let new_samplerate = sample_rate as f64;
            if stream_config.samplerate != new_samplerate
                || stream_config.channels.count() != new_channels.count()
            {
                return Ok(false);
            }
        }
        Ok::<_, error::WasapiError>(true)
    };
    try_()
        .inspect_err(|err| eprintln!("Error while checking configuration is valid: {err}"))
        .unwrap_or(false)
}
