use crate::backends::alsa::stream::AlsaStream;
use crate::backends::alsa::AlsaError;
use crate::{
    AudioCallback, AudioDevice, Channel, DeviceType, SendEverywhereButOnWeb, StreamConfig,
};
use alsa::{pcm, Direction, PCM};
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

impl fmt::Debug for AlsaDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlsaDevice")
            .field("name", &self.name)
            .field("direction", &format!("{:?}", self.direction))
            .finish_non_exhaustive()
    }
}

impl AudioDevice for AlsaDevice {
    type StreamHandle<Callback: AudioCallback> = AlsaStream<Callback>;
    type Error = AlsaError;

    fn name(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name.as_str())
    }

    fn device_type(&self) -> DeviceType {
        match self.direction {
            alsa::Direction::Capture => DeviceType::PHYSICAL | DeviceType::INPUT,
            alsa::Direction::Playback => DeviceType::PHYSICAL | DeviceType::OUTPUT,
        }
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel<'_>> {
        []
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        let params = self.pcm.hw_params_current()?;
        let min = params
            .get_buffer_size_min()
            .inspect_err(|err| log::warn!("Cannot get buffer size: {err}"))
            .ok()
            .map(|x| x as usize);
        let max = params
            .get_buffer_size_max()
            .inspect_err(|err| log::warn!("Cannot get buffer size: {err}"))
            .ok()
            .map(|x| x as usize);

        let channels = params.get_channels()? as usize;
        let (input_channels, output_channels) =
            if matches!(self.direction, alsa::Direction::Capture) {
                (channels, 0)
            } else {
                (0, channels)
            };

        Ok(StreamConfig {
            samplerate: params.get_rate()? as _,
            buffer_size_range: (min, max),
            exclusive: false,
            input_channels,
            output_channels,
        })
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

    fn create_stream<Callback: SendEverywhereButOnWeb + AudioCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        match self.direction {
            Direction::Playback => {
                AlsaStream::new_output(self.name.clone(), stream_config, callback)
            }
            Direction::Capture => AlsaStream::new_input(self.name.clone(), stream_config, callback),
        }
    }
}

impl AlsaDevice {
    /// Shortcut constructor for getting ALSA devices directly.
    pub fn default_device(direction: alsa::Direction) -> Result<Self, alsa::Error> {
        Self::new("default", direction)
    }

    pub(super) fn new(name: &str, direction: alsa::Direction) -> Result<Self, alsa::Error> {
        let pcm = PCM::new(name, direction, true)?;
        let pcm = Rc::new(pcm);
        Ok(Self {
            name: name.to_string(),
            direction,
            pcm,
        })
    }

    fn get_hwp(&self, config: &StreamConfig) -> Result<pcm::HwParams<'_>, alsa::Error> {
        let hwp = pcm::HwParams::any(&self.pcm)?;
        hwp.set_channels(config.output_channels as _)?;
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
    ) -> Result<(pcm::HwParams<'_>, pcm::SwParams<'_>, pcm::IO<'_, f32>), alsa::Error> {
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
}
