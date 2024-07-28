//! # ALSA backend
//! 
//! ALSA is a generally available driver for Linux and BSD systems. It is the oldest of the Linux
//! drivers supported in this library, and as such makes it a good fallback driver. Newer drivers
//! (PulseAudio, PipeWire) offer ALSA-compatible APIs so that older software can still access the
//! audio devices through them.

use core::fmt;
use std::{borrow::Cow, convert::Infallible, ffi::CStr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use alsa::{device_name::HintIter, pcm, PCM};
use thiserror::Error;

use crate::{
    AudioCallbackContext, AudioDevice, AudioDriver, AudioInput, AudioInputCallback,
    AudioInputDevice, AudioOutput, AudioOutputCallback, AudioOutputDevice, AudioStreamHandle,
    Channel, DeviceType, StreamConfig,
};
use crate::audio_buffer::{AudioMut, AudioRef};
use crate::channel_map::{Bitset, ChannelMap32};
use crate::timestamp::Timestamp;

/// Type of errors from using the ALSA backend.
#[derive(Debug, Error)]
#[error("ALSA error: ")]
pub enum AlsaError {
    /// Error originates from ALSA itself.
    #[error("{0}")]
    BackendError(#[from] alsa::Error),
}

/// ALSA driver type. ALSA is statically available without client configuration, therefore this type
/// is zero-sized.
#[derive(Debug, Clone, Default)]
pub struct AlsaDriver;

impl AudioDriver for AlsaDriver {
    type Error = AlsaError;
    type Device = AlsaDevice;

    const DISPLAY_NAME: &'static str = "ALSA";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        Ok(Cow::Borrowed("ALSA (version unknown)"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        Ok(AlsaDevice::default_device(device_type)?)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        const C_PCM: &CStr = match CStr::from_bytes_with_nul(b"pcm\0") {
            Ok(cstr) => cstr,
            Err(_) => unreachable!(),
        };
        Ok(HintIter::new(None, c"pcm")?
            .filter_map(|hint| AlsaDevice::new(hint.name.as_ref()?, hint.direction?).ok()))
    }
}

/// Type of ALSA devices. 
#[derive(Clone)]
pub struct AlsaDevice {
    pcm: Arc<PCM>,
    name: String,
    direction: alsa::Direction,
}

impl fmt::Debug for AlsaDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlsaDevice")
            .field("name", &self.name)
            .field("direction", &format!("{:?}", self.direction))
            .finish_non_exhaustive()
    }
}

impl AudioDevice for AlsaDevice {
    type Error = Infallible;

    fn name(&self) -> Cow<str> {
        Cow::Borrowed(self.name.as_str())
    }

    fn device_type(&self) -> DeviceType {
        match self.direction {
            alsa::Direction::Playback => DeviceType::Output,
            alsa::Direction::Capture => DeviceType::Input,
        }
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        []
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        self.get_hwp(config).is_ok()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }
}

impl AudioInputDevice for AlsaDevice {
    type StreamHandle<Callback: AudioInputCallback> = AlsaStream<Callback>;

    fn create_input_stream<Callback: 'static + Send + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        Ok(AlsaStream::new_input(
            self.name.clone(),
            stream_config,
            callback,
        ))
    }
}

impl AudioOutputDevice for AlsaDevice {
    type StreamHandle<Callback: AudioOutputCallback> = AlsaStream<Callback>;

    fn create_output_stream<Callback: 'static + Send + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        Ok(AlsaStream::new_output(
            self.name.clone(),
            stream_config,
            callback,
        ))
    }
}

impl AlsaDevice {
    /// Shortcut constructor for getting ALSA devices directly.
    pub fn default_device(device_type: DeviceType) -> Result<Option<Self>, alsa::Error> {
        let direction = match device_type {
            DeviceType::Input => alsa::Direction::Capture,
            DeviceType::Output => alsa::Direction::Playback,
            _ => return Ok(None),
        };
        let pcm = Arc::new(PCM::new("default", direction, true)?);
        Ok(Some(Self {
            pcm,
            direction,
            name: "default".to_string(),
        }))
    }

    fn new(name: &str, direction: alsa::Direction) -> Result<Self, alsa::Error> {
        let pcm = PCM::new(name, direction, true)?;
        let pcm = Arc::new(pcm);
        Ok(Self {
            name: name.to_string(),
            direction,
            pcm,
        })
    }

    fn get_hwp(&self, config: &StreamConfig) -> Result<pcm::HwParams, alsa::Error> {
        let hwp = pcm::HwParams::any(&self.pcm)?;
        hwp.set_channels(config.channels as _)?;
        hwp.set_rate(config.samplerate as _, alsa::ValueOr::Nearest)?;
        hwp.set_format(pcm::Format::float())?;
        hwp.set_access(pcm::Access::RWInterleaved)?;
        Ok(hwp)
    }

    fn apply_config(
        &self,
        config: &StreamConfig,
    ) -> Result<(pcm::HwParams, pcm::SwParams, pcm::IO<f32>), alsa::Error> {
        let hwp = self.get_hwp(config)?;
        self.pcm.hw_params(&hwp)?;
        let io = self.pcm.io_f32()?;
        let hwp = self.pcm.hw_params_current()?;
        let swp = self.pcm.sw_params_current()?;

        // TODO: Forward buffer size hints

        swp.set_start_threshold(hwp.get_buffer_size()?)?;
        self.pcm.sw_params(&swp)?;
        Ok((hwp, swp, io))
    }
}

