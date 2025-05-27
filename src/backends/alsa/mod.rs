//! # ALSA backend
//!
//! ALSA is a generally available driver for Linux and BSD systems. It is the oldest of the Linux
//! drivers supported in this library, and as such makes it a good fallback driver. Newer drivers
//! (PulseAudio, PipeWire) offer ALSA-compatible APIs so that older software can still access the
//! audio devices through them.

use crate::{AudioDriver, DeviceType};
use alsa::device_name::HintIter;
use device::AlsaDevice;
use std::borrow::Cow;
use thiserror::Error;

mod device;
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

    fn version(&self) -> Result<Cow<'_, str>, Self::Error> {
        Ok(Cow::Borrowed("unknown"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        let direction = match device_type {
            _ if device_type.is_input() => alsa::Direction::Capture,
            _ if device_type.is_output() => alsa::Direction::Playback,
            _ => return Ok(None),
        };
        Ok(Some(AlsaDevice::default_device(direction)?))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        Ok(HintIter::new(None, c"pcm")?
            .filter_map(|hint| AlsaDevice::new(hint.name.as_ref()?, hint.direction?).ok()))
    }
}
