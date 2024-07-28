use std::{
    borrow::Cow, ffi::OsString, marker::PhantomData, ops::Add, os::windows::ffi::OsStringExt, ptr, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, OnceLock}, thread::JoinHandle, time::Duration
};

use crate::{
    prelude::{AudioMut, Timestamp}, AudioCallbackContext, AudioDevice, AudioDriver, AudioInputCallback,
    AudioInputDevice, AudioOutput, AudioOutputCallback, AudioOutputDevice, AudioStreamHandle,
    Channel, DeviceType, StreamConfig,
};
use thiserror::Error;
use windows::Win32::{
    Devices::Properties,
    Foundation::{CloseHandle, HANDLE},
    Media::{
        Audio::{
            self, IAudioCaptureClient, IAudioClient, IAudioClock, IAudioRenderClient, IMMDevice, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0
        },
        KernelStreaming, Multimedia,
    },
    System::{
        Com::{self, StructuredStorage, STGM_READ},
        Threading,
        Variant::VT_LPWSTR,
    },
};

mod util {
    use std::marker::PhantomData;

    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    thread_local!(static COM_INITIALIZER: ComInitializer = {
        unsafe {
            // Try to initialize COM with STA by default to avoid compatibility issues with the ASIO
            // backend (where CoInitialize() is called by the ASIO SDK) or winit (where drag and drop
            // requires STA).
            // This call can fail with RPC_E_CHANGED_MODE if another library initialized COM with MTA.
            // That's OK though since COM ensures thread-safety/compatibility through marshalling when
            // necessary.
            let result = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if result.is_ok() || result == RPC_E_CHANGED_MODE {
                ComInitializer {
                    result,
                    _ptr: PhantomData,
                }
            } else {
                // COM initialization failed in another way, something is really wrong.
                panic!(
                    "Failed to initialize COM: {}",
                    std::io::Error::from_raw_os_error(result.0)
                );
            }
        }
    });

    /// RAII object that guards the fact that COM is initialized.
    ///
    // We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
    // object.
    struct ComInitializer {
        result: windows::core::HRESULT,
        _ptr: PhantomData<*mut ()>,
    }

    impl Drop for ComInitializer {
        #[inline]
        fn drop(&mut self) {
            // Need to avoid calling CoUninitialize() if CoInitializeEx failed since it may have
            // returned RPC_E_MODE_CHANGED - which is OK, see above.
            if self.result.is_ok() {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// Ensures that COM is initialized in this thread.
    #[inline]
    pub fn com_initializer() {
        COM_INITIALIZER.with(|_| {});
    }
}

/// Type of errors from the WASAPI backend.
#[derive(Debug, Error)]
#[error("WASAPI error: ")]
pub enum WasapiError {
    /// Error originating from WASAPI.
    BackendError(#[from] windows::core::Error),
}

/// The WASAPI driver.
#[derive(Debug, Clone, Default)]
pub struct WasapiDriver;

impl AudioDriver for WasapiDriver {
    type Error = WasapiError;
    type Device = WasapiDevice;

    const DISPLAY_NAME: &'static str = "WASAPI";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        Ok(Cow::Borrowed("WASAPI (version unknown)"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        audio_device_enumerator().get_default_device(device_type)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        audio_device_enumerator().get_device_list()
    }
}

/// Type of devices available from the WASAPI driver.
#[derive(Debug, Clone)]
pub struct WasapiDevice {
    device: windows::Win32::Media::Audio::IMMDevice,
    device_type: DeviceType,
    audio_client: Arc<Mutex<Option<IAudioClient>>>,
}

impl WasapiDevice {
    fn new(device: IMMDevice, device_type: DeviceType) -> Self {
        WasapiDevice {
            device,
            device_type,
            audio_client: Arc::new(Mutex::new(None)),
        }
    }
}

impl AudioDevice for WasapiDevice {
    type Error = WasapiError;

    fn name(&self) -> Cow<str> {
        match get_device_name(&self.device) {
            Some(std) => Cow::Owned(std),
            None => {
                eprintln!("Cannot get audio device name");
                Cow::Borrowed("<unknown>")
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        todo!()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        []
    }
}

// impl AudioInputDevice for WasapiDevice {
//     type StreamHandle<Callback: AudioInputCallback> = WasapiStream<Callback>;

//     fn create_input_stream<Callback: 'static + Send + AudioInputCallback>(
//         &self,
//         stream_config: StreamConfig,
//         callback: Callback,
//     ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
//         Ok(WasapiStream::new_input(
//             self.name.clone(),
//             stream_config,
//             callback,
//         ))
//     }
// }

impl AudioOutputDevice for WasapiDevice {
    type StreamHandle<Callback: AudioOutputCallback> = WasapiStream<Callback>;

    fn create_output_stream<Callback: 'static + Send + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        unsafe {
            let audio_client: Audio::IAudioClient = 
            // can fail if the device has been disconnected since we enumerated it, or if
            // the device doesn't support playback for some reason
            self.device.Activate(Com::CLSCTX_ALL, None)?;

            let format_attempt =
                config_to_waveformatextensible(&stream_config, sample_format).ok_or(err)?;

            let buffer_duration = buffer_size_to_duration(
                stream_config.buffer_size_range.0.unwrap_or(0),
                stream_config.samplerate.round() as u32,
            );

            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                buffer_duration,
                0,
                &format_attempt.Format,
                None,
            )?;

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client.GetBufferSize()? as usize;

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event_handle = {
                let event_handle =
                    Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))?;

                audio_client.SetEventHandle(event_handle)?;

                event_handle
            };

            let render_client = audio_client.GetService::<IAudioRenderClient>()?;

            let audio_clock = audio_client.GetService::<Audio::IAudioClock>()?;

            let stream_config = StreamConfig {
                samplerate: stream_config.samplerate,
                channels: stream_config.channels,
                buffer_size_range: (Some(max_frames_in_buffer), Some(max_frames_in_buffer)),
            };

            Ok(WasapiStream::new_output(
                audio_client,
                render_client,
                stream_config,
                callback,
            ))
        }
    }
}

