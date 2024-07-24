use core::fmt;
use std::{
    borrow::Cow,
    convert::Infallible,
    ffi::CStr,
    marker::PhantomData, rc::Rc,
};

use alsa::{device_name::HintIter, pcm::{self, HwParams, SwParams}, PCM};
use thiserror::Error;

use crate::{AudioDevice, AudioDriver, AudioStream, StreamConfig};

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

    fn default_device(&self) -> Result<Self::Device, Self::Error> {
        Ok(AlsaDevice::default_device()?)
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

    type Stream<Callback> = AlsaStream<Callback>;

    fn name(&self) -> Cow<str> {
        todo!()
    }

    fn device_type(&self) -> crate::DeviceType {
        todo!()
    }

    fn is_config_supported(&self, config: &crate::StreamConfig) -> bool {
        self.get_hwp(config).is_ok()
    }

    fn enumerate_configurations(&self) -> impl IntoIterator<Item = crate::StreamConfig> {
        []
    }

    fn create_stream<Callback>(
        &self,
        config: crate::StreamConfig,
        callback: Callback,
    ) -> Result<Self::Stream<Callback>, Self::Error> {
        todo!()
    }
}

impl AlsaDevice {
    pub fn default_device() -> Result<Self, alsa::Error> {
        Self::new("default", alsa::Direction::Playback)
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

    fn apply_config(&self, config: &StreamConfig) -> Result<pcm::IO<>, alsa::Error> {
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

pub struct AlsaStream<Callback> {
    pcm: Rc<PCM>,
    __callback: PhantomData<Callback>,
}

impl<Callback> AudioStream<Callback> for AlsaStream<Callback> {
    type Error = Infallible;

    fn start(&self) -> Result<(), Self::Error> {
        todo!()
    }

    fn stop(&self) -> Result<(), Self::Error> {
        todo!()
    }

    fn eject(self) -> Callback {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use crate::AudioDriver;

    use super::AlsaDriver;

    #[test]
    fn test_enumeration() {
        let driver = AlsaDriver::default();
        eprintln!("Driver name   : {}", AlsaDriver::DISPLAY_NAME);
        eprintln!("Driver version: {}", driver.version().unwrap());
        eprintln!("Default device: {:?}", driver.default_device().unwrap());
        eprintln!("All devices   :");
        for device in driver.list_devices().unwrap() {
            eprintln!("\t{:?}", device);
        }
    }
}
