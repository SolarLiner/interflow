use std::borrow::Cow;

pub mod audio_buffer;
pub mod backends;
pub mod channel_map;

/// Audio drivers provide access to the inputs and outputs of physical devices.
/// Several drivers might provide the same accesses, some sharing it with other applications,
/// while others work in exclusive mode.
pub trait AudioDriver {
    type Error: std::error::Error;
    type Device: AudioDevice;

    const DISPLAY_NAME: &'static str;

    fn version(&self) -> Result<Cow<str>, Self::Error>;

    fn default_device(&self) -> Result<Self::Device, Self::Error>;

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
    pub channels: ChannelMap,
    pub buffer_size_min: Option<usize>,
    pub buffer_size_max: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Channel<'a> {
    pub index: usize,
    pub name: Cow<'a, str>,
}

pub trait AudioDevice {
    type Error: std::error::Error;
    type Stream<Callback>: AudioStream<Callback, Error=Self::Error>;

    fn name(&self) -> Cow<str>;

    fn device_type(&self) -> DeviceType;

    fn channel_map(&self) -> impl IntoIterator<Item=Channel>;

    fn is_config_supported(&self, config: &StreamConfig) -> bool;

    fn enumerate_configurations(&self) -> impl IntoIterator<Item = StreamConfig>;

    fn create_stream<Callback>(
        &self,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::Stream<Callback>, Self::Error>;
}

pub trait AudioStream<Callback>: Sized {
    type Error: std::error::Error;

    fn start(&self) -> Result<(), Self::Error>;

    fn stop(&self) -> Result<(), Self::Error>;

    fn eject(self) -> Callback;
}
