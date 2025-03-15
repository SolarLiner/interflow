//! # ALSA backend
//!
//! ALSA is a generally available driver for Linux and BSD systems. It is the oldest of the Linux
//! drivers supported in this library, and as such makes it a good fallback driver. Newer drivers
//! (PulseAudio, PipeWire) offer ALSA-compatible APIs so that older software can still access the
//! audio devices through them.

use crate::{device::DeviceType, driver::AudioDuplexDriver};
use crate::driver::AudioDriver;
use alsa::device_name::HintIter;
use device::{AlsaDevice, AlsaDuplexDevice};
use std::borrow::Cow;
use thiserror::Error;

mod device;
mod duplex;
mod input;
mod output;
mod stream;
mod triggerfd;

/// Type of errors from using the ALSA backend.
#[derive(Debug, Error)]
#[error("ALSA error: ")]
pub enum AlsaError {
    /// Error originates from ALSA itself.
    #[error("{0}")]
    BackendError(#[from] alsa::Error),
    /// Error originates from I/O operations.
    #[error("I/O error: {0}")]
    IoError(#[from] nix::Error),
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
        Ok(HintIter::new(None, c"pcm")?
            .filter_map(|hint| AlsaDevice::new(hint.name.as_ref()?, hint.direction?).ok()))
    }
}

impl AudioDuplexDriver for AlsaDriver {
    type DuplexDevice = AlsaDuplexDevice;

    fn default_duplex_device(&self) -> Result<Option<Self::DuplexDevice>, Self::Error> {
        let Some(input) = self.default_device(DeviceType::Input)? else {
            return Ok(None);
        };
        let Some(output) = self.default_device(DeviceType::Output)? else {
            return Ok(None);
        };
        Ok(Some(AlsaDuplexDevice::new(input, output)))
    }

    fn list_duplex_devices(
        &self,
    ) -> Result<impl IntoIterator<Item = Self::DuplexDevice>, Self::Error> {
        Ok(HintIter::new(None, c"pcm")?
            .filter_map(|hint| AlsaDuplexDevice::full_duplex(hint.name.as_ref()?).ok()))
    }

    fn device_from_input_output(
        &self,
        input: Self::Device,
        output: Self::Device,
    ) -> Result<Self::DuplexDevice, Self::Error> {
        Ok(AlsaDuplexDevice::new(input, output))
    }
}
