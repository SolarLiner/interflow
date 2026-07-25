use crate::device::Device;
use crate::Error;
use interflow_core::buffer::AudioBuffer;
use interflow_core::device::{ResolvedStreamConfig, StreamConfig};
use interflow_core::stream;
use interflow_core::stream::{AudioInput, AudioOutput, CallbackContext, StreamProxy};
use interflow_core::timing::Timestamp;
use interflow_core::traits::{ExtensionProvider, Selector};
use std::num::NonZeroUsize;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_FAILED};
use windows::Win32::Media::Audio;
use windows::Win32::System::Threading;

type EjectSignal = Arc<AtomicBool>;

pub struct Handle<Callback> {
    join_handle: JoinHandle<Result<Callback, Error>>,
    eject_signal: EjectSignal,
}

impl<Callback: 'static + stream::Callback> stream::StreamHandle<Callback> for Handle<Callback> {
    type Error = Error;

    fn eject(self) -> Result<Callback, Self::Error> {
        self.eject_signal.store(true, Ordering::Relaxed);
        self.join_handle.join().expect("Audio thread panicked")
    }
}

impl<Callback: 'static + Send + stream::Callback> Handle<Callback> {
    pub(crate) fn new(
        device: &Device,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, Error> {
        let requested = stream_config.requested_device_type();
        if requested.is_duplex() {
            return Err(Error::DuplexStreamRequested);
        }

        let eject_signal = EjectSignal::default();
        let device = device.clone();
        let thread_name = if requested.is_input() {
            "interflow_wasapi_input_stream"
        } else {
            "interflow_wasapi_output_stream"
        };

        let join_handle = std::thread::Builder::new()
            .name(thread_name.to_string())
            .spawn({
                let eject = eject_signal.clone();
                move || {
                    set_thread_priority();
                    if requested.is_input() {
                        run_input(device, stream_config, callback, eject)
                    } else {
                        run_output(device, stream_config, callback, eject)
                    }
                }
            })
            .expect("Cannot spawn audio thread");

        Ok(Self {
            join_handle,
            eject_signal,
        })
    }
}

fn run_output<C: stream::Callback>(
    device: Device,
    stream_config: StreamConfig,
    mut callback: C,
    eject_signal: EjectSignal,
) -> Result<C, Error> {
    unsafe {
        let audio_client = device.activate_audio_client()?;

        let format = Device::build_format(&stream_config);
        let buffer_duration = stream_config
            .buffer_size_range
            .0
            .or(stream_config.buffer_size_range.1)
            .map(|frames| buffer_size_to_duration(frames, stream_config.sample_rate as u32))
            .unwrap_or(0);

        audio_client.Initialize(
            Audio::AUDCLNT_SHAREMODE_SHARED,
            Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK | Audio::AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            buffer_duration,
            0,
            &format,
            None,
        )?;

        let frame_count = audio_client.GetBufferSize()? as usize;
        let resolved_config = ResolvedStreamConfig {
            sample_rate: stream_config.sample_rate,
            input_channels: 0,
            output_channels: stream_config.output_channels,
            max_frame_count: frame_count,
        };

        let event_handle =
            Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))?;
        audio_client.SetEventHandle(event_handle)?;

        let render_client = audio_client.GetService::<Audio::IAudioRenderClient>()?;
        let audio_clock = audio_client.GetService::<Audio::IAudioClock>()?;

        let frame_size = NonZeroUsize::new(frame_count.max(1)).unwrap();
        let mut audio_output = AudioBuffer::zeroed(frame_size, stream_config.output_channels);
        let audio_input = AudioBuffer::empty(frame_size);

        let context = CallbackContext {
            stream_config: &resolved_config,
            timestamp: Timestamp::new(resolved_config.sample_rate),
            stream_proxy: &STREAM_PROXY,
        };
        callback.prepare(context);

        audio_client.Start()?;
        let clock_start = stream_instant(&audio_clock)?;

        loop {
            if eject_signal.load(Ordering::Relaxed) {
                break;
            }
            let result = Threading::WaitForSingleObject(event_handle, Threading::INFINITE);
            if result == WAIT_FAILED {
                let err = foundation_get_last_error();
                return Err(Error::FoundationError(format!(
                    "WaitForSingleObject failed: {:?}",
                    err
                )));
            }

            let padding = audio_client.GetCurrentPadding()? as usize;
            let frames_available = frame_count.saturating_sub(padding);
            if frames_available == 0 {
                continue;
            }

            let timestamp =
                output_timestamp(&audio_clock, clock_start, resolved_config.sample_rate)?;

            let dummy_input = AudioInput {
                timestamp,
                buffer: audio_input.slice(..0),
                channel_flags: &[],
            };

            let mut output = AudioOutput {
                timestamp,
                buffer: audio_output.slice_mut(..frames_available),
                channel_flags: &[],
            };

            callback.process_audio(
                CallbackContext {
                    stream_config: &resolved_config,
                    timestamp,
                    stream_proxy: &STREAM_PROXY,
                },
                &dummy_input,
                &mut output,
            );

            let wasapi_buf = render_client.GetBuffer(frames_available as _)?;
            let wasapi_slice = slice::from_raw_parts_mut(
                wasapi_buf as *mut f32,
                frames_available * stream_config.output_channels,
            );
            for frame in 0..frames_available {
                for ch in 0..stream_config.output_channels {
                    wasapi_slice[frame * stream_config.output_channels + ch] =
                        audio_output[ch][frame];
                }
            }
            render_client.ReleaseBuffer(frames_available as _, 0)?;
        }

        audio_client.Stop()?;
        CloseHandle(event_handle)?;
        Ok(callback)
    }
}