fn config_to_waveformatextensible(
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Option<Audio::WAVEFORMATEXTENSIBLE> {
    let format_tag = match sample_format {
        SampleFormat::U8 | SampleFormat::I16 => Audio::WAVE_FORMAT_PCM,

        SampleFormat::I32 | SampleFormat::I64 | SampleFormat::F32 => {
            KernelStreaming::WAVE_FORMAT_EXTENSIBLE
        }

        _ => return None,
    };
    let channels = config.channels as u16;
    let sample_rate = config.samplerate as u32;
    let sample_bytes = sample_format.sample_size() as u16;
    let avg_bytes_per_sec = u32::from(channels) * sample_rate * u32::from(sample_bytes);
    let block_align = channels * sample_bytes;
    let bits_per_sample = 8 * sample_bytes;

    let cb_size = if format_tag == Audio::WAVE_FORMAT_PCM {
        0
    } else {
        let extensible_size = std::mem::size_of::<Audio::WAVEFORMATEXTENSIBLE>();
        let ex_size = std::mem::size_of::<Audio::WAVEFORMATEX>();
        (extensible_size - ex_size) as u16
    };

    let waveformatex = Audio::WAVEFORMATEX {
        wFormatTag: format_tag as u16,
        nChannels: channels as u16,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    let channel_mask = KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = match sample_format {
        SampleFormat::U8 | SampleFormat::I16 | SampleFormat::I32 | SampleFormat::I64 => {
            KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM
        }

        SampleFormat::F32 => Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        _ => return None,
    };

    let waveformatextensible = WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wSamplesPerBlock: bits_per_sample,
        },
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    Some(waveformatextensible)
}

pub enum AudioClientFlow {
    Render { render_client: IAudioRenderClient },
    Capture { capture_client: IAudioCaptureClient },
}

pub struct WasapiStream<Callback> {
    pub audio_client: Audio::IAudioClient,
    pub stream_config: StreamConfig,
    pub event_handle: HANDLE,
    pub join_handle: JoinHandle<Result<Callback, WasapiError>>,
    _p: PhantomData<*mut Callback>,
}

impl<Callback> AudioStreamHandle<Callback> for WasapiStream<Callback> {
    type Error = WasapiError;

    fn eject(self) -> Result<Callback, Self::Error> {
        unsafe {
            CloseHandle(self.event_handle)?;
        }

        self.join_handle.join().unwrap()
    }
}

// impl<Callback: 'static + Send + AudioInputCallback> WasapiStream<Callback> {
//     fn new_input(stream_config: StreamConfig, mut callback: Callback) -> Self {

//     }
// }

impl<Callback: 'static + Send + AudioOutputCallback> WasapiStream<Callback> {
    fn new_output(
        audio_client: IAudioClient,
        render_client: IAudioRenderClient,
        audio_clock: IAudioClock,
        stream_config: StreamConfig,
        event_handle: HANDLE,
        mut callback: Callback,
    ) -> Self {
        let eject_signal = Arc::new(AtomicBool::new(false));
        let join_handle = std::thread::spawn({
            let eject_signal = eject_signal.clone();
            move || {
                set_thread_priority();

                let _try = || loop {
                    if eject_signal.load(Ordering::Relaxed) {
                        break Ok(callback);
                    }
                    
                    // Get the number of available frames
                    let frames_available = unsafe {
                        let padding = audio_client.GetCurrentPadding()?;
                        stream_config.buffer_size_range.0.unwrap() as u32 - padding
                    };

                    unsafe {
                        let data = render_client.GetBuffer(frames_available)?;
                        
                        debug_assert!(!data.is_null());

                        let len = frames_available as usize * stream.bytes_per_frame as usize
                            / stream.sample_format.sample_size();
                        let data: &mut [u8] = std::slice::from_raw_parts_mut(buffer, len);
                        let timestamp =
                            output_timestamp(stream, frames_available, stream_config.samplerate)?;
                        let context = AudioCallbackContext {
                            stream_config,
                            timestamp,
                        };
                        let mut buffer = vec![0f32; (frames_available * stream_config.channels) as usize];
                        let output = AudioOutput {
                            buffer: AudioMut::from_interleaved_mut(
                                &mut buffer,
                                stream_config.channels as usize,
                            )
                            .unwrap(),
                            timestamp,
                        };
                        callback.on_output_data(context, output);

                        data.write_all()

                        render_client.ReleaseBuffer(frames_available, 0)?;
                    }
                };

                _try()
            }
        });

        WasapiStream {
            audio_client,
            stream_config,
            event_handle,
            join_handle,
            _p: PhantomData::default(),
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

fn get_device_name(device: &windows::Win32::Media::Audio::IMMDevice) -> Option<String> {
    unsafe {
        // Open the device's property store.
        let property_store = device
            .OpenPropertyStore(STGM_READ)
            .expect("could not open property store");

        // Get the endpoint's friendly-name property, else the interface's friendly-name, else the device description.
        let mut property_value = property_store
            .GetValue(&Properties::DEVPKEY_Device_FriendlyName as *const _ as *const _)
            .or(property_store.GetValue(
                &Properties::DEVPKEY_DeviceInterface_FriendlyName as *const _ as *const _,
            ))
            .or(property_store
                .GetValue(&Properties::DEVPKEY_Device_DeviceDesc as *const _ as *const _))
            .ok()?;

        let prop_variant = &property_value.as_raw().Anonymous.Anonymous;

        // Read the friendly-name from the union data field, expecting a *const u16.
        if prop_variant.vt != VT_LPWSTR.0 {
            return None;
        }

        let ptr_utf16 = *(&prop_variant.Anonymous as *const _ as *const *const u16);

        // Find the length of the friendly name.
        let mut len = 0;
        while *ptr_utf16.offset(len) != 0 {
            len += 1;
        }

        // Convert to a string.
        let name_slice = std::slice::from_raw_parts(ptr_utf16, len as usize);
        let name_os_string: OsString = OsStringExt::from_wide(name_slice);
        let name = name_os_string
            .into_string()
            .unwrap_or_else(|os_string| os_string.to_string_lossy().into());

        // Clean up.
        StructuredStorage::PropVariantClear(&mut property_value).ok()?;

        Some(name)
    }
}

static ENUMERATOR: OnceLock<AudioDeviceEnumerator> = OnceLock::new();

fn audio_device_enumerator() -> &'static AudioDeviceEnumerator {
    ENUMERATOR.get_or_init(|| {
        // Make sure COM is initialised.
        util::com_initializer();

        unsafe {
            let enumerator = Com::CoCreateInstance::<_, Audio::IMMDeviceEnumerator>(
                &Audio::MMDeviceEnumerator,
                None,
                Com::CLSCTX_ALL,
            )
            .unwrap();

            AudioDeviceEnumerator(enumerator)
        }
    })
}

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
struct AudioDeviceEnumerator(Audio::IMMDeviceEnumerator);

impl AudioDeviceEnumerator {
    // Returns the default output device.
    fn get_default_device(
        &self,
        device_type: DeviceType,
    ) -> Result<Option<WasapiDevice>, WasapiError> {
        let data_flow = match device_type {
            DeviceType::Input => Audio::eCapture,
            DeviceType::Output => Audio::eRender,
            _ => return Ok(None),
        };

        unsafe {
            let device = self.0.GetDefaultAudioEndpoint(data_flow, Audio::eConsole)?;

            Ok(Some(WasapiDevice::new(device, DeviceType::Output)))
        }
    }

    // Returns a chained iterator of output and input devices.
    fn get_device_list(&self) -> Result<impl IntoIterator<Item = WasapiDevice>, WasapiError> {
        // Create separate collections for output and input devices and then chain them.
        unsafe {
            let output_collection = self
                .0
                .EnumAudioEndpoints(Audio::eRender, Audio::DEVICE_STATE_ACTIVE)?;

            let count = output_collection.GetCount()?;

            let output_device_list = WasapiDeviceList {
                collection: output_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::Output,
            };

            let input_collection = self
                .0
                .EnumAudioEndpoints(Audio::eCapture, Audio::DEVICE_STATE_ACTIVE)?;

            let count = input_collection.GetCount()?;

            let input_device_list = WasapiDeviceList {
                collection: input_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::Input,
            };

            Ok(output_device_list.chain(input_device_list))
        }
    }
}

unsafe impl Send for AudioDeviceEnumerator {}
unsafe impl Sync for AudioDeviceEnumerator {}

/// An iterable collection WASAPI devices.
pub struct WasapiDeviceList {
    collection: Audio::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
    device_type: DeviceType,
}

unsafe impl Send for WasapiDeviceList {}
unsafe impl Sync for WasapiDeviceList {}

impl Iterator for WasapiDeviceList {
    type Item = WasapiDevice;

    fn next(&mut self) -> Option<WasapiDevice> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
            self.next_item += 1;
            Some(WasapiDevice::new(device, self.device_type))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rest = (self.total_count - self.next_item) as usize;
        (rest, Some(rest))
    }
}

fn buffer_size_to_duration(buffer_size: usize, sample_rate: u32) -> i64 {
    (buffer_size as i64 / sample_rate as i64) * (1_000_000_000 / 100)
}

fn buffer_duration_to_frames(buffer_duration: i64, sample_rate: u32) -> i64 {
    (buffer_duration * sample_rate as i64) / (100 / 1_000_000_000)
}

/// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: u32, samplerate: f64) -> std::time::Duration {
    let secsf = frames as f64 / samplerate;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

fn stream_instant(audio_clock: &IAudioClock) -> Result<Duration, WasapiError> {
    let mut position: u64 = 0;
    let mut qpc_position: u64 = 0;
    unsafe {
        audio_clock
            .GetPosition(&mut position, Some(&mut qpc_position))?;
    };
    // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
    let qpc_nanos = qpc_position as u64 * 100;
    let instant = Duration::from_nanos(qpc_nanos);
    Ok(instant)
}

fn output_timestamp(
    audio_clock: &IAudioClock,
    frames_available: u32,
    samplerate: f64,
) -> Result<Timestamp, WasapiError> {
    let callback = stream_instant(audio_clock)?;
    let buffer_duration = frames_to_duration(frames_available, samplerate);
    let playback = callback
        .add(buffer_duration);
    Ok(Timestamp::from_duration(samplerate, playback))
}