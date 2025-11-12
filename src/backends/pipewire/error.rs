//! PipeWire errors.

use thiserror::Error;

/// PipeWire error.
#[derive(Debug, Error)]
pub enum PipewireError {
    /// Error originating in the PipeWire backend.
    #[error("Pipewire error: {0}")]
    BackendError(#[from] pipewire::Error),
    /// Error creating a pipewire stream (SPA pod serialization problem).
    #[error("Cannot create Pipewire stream: {0}")]
    GenError(#[from] libspa::pod::serialize::GenError),
}
