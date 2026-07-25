pub mod device;
pub mod platform;
pub mod stream;
mod util;

pub mod prelude {
    pub use crate::device::Device;
    pub use crate::platform::Platform;
    pub use crate::stream::Handle;
}

use std::convert::Infallible;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{} (code {})", .0.message(), .0.code())]
    BackendError(#[from] windows::core::Error),
    #[error("Configuration not available")]
    ConfigurationNotAvailable,
    #[error("Win32 error: {0}")]
    FoundationError(String),
    #[error("Unsupported duplex stream requested")]
    DuplexStreamRequested,
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}