/// Type of ALSA streams.
/// 
/// The audio stream implementation relies on the synchronous API for now, as the [`alsa`] crate 
/// does not seem to wrap the asynchronous API as of now. A separate I/O thread is spawned when 
/// creating a stream, and is stopped when caling [`AudioInputDevice::eject`] /
/// [`AudioOutputDevice::eject`].
pub struct AlsaStream<Callback> {
    eject_signal: Arc<AtomicBool>,
    join_handle: JoinHandle<Result<Callback, AlsaError>>,
}

impl<Callback> AudioStreamHandle<Callback> for AlsaStream<Callback> {
    type Error = AlsaError;

    fn eject(self) -> Result<Callback, Self::Error> {
        self.eject_signal.store(true, Ordering::Relaxed);
        self.join_handle.join().unwrap()
    }
}

impl<Callback: 'static + Send + AudioInputCallback> AlsaStream<Callback> {
    fn new_input(name: String, stream_config: StreamConfig, mut callback: Callback) -> Self {
        let eject_signal = Arc::new(AtomicBool::new(false));
        let join_handle = std::thread::spawn({
            let eject_signal = eject_signal.clone();
            move || {
                let device = AlsaDevice::new(&name, alsa::Direction::Capture)?;
                let (hwp, _, io) = device.apply_config(&stream_config)?;
                let (_, period_size) = device.pcm.get_params()?;
                let period_size = period_size as usize;
                eprintln!("Period size: {period_size}");
                let num_channels = hwp.get_channels()? as usize;
                let samplerate = hwp.get_rate()? as f64;
                let stream_config = StreamConfig {
                    samplerate,
                    channels: ChannelMap32::default()
                        .with_indices(std::iter::repeat(1).take(num_channels)),
                    buffer_size_range: (Some(period_size), Some(period_size)),
                };
                let mut timestamp = Timestamp::new(samplerate);
                let mut buffer = vec![0f32; period_size * num_channels];
                device.pcm.prepare()?;
                if device.pcm.state() != pcm::State::Running {
                    device.pcm.start()?;
                }
                let _try = || loop {
                    if eject_signal.load(Ordering::Relaxed) {
                        break Ok(callback);
                    }
                    let frames = device.pcm.avail_update()? as usize;
                    let len = frames * num_channels;
                    match io.readi(&mut buffer[..len]) {
                        Err(err) => device.pcm.try_recover(err, true)?,
                        _ => {}
                    }
                    let buffer = AudioRef::from_interleaved(&buffer[..len], num_channels).unwrap();
                    let context = AudioCallbackContext {
                        stream_config,
                        timestamp,
                    };
                    let input = AudioInput { buffer, timestamp };
                    callback.on_input_data(context, input);
                    timestamp += frames as u64;
                    
                    match device.pcm.state() {
                        pcm::State::Suspended => {
                            if hwp.can_resume() {
                                device.pcm.resume()?;
                            } else {
                                device.pcm.prepare()?;
                            }
                        }
                        pcm::State::Paused => std::thread::sleep(Duration::from_secs(1)),
                        _ => {}
                    }
                };
                _try()
            }
        });
        Self {
            eject_signal,
            join_handle,
        }
    }
}

impl<Callback: 'static + Send + AudioOutputCallback> AlsaStream<Callback> {
    fn new_output(name: String, stream_config: StreamConfig, mut callback: Callback) -> Self {
        let eject_signal = Arc::new(AtomicBool::new(false));
        let join_handle = std::thread::spawn({
            let eject_signal = eject_signal.clone();
            move || {
                let device = AlsaDevice::new(&name, alsa::Direction::Playback)?;
                let (hwp, _, io) = device.apply_config(&stream_config)?;
                let (_, period_size) = device.pcm.get_params()?;
                let period_size = period_size as usize;
                let num_channels = hwp.get_channels()? as usize;
                let samplerate = hwp.get_rate()? as f64;
                let stream_config = StreamConfig {
                    samplerate,
                    channels: ChannelMap32::default()
                        .with_indices(std::iter::repeat(1).take(num_channels)),
                    buffer_size_range: (Some(period_size), Some(period_size)),
                };
                let frames = device.pcm.avail_update()? as usize;
                let mut timestamp = Timestamp::new(samplerate);
                let mut buffer = vec![0f32; frames * num_channels];
                device.pcm.prepare()?;
                if device.pcm.state() != pcm::State::Running {
                    device.pcm.start()?;
                }
                let _try = || loop {
                    if eject_signal.load(Ordering::Relaxed) {
                        break Ok(callback);
                    }
                    let frames = device.pcm.avail_update()? as usize;
                    let len = frames * num_channels;
                    let context = AudioCallbackContext {
                        stream_config,
                        timestamp,
                    };
                    let input = AudioOutput {
                        buffer: AudioMut::from_interleaved_mut(&mut buffer[..len], num_channels)
                            .unwrap(),
                        timestamp,
                    };
                    callback.on_output_data(context, input);
                    timestamp += frames as u64;
                    match io.writei(&buffer[..len]) {
                        Err(err) => device.pcm.try_recover(err, true)?,
                        _ => {}
                    }
                    match device.pcm.state() {
                        pcm::State::Suspended => {
                            if hwp.can_resume() {
                                device.pcm.resume()?;
                            } else {
                                device.pcm.prepare()?;
                            }
                        }
                        pcm::State::Paused => std::thread::sleep(Duration::from_secs(1)),
                        _ => {}
                    }
                };
                _try().inspect_err(|err| eprintln!("Audio thread error: {err}"))
            }
        });
        Self {
            eject_signal,
            join_handle,
        }
    }
}
