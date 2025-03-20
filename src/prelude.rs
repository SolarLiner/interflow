#![allow(unused)]
//! Prelude module for `interflow`. Use as a star-import.

#[cfg(os_asio)]
pub use crate::backends::asio::prelude::*;
#[cfg(os_wasapi)]
pub use crate::backends::wasapi::prelude::*;
pub use crate::backends::*;
pub use crate::*;

pub use device::{AudioDevice, AudioDuplexDevice, AudioInputDevice, AudioOutputDevice, DeviceType};
pub use driver::{AudioDriver, AudioDuplexDriver};
pub use duplex::{create_duplex_stream, AudioDuplexCallback};
pub use stream::{
    AudioCallbackContext, AudioInput, AudioInputCallback, AudioOutput, AudioOutputCallback,
    AudioStreamHandle, StreamConfig,
};
