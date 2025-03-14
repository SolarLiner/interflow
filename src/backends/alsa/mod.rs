//! # ALSA backend
//!
//! ALSA is a generally available driver for Linux and BSD systems. It is the oldest of the Linux
//! drivers supported in this library, and as such makes it a good fallback driver. Newer drivers
//! (PulseAudio, PipeWire) offer ALSA-compatible APIs so that older software can still access the
//! audio devices through them.

use std::borrow::Cow;
use alsa::device_name::HintIter;
use thiserror::Error;
use device::AlsaDevice;
use crate::channel_map::Bitset;
use crate::{
    AudioDevice, AudioDriver, AudioInputCallback,
    AudioInputDevice, AudioOutputCallback, AudioOutputDevice, AudioStreamHandle
    , DeviceType,
};

mod output;
mod input;
mod device;
mod stream;

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
        Ok(HintIter::new(None, c"pcm")?
            .filter_map(|hint| AlsaDevice::new(hint.name.as_ref()?, hint.direction?).ok()))
    }
}

