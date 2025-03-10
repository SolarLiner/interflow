//! Windows Audio Session API (WASAPI) backend for interflow.

mod util;

mod error;

pub(crate) mod driver;
mod device;
mod stream;
pub mod prelude;

pub use prelude::*;