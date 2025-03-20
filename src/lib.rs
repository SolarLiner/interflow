#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

pub mod audio_buffer;
pub mod backends;
pub mod channel_map;
pub mod device;
pub mod driver;
pub mod duplex;
pub mod prelude;
pub mod stream;
pub mod timestamp;

/// Marker trait for values which are [Send] everywhere but on the web (as WASM does not yet have
/// proper threads, and implementation of audio engines on WASM are either separate modules or a single module in a
/// push configuration).
///
/// This should only be used to define the traits and should not be relied upon in external code.
///
/// This definition is selected on non-web platforms, and does require [`Send`].
#[cfg(not(wasm))]
pub trait SendEverywhereButOnWeb: 'static + Send {}
#[cfg(not(wasm))]
impl<T: 'static + Send> SendEverywhereButOnWeb for T {}

/// Marker trait for values which are [Send] everywhere but on the web (as WASM does not yet have
/// proper threads, and implementation of audio engines on WASM are either separate modules or a single module in a
/// push configuration).
///
/// This should only be used to define the traits and should not be relied upon in external code.
///
/// This definition is selected on web platforms, and does not require [`Send`].
#[cfg(wasm)]
pub trait SendEverywhereButOnWeb {}
#[cfg(wasm)]
impl<T> SendEverywhereButOnWeb for T {}
