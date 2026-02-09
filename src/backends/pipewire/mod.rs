//! # PipeWire backend
//!
//! PipeWire is a modern multimedia server for Linux systems, designed to handle both audio and
//! video streams. It provides low-latency performance and advanced routing capabilities while
//! maintaining compatibility with older APIs like ALSA and PulseAudio, making it a flexible and
//! future-oriented backend.

pub mod device;
pub mod driver;
pub mod error;
pub mod stream;
mod utils;
