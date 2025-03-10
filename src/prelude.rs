//! Prelude module for `interflow`. Use as a star-import.

pub use crate::backends::*;
#[cfg(os_wasapi)]
pub use crate::backends::wasapi::prelude::*;
pub use crate::backends::*;
pub use crate::*;
pub use crate::duplex::create_duplex_stream;
