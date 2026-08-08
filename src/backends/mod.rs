//! # Backends
//!
//! Home of the various backends supported by the library.
//!
//! Each backend is provided in its own submodule. Types should be public so that the user isn't
//! limited to going through the main API if they want to choose a specific backend.

pub mod null;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use interflow_coreaudio as coreaudio;

#[cfg(target_os = "windows")]
pub use interflow_wasapi as wasapi;