fn run_input<C: stream::Callback>(
    device: Device,
    stream_config: StreamConfig,
    mut callback: C,
    eject_signal: EjectSignal,
) -> Result<C, Error> {
    unsafe {
        let audio_client = device.activate_audio_client()?;

        let format = Device::build_format(&stream_config);
        let buffer_duration = stream_config
            .buffer_size_range
            .0
            .or(stream_config.buffer_size_range.1)
            .map(|frames| buffer_size_to_duration(frames, stream_config.sample_rate as u32))
            .unwrap_or(0);

        audio_client.Initialize(
            Audio::AUDCLNT_SHAREMODE_SHARED,
            Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK | Audio::AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            buffer_duration,
            0,
            &format,
            None,
        )?;

        let frame_count = audio_client.GetBufferSize()? as usize;
        let resolved_config = ResolvedStreamConfig {
            sample_rate: stream_config.sample_rate,
            input_channels: stream_config.input_channels,
            output_channels: 0,
            max_frame_count: frame_count,
        };

        let event_handle =
            Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))?;
        audio_client.SetEventHandle(event_handle)?;

        let capture_client = audio_client.GetService::<Audio::IAudioCaptureClient>()?;
        let audio_clock = audio_client.GetService::<Audio::IAudioClock>()?;

        let frame_size = NonZeroUsize::new(frame_count.max(1)).unwrap();
        let mut audio_input = AudioBuffer::zeroed(frame_size, stream_config.input_channels);
        let mut audio_output = AudioBuffer::empty(frame_size);

        let context = CallbackContext {
            stream_config: &resolved_config,
            timestamp: Timestamp::new(resolved_config.sample_rate),
            stream_proxy: &STREAM_PROXY,
        };
        callback.prepare(context);

        audio_client.Start()?;
        let clock_start = stream_instant(&audio_clock)?;

        loop {
            if eject_signal.load(Ordering::Relaxed) {
                break;
            }
            let result = Threading::WaitForSingleObject(event_handle, Threading::INFINITE);
            if result == WAIT_FAILED {
                let err = foundation_get_last_error();
                return Err(Error::FoundationError(format!(
                    "WaitForSingleObject failed: {:?}",
                    err
                )));
            }

            let mut buf_ptr = ptr::null_mut();
            let mut frames_available = 0u32;
            let mut flags = 0u32;
            capture_client.GetBuffer(
                &mut buf_ptr,
                &mut frames_available,
                &mut flags,
                None,
                None,
            )?;

            if frames_available == 0 {
                continue;
            }

            let wasapi_slice = slice::from_raw_parts(
                buf_ptr as *const f32,
                frames_available as usize * stream_config.input_channels,
            );
            audio_input.copy_from_interleaved(wasapi_slice);
            capture_client.ReleaseBuffer(frames_available)?;

            let timestamp =
                output_timestamp(&audio_clock, clock_start, resolved_config.sample_rate)?;

            let input = AudioInput {
                timestamp,
                buffer: audio_input.slice(..frames_available as usize),
                channel_flags: &[],
            };

            let mut dummy_output = AudioOutput {
                timestamp,
                buffer: audio_output.slice_mut(..0),
                channel_flags: &[],
            };

            callback.process_audio(
                CallbackContext {
                    stream_config: &resolved_config,
                    timestamp,
                    stream_proxy: &STREAM_PROXY,
                },
                &input,
                &mut dummy_output,
            );
        }

        audio_client.Stop()?;
        CloseHandle(event_handle)?;
        Ok(callback)
    }
}

fn stream_instant(audio_clock: &Audio::IAudioClock) -> Result<Duration, Error> {
    let mut position: u64 = 0;
    let mut qpc_position: u64 = 0;
    unsafe {
        audio_clock.GetPosition(&mut position, Some(&mut qpc_position))?;
    }
    let qpc_nanos = qpc_position * 100;
    Ok(Duration::from_nanos(qpc_nanos))
}

fn output_timestamp(
    audio_clock: &Audio::IAudioClock,
    clock_start: Duration,
    sample_rate: f64,
) -> Result<Timestamp, Error> {
    let clock = stream_instant(audio_clock)?;
    let diff = clock - clock_start;
    Ok(Timestamp::from_duration(sample_rate, diff))
}

fn buffer_size_to_duration(buffer_size: usize, sample_rate: u32) -> i64 {
    (buffer_size as i64 * 10_000_000) / sample_rate as i64
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

fn foundation_get_last_error() -> windows::core::Error {
    unsafe { windows::Win32::Foundation::GetLastError().into() }
}

struct WASAPIStreamProxy;

impl ExtensionProvider for WASAPIStreamProxy {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl StreamProxy for WASAPIStreamProxy {}

static STREAM_PROXY: WASAPIStreamProxy = WASAPIStreamProxy;
