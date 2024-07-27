use core::fmt;
use std::{
    borrow::Cow,
    convert::Infallible,
    ffi::CStr
    , rc::Rc,
};

use alsa::{device_name::HintIter, pcm::{self, HwParams}, PCM};
use thiserror::Error;

use crate::{AudioDevice, AudioDriver, Channel, DeviceType, StreamConfig};

#[derive(Debug, Error)]
#[error("ALSA error: ")]
pub enum AlsaError {
    BackendError(#[from] alsa::Error),
}

#[derive(Debug, Clone, Default)]
pub struct AlsaDriver {}

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

    fn list_devices(&self) -> Result<impl IntoIterator<Item=Self::Device>, Self::Error> {
        const C_PCM: &CStr = match CStr::from_bytes_with_nul(b"pcm\0") {
            Ok(cstr) => cstr,
            Err(_) => unreachable!(),
        };
        Ok(HintIter::new(None, c"pcm")?
            .filter_map(|hint| AlsaDevice::new(hint.name.as_ref()?, hint.direction?).ok()))
    }
}

pub struct AlsaDevice {
    pcm: Rc<PCM>,
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

    fn channel_map(&self) -> impl IntoIterator<Item=Channel> {
        []
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        self.get_hwp(config).is_ok()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item=StreamConfig>> {
        None::<[StreamConfig; 0]>
    }
}

impl AlsaDevice {
    pub fn default_device(device_type: DeviceType) -> Result<Option<Self>, alsa::Error> {
        let direction = match device_type {
            DeviceType::Input => alsa::Direction::Capture,
            DeviceType::Output => alsa::Direction::Playback,
            _ => return Ok(None),
        };
        let pcm = Rc::new(PCM::new("default", direction, true)?);
        Ok(Some(Self { pcm, direction, name: "default".to_string() }))
    }

    fn new(name: &str, direction: alsa::Direction) -> Result<Self, alsa::Error> {
        let pcm = PCM::new(name, direction, true)?;
        let pcm = Rc::new(pcm);
        Ok(Self {
            name: name.to_string(),
            direction,
            pcm,
        })
    }

    fn get_hwp(&self, config: &StreamConfig) -> Result<HwParams, alsa::Error> {
        let hwp = HwParams::any(&self.pcm)?;
        hwp.set_channels(config.channels as _)?;
        hwp.set_rate(config.samplerate as _, alsa::ValueOr::Nearest)?;
        hwp.set_format(pcm::Format::float())?;
        hwp.set_access(pcm::Access::RWNonInterleaved)?;
        Ok(hwp)
    }

    fn apply_config(&self, config: &StreamConfig) -> Result<pcm::IO<f32>, alsa::Error> {
        let hwp = self.get_hwp(config)?;
        self.pcm.hw_params(&hwp)?;
        let io = self.pcm.io_f32()?;
        let hwp = self.pcm.hw_params_current()?;
        let swp = self.pcm.sw_params_current()?;

        // TODO: Forward buffer size hints

        swp.set_start_threshold(hwp.get_buffer_size()?)?;
        self.pcm.sw_params(&swp)?;
        Ok(io)
    }
}
