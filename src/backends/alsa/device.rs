use crate::backends::alsa::stream::AlsaStream;
use crate::backends::alsa::AlsaError;
use crate::{
    AudioCallback, AudioDevice, Channel, DeviceType, StreamConfig,
};
use alsa::{pcm, Direction, PCM};
use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

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
            .map(|x| x as usize)
            .inspect_err(|err| log::warn!("Cannot get buffer size: {err}"))
            .ok();
        let max = params
            .get_buffer_size_max()
            .map(|x| x as usize)
            .inspect_err(|err| log::warn!("Cannot get buffer size: {err}"))
            .ok();

        let channels = params.get_channels()? as usize;
        let (input_channels, output_channels) =
            if matches!(self.direction, alsa::Direction::Capture) {
                (channels, 0)
            } else {
                (0, channels)
            };

        Ok(StreamConfig {
            sample_rate: params.get_rate()? as _,
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

    fn create_stream<Callback: Send + AudioCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let name = Arc::<str>::from(self.name.to_owned());
        let direction = self.direction;
        let input_device = {
            let name = name.clone();
            move || match direction {
                alsa::Direction::Capture => Ok(Some(AlsaDevice::new(
                    &name.clone(),
                    alsa::Direction::Capture,
                )?)),
                alsa::Direction::Playback => Ok(None),
            }
        };
        let output_device = move || match direction {
            alsa::Direction::Capture => Ok(None),
            alsa::Direction::Playback => Ok(Some(AlsaDevice::new(
                &name.clone(),
                alsa::Direction::Playback,
            )?)),
        };
        AlsaStream::new(input_device, output_device, stream_config, callback)
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

        if matches!(self.direction, alsa::Direction::Playback) {
            hwp.set_channels(config.output_channels as _)?;
        } else {
            hwp.set_channels(config.input_channels as _)?;
        }

        swp.set_start_threshold(hwp.get_buffer_size()?)?;
        self.pcm.sw_params(&swp)?;
        log::debug!("Apply config: swp {swp:#?}");

        Ok((hwp, swp, io))
    }

    fn get_hwp(&self, config: &StreamConfig) -> Result<pcm::HwParams, alsa::Error> {
        let hwp = pcm::HwParams::any(&self.pcm)?;
        hwp.set_channels(config.output_channels as _)?;
        hwp.set_rate(config.sample_rate as _, alsa::ValueOr::Nearest)?;
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
}

#[derive(Debug, Clone)]
pub struct AlsaDuplex {
    pub capture: AlsaDevice,
    pub playback: AlsaDevice,
}

impl AudioDevice for AlsaDuplex {
    type StreamHandle<Callback: AudioCallback> = AlsaStream<Callback>;

    type Error = AlsaError;

    fn name(&self) -> Cow<str> {
        Cow::Owned(format!("{} / {}", &self.capture.name, &self.playback.name))
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::PHYSICAL | DeviceType::DUPLEX
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        []
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        let hwp_inp = self.capture.pcm.hw_params_current()?;
        let hwp_out = self.playback.pcm.hw_params_current()?;
        let sample_rate = hwp_out.get_rate()? as f64;
        let input_channels = hwp_inp.get_channels()? as usize;
        let output_channels = hwp_out.get_channels()? as usize;
        let min_size = {
            let inp_min = hwp_inp.get_buffer_size_min().unwrap_or(0);
            let out_min = hwp_out.get_buffer_size_min().unwrap_or(0);
            inp_min.max(out_min)
        };
        let max_size = {
            let inp_max = hwp_inp.get_buffer_size_max().unwrap_or(0);
            let out_max = hwp_out.get_buffer_size_max().unwrap_or(0);
            inp_max.min(out_max)
        };
        Ok(StreamConfig {
            sample_rate,
            input_channels,
            output_channels,
            buffer_size_range: (
                (min_size == 0).then_some(min_size as _),
                (max_size == 0).then_some(max_size as _),
            ),
            exclusive: false,
        })
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        self.capture.is_config_supported(config) && self.playback.is_config_supported(config)
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }

    fn create_stream<Callback: Send + AudioCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let input_device = {
            let name = self.capture.name.to_owned();
            move || AlsaDevice::new(&name, alsa::Direction::Capture).map(Some)
        };
        let output_device = {
            let name = self.playback.name.to_owned();
            move || AlsaDevice::new(&name, alsa::Direction::Playback).map(Some)
        };
        AlsaStream::new(input_device, output_device, stream_config, callback)
    }
}
