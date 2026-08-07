//! # Interflow CoreAudio
//!
//! Interflow backend using CoreAudio for macOS and iOS applications
#![cfg(any(target_os = "macos", target_os = "ios"))]
#![warn(missing_docs)]
use coreaudio::audio_unit;
use interflow_core::DeviceType;
use std::convert::Infallible;

pub mod device;
pub mod platform;
pub mod stream;
mod utils;

/// Prelude module. Import all with `interflow_coreaudio::prelude::*`.
pub mod prelude {
    pub use crate::device::{Device, DeviceRequest};
    pub use crate::platform::Platform;
    pub use crate::stream::Handle;
}

/// Type of errors from the CoreAudio backend
#[derive(Debug, thiserror::Error)]
#[error("CoreAudio error: ")]
pub enum Error {
    /// Error originating from CoreAudio
    #[error(transparent)]
    Backend(#[from] coreaudio::Error),
    /// The scope given to an audio device is invalid.
    #[error("Invalid scope {0:?}")]
    InvalidScope(audio_unit::Scope),
    /// No matching devices for the given type.
    #[error("No matching devices for type: {0:?}")]
    NoMatchingDevices(DeviceType),
    /// User has requested a duplex (both input and output) stream, which is not supported by CoreAudio.
    #[error("Duplex devices are not supported")]
    DuplexUnavailable,
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}
