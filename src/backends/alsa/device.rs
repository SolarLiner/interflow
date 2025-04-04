use crate::backends::alsa::AlsaError;
use crate::device::Channel;
use crate::device::{AudioDevice, AudioInputDevice, AudioOutputDevice, DeviceType};
use crate::stream::{AudioInputCallback, AudioOutputCallback, StreamConfig};
use crate::{
    backends::alsa::stream::AlsaStream, device::AudioDuplexDevice, duplex::AudioDuplexCallback,
    SendEverywhereButOnWeb,
};
use alsa::{pcm, PCM};
use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;

/// Type of ALSA devices.
#[derive(Clone)]
pub struct AlsaDevice {
    pub(super) pcm: Rc<PCM>,
    pub(super) name: String,
    pub(super) direction: alsa::Direction,
}

impl AlsaDevice {
    fn channel_map(&self, requested_direction: alsa::Direction) -> impl Iterator<Item = Channel> {
        let max_channels = if self.direction == requested_direction {
            self.pcm
                .hw_params_current()
                .and_then(|hwp| hwp.get_channels_max())
                .unwrap_or(0)
        } else {
            0
        };
        (0..max_channels as usize).map(|i| Channel {
            index: i,
            name: Cow::Owned(format!("Channel {}", i)),
        })
    }
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
    type Error = AlsaError;

    fn name(&self) -> Cow<str> {
        Cow::Borrowed(self.name.as_str())
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        self.get_hwp(config)
            .inspect_err(|err| {
                log::debug!("{config:#?}");
                log::debug!("Configuration unsupported: {err}");
            })
            .is_ok()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        log::info!("TODO: enumerate configurations");
        None::<[StreamConfig; 0]>
    }
}

impl AudioInputDevice for AlsaDevice {
    fn input_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    type StreamHandle<Callback: AudioInputCallback> = AlsaStream<Callback>;

    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        self.default_config()
    }

    fn create_input_stream<Callback: 'static + Send + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        AlsaStream::new_input(self.name.clone(), stream_config, callback)
    }
}

impl AudioOutputDevice for AlsaDevice {
    fn output_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    type StreamHandle<Callback: AudioOutputCallback> = AlsaStream<Callback>;

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        self.default_config()
    }

    fn create_output_stream<Callback: 'static + Send + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        AlsaStream::new_output(self.name.clone(), stream_config, callback)
    }
}

impl AlsaDevice {
    /// Shortcut constructor for getting ALSA devices directly.
    pub fn default_device(device_type: DeviceType) -> Result<Option<Self>, alsa::Error> {
        let direction = match device_type {
            DeviceType::Input => alsa::Direction::Capture,
            DeviceType::Output => alsa::Direction::Playback,
        };
        let pcm = Rc::new(PCM::new("default", direction, true)?);
        Ok(Some(Self {
            pcm,
            direction,
            name: "default".to_string(),
        }))
    }

    pub(super) fn new(name: &str, direction: alsa::Direction) -> Result<Self, alsa::Error> {
        log::info!("Opening device: {name}, direction {direction:?}");
        let pcm = Rc::new(PCM::new(name, direction, true)?);
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
        if let Some(min) = config.buffer_size_range.0 {
            hwp.set_buffer_size_min(min as pcm::Frames * 2)?;
        }
        if let Some(max) = config.buffer_size_range.1 {
            hwp.set_buffer_size_max(max as pcm::Frames * 2)?;
        }
        hwp.set_periods(2, alsa::ValueOr::Nearest)?;
        hwp.set_format(pcm::Format::float())?;
        hwp.set_access(pcm::Access::RWInterleaved)?;
        Ok(hwp)
    }

    pub(super) fn apply_config(
        &self,
        config: &StreamConfig,
    ) -> Result<(pcm::HwParams, pcm::SwParams, pcm::IO<f32>), alsa::Error> {
        let hwp = self.get_hwp(config)?;
        self.pcm.hw_params(&hwp)?;
        let io = self.pcm.io_f32()?;
        let hwp = self.pcm.hw_params_current()?;
        let swp = self.pcm.sw_params_current()?;

        log::debug!("Apply config: hwp {hwp:#?}");

        swp.set_avail_min(hwp.get_period_size()?)?;
        swp.set_start_threshold(hwp.get_buffer_size()?)?;
        self.pcm.sw_params(&swp)?;
        log::debug!("Apply config: swp {swp:#?}");

        Ok((hwp, swp, io))
    }

    pub(super) fn ensure_state(&self, hwp: &pcm::HwParams) -> Result<bool, AlsaError> {
        match self.pcm.state() {
            pcm::State::Suspended if hwp.can_resume() => self.pcm.resume()?,
            pcm::State::Suspended => self.pcm.prepare()?,
            pcm::State::Paused => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn default_config(&self) -> Result<StreamConfig, AlsaError> {
        let samplerate = 48e3; // Default ALSA sample rate
        let channel_count = 2; // Stereo stream
        let channels = (1 << channel_count) - 1;
        Ok(StreamConfig {
            samplerate: samplerate as _,
            channels,
            buffer_size_range: (None, None),
            exclusive: false,
        })
    }
}

pub struct AlsaDuplexDevice {
    pub(super) input: AlsaDevice,
    pub(super) output: AlsaDevice,
}

impl AudioDevice for AlsaDuplexDevice {
    type Error = AlsaError;

    fn name(&self) -> Cow<str> {
        Cow::Owned(format!("{} / {}", self.input.name(), self.output.name()))
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        let Ok((hwp, _, _)) = self.output.apply_config(config) else {
            return false;
        };
        let Ok(period) = hwp.get_period_size() else {
            return false;
        };
        let period = period as usize;
        self.input
            .apply_config(&StreamConfig {
                buffer_size_range: (Some(period), Some(period)),
                ..*config
            })
            .is_ok()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        Some(
            self.output
                .enumerate_configurations()?
                .into_iter()
                .filter(|config| self.is_config_supported(config)),
        )
    }
}

impl AudioDuplexDevice for AlsaDuplexDevice {
    type StreamHandle<Callback: AudioDuplexCallback> = AlsaStream<Callback>;

    fn default_duplex_config(&self) -> Result<StreamConfig, Self::Error> {
        self.output.default_output_config()
    }

    fn create_duplex_stream<Callback: SendEverywhereButOnWeb + AudioDuplexCallback>(
        &self,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<<Self as AudioDuplexDevice>::StreamHandle<Callback>, Self::Error> {
        AlsaStream::new_duplex(
            config,
            self.input.name.clone(),
            self.output.name.clone(),
            callback,
        )
    }
}

impl AlsaDuplexDevice {
    /// Create a new duplex device from an input and output device.
    pub fn new(input: AlsaDevice, output: AlsaDevice) -> Self {
        Self { input, output }
    }

    /// Create a full-duplex device from the given name.
    pub fn full_duplex(name: &str) -> Result<Self, AlsaError> {
        Ok(Self::new(
            AlsaDevice::new(name, alsa::Direction::Capture)?,
            AlsaDevice::new(name, alsa::Direction::Playback)?,
        ))
    }
}
