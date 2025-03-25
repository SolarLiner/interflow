use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipewireError {
    #[error("Pipewire error: {0}")]
    BackendError(#[from] pipewire::Error),
    #[error("Cannot create Pipewire stream: {0}")]
    GenError(#[from] libspa::pod::serialize::GenError),
}
