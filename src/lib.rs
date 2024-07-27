#![warn(missing_docs)]

use std::borrow::Cow;

use crate::audio_buffer::{AudioMut, AudioRef};
use crate::channel_map::ChannelMap32;
use crate::timestamp::Timestamp;

pub mod audio_buffer;
pub mod backends;
pub mod channel_map;
pub mod prelude;
pub mod timestamp;

/// Audio drivers provide access to the inputs and outputs of physical devices.
/// Several drivers might provide the same accesses, some sharing it with other applications,
/// while others work in exclusive mode.
pub trait AudioDriver {
    type Error: std::error::Error;
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

/// Devices are either inputs, outputs, or provide both at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Input,
    Output,
    Duplex,
}

/// Configuration for an audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// Configured sample rate of the requested stream. The opened stream can have a different
    /// sample rate, so don't rely on this parameter being correct at runtime.
    pub samplerate: f64,
    /// Map of channels requested by the stream. Entries correspond in order to
    /// [AudioDevice::channel_map].
    pub channels: ChannelMap32,
    /// Range of preferential buffer sizes. The library will make a bast-effort attempt at
    /// honoring this setting, and in future versions may provide additional buffering to ensure
    /// it, but for now you should not make assumptions on buffer sizes based on this setting.
    pub buffer_size_range: (Option<usize>, Option<usize>),
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
}

/// Marker trait for values which are [Send] everywhere but on the web (as WASM does not yet have
/// web targets.
///
/// This should only be used to define the traits and should not be relied upon in external code.
#[cfg(not(wasm))]
pub trait SendEverywhereButOnWeb: 'static + Send {}
#[cfg(not(wasm))]
impl<T: 'static + Send> SendEverywhereButOnWeb for T {}

#[cfg(wasm)]
pub trait SendEverywhereButOnWeb {}
#[cfg(wasm)]
impl<T> SendEverywhereButOnWeb for T {}

pub trait AudioInputDevice: AudioDevice {
    type StreamHandle<Callback: AudioInputCallback>: AudioStreamHandle<Callback>;

    fn create_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error>;
}

pub trait AudioOutputDevice: AudioDevice {
    type StreamHandle<Callback: AudioOutputCallback>: AudioStreamHandle<Callback>;

    fn create_output_stream<Callback: SendEverywhereButOnWeb + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error>;
}

pub trait AudioStreamHandle<Callback> {
    type Error: std::error::Error;

    fn eject(self) -> Result<Callback, Self::Error>;
}

#[duplicate::duplicate_item(
    name            bufty;
    [AudioInput]    [AudioRef < 'a, T >];
    [AudioOutput]   [AudioMut < 'a, T >];
)]
pub struct name<'a, T> {
    pub timestamp: Timestamp,
    pub buffer: bufty,
}

pub struct AudioCallbackContext {
    pub stream_config: StreamConfig,
    pub timestamp: Timestamp,
}

pub trait AudioInputCallback {
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>);
}

pub trait AudioOutputCallback {
    fn on_output_data(&mut self, context: AudioCallbackContext, input: AudioOutput<f32>);
}
