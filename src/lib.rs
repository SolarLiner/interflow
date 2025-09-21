#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use std::borrow::Cow;
use std::fmt;
use std::fmt::Formatter;

use crate::audio_buffer::{AudioMut, AudioRef};
use crate::channel_map::ChannelMap32;
use crate::timestamp::Timestamp;
use crate::device::DeviceType;

pub mod audio_buffer;
pub mod backends;
pub mod channel_map;
pub mod device;
pub mod driver;
pub mod duplex;
pub mod prelude;
pub mod stream;
pub mod timestamp;


/// Audio drivers provide access to the inputs and outputs of devices.
/// Several drivers might provide the same accesses, some sharing it with other applications,
/// while others work in exclusive mode.
pub trait AudioDriver {
    /// Type of errors that can happen when using this audio driver.
    type Error: std::error::Error;
    /// Type of audio devices this driver provides.
    type Device: AudioDevice;

    /// Driver display name.
    const DISPLAY_NAME: &'static str;

    /// Runtime version of the audio driver. If there is a difference between "client" and
    /// "server" versions, then this should reflect the server version.
    fn version(&self) -> Result<Cow<str>, Self::Error>;

    /// Default device of the given type. This is most often tied to the audio settings at the
    /// operating system level.
    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error>;

    /// List all devices available through this audio driver.
    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error>;
}


/// Configuration for an audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// Configured sample rate of the requested stream. The opened stream can have a different
    /// sample rate, so don't rely on this parameter being correct at runtime.
    pub samplerate: f64,
    /// Map of channels requested by the stream. Entries correspond in order to
    /// [AudioDevice::channel_map].
    ///
    /// Some drivers allow specifying which channels are going to be opened and available through
    /// the audio buffers. For other drivers, only the number of requested channels is used, and
    /// order does not matter.
    pub channels: ChannelMap32,
    /// Range of preferential buffer sizes, in units of audio samples per channel.
    /// The library will make a best-effort attempt at honoring this setting, and in future versions
    /// may provide additional buffering to ensure it, but for now you should not make assumptions
    /// on buffer sizes based on this setting.
    pub buffer_size_range: (Option<usize>, Option<usize>),
    /// Whether the device should be exclusively held (meaning no other application can open the
    /// same device).
    pub exclusive: bool,
}

/// Audio channel description.
#[derive(Debug, Clone)]
pub struct Channel<'a> {
    /// Index of the channel in the device
    pub index: usize,
    /// Display name for the channel, if available, else a generic name like "Channel 1"
    pub name: Cow<'a, str>,
}

/// Trait for types describing audio devices. Audio devices have zero or more inputs and outputs,
/// and depending on the driver, can be duplex devices which can provide both of them at the same
/// time natively.
pub trait AudioDevice {
    /// Type of errors that can happen when using this device.
    type Error: std::error::Error;

    /// Device display name
    fn name(&self) -> Cow<str>;

    /// Device type. Either input, output, or duplex.
    fn device_type(&self) -> DeviceType;

    /// Iterator of the available channels in this device. Channel indices are used when
    /// specifying which channels to open when creating an audio stream.
    fn channel_map(&self) -> impl IntoIterator<Item = Channel>;

    /// Not all configuration values make sense for a particular device, and this method tests a
    /// configuration to see if it can be used in an audio stream.
    fn is_config_supported(&self, config: &StreamConfig) -> bool;

    /// Enumerate all possible configurations this device supports. If that is not provided by
    /// the device, and not easily generated manually, this will return `None`.
    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>>;

    /// Returns the supported I/O buffer size range for the device.
    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), Self::Error> {
        Ok((None, None))
    }
}

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
