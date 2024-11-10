use thiserror::Error;

/// Type of errors from the WASAPI backend.
#[derive(Debug, Error)]
#[error("WASAPI error: ")]
pub enum WasapiError {
    /// Error originating from WASAPI.
    #[error("{} (code {})", .0.message(), .0.code())]
    BackendError(#[from] windows::core::Error),
    /// Requested WASAPI device configuration is not available
    #[error("Configuration not available")]
    ConfigurationNotAvailable,
    /// Windows Foundation error
    #[error("Win32 error: {0}")]
    FoundationError(String),
}