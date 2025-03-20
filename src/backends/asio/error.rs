use asio_sys::AsioError as AsioSysError;
use thiserror::Error;

/// Type of errors from the ASIO backend.
#[derive(Debug, Error)]
#[error("ASIO error: ")]
pub enum AsioError {
    /// Error originating from ASIO.
    #[error("{0}")]
    BackendError(#[from] AsioSysError),
    /// Requested WASAPI device configuration is not available
    #[error("Configuration not available")]
    ConfigurationNotAvailable,
    #[error("Device unavailable")]
    DeviceUnavailable,
    #[error("Multiple streams not supported")]
    MultipleStreams,
}
