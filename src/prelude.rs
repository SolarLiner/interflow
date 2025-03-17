#![allow(unused)]
//! Prelude module for `interflow`. Use as a star-import.

#[cfg(os_wasapi)]
pub use crate::backends::wasapi::prelude::*;
pub use crate::backends::*;
pub use crate::duplex::{
    create_duplex_stream, AudioDuplexCallback, DuplexStreamConfig, DuplexStreamHandle,
};
pub use crate::*;
