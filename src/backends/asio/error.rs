use thiserror::Error;
use asio_sys::AsioError as AsioSysError;

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
}
