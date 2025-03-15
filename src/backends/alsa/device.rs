use crate::backends::alsa::stream::AlsaStream;
use crate::backends::alsa::AlsaError;
use crate::{
    AudioDevice, AudioInputCallback, AudioInputDevice, AudioOutputCallback, AudioOutputDevice,
    Channel, DeviceType, StreamConfig,
};
use alsa::{pcm, PCM};
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

/// Type of ALSA devices.
#[derive(Clone)]
pub struct AlsaDevice {
    pub(super) pcm: Arc<PCM>,
    pub(super) name: String,
    pub(super) direction: alsa::Direction,
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
            _ => return Ok(None),
        };
        let pcm = Arc::new(PCM::new("default", direction, true)?);
        Ok(Some(Self {
            pcm,
            direction,
            name: "default".to_string(),
        }))
    }

    pub(super) fn new(name: &str, direction: alsa::Direction) -> Result<Self, alsa::Error> {
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
        if let Some(min) = config.buffer_size_range.0 {
            hwp.set_buffer_size_min(min as _)?;
        }
        if let Some(max) = config.buffer_size_range.1 {
            hwp.set_buffer_size_max(max as _)?;
        }
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

        swp.set_start_threshold(hwp.get_buffer_size()?)?;
        self.pcm.sw_params(&swp)?;
        log::debug!("Apply config: swp {swp:#?}");

        Ok((hwp, swp, io))
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
