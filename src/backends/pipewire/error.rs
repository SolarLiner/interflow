use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipewireError {
    #[error(transparent)]
    BackendError(#[from] pipewire::Error),
}
