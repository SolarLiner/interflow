use std::borrow::Cow;
use crate::DeviceType;
use crate::stream::{self, StreamHandle};
use crate::traits::ExtensionProvider;

/// Configuration for an audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// Configured sample rate of the requested stream. The opened stream can have a different
    /// sample rate, so don't rely on this parameter being correct at runtime.
    pub sample_rate: f64,
    /// Number of input channels requested
    pub input_channels: usize,
    /// Number of output channels requested
    pub output_channels: usize,
    /// Range of preferential buffer sizes, in units of audio samples per channel.
    /// The library will make a best-effort attempt at honoring this setting, and in future versions
    /// may provide additional buffering to ensure it, but for now you should not make assumptions
    /// on buffer sizes based on this setting.
    pub buffer_size_range: (Option<usize>, Option<usize>),
    /// Whether the device should be exclusively held (meaning no other application can open the
    /// same device).
    pub exclusive: bool,
}

impl StreamConfig {
    /// Returns a [`DeviceType`] that describes this [`StreamConfig`]. Only [`DeviceType::INPUT`] and
    /// [`DeviceType::OUTPUT`] are set.
    pub fn requested_device_type(&self) -> DeviceType {
        let mut ret = DeviceType::empty();
        ret.set(DeviceType::INPUT, self.input_channels > 0);
        ret.set(DeviceType::OUTPUT, self.output_channels > 0);
        ret
    }

    /// Changes the [`StreamConfig`] such that it matches the configuration of a stream created with a device with
    /// the given [`DeviceType`].
    ///
    /// This method returns a copy of the input [`StreamConfig`].
    pub fn restrict(mut self, requested_type: DeviceType) -> Self {
        if !requested_type.is_input() {
            self.input_channels = 0;
        }
        if !requested_type.is_output() {
            self.output_channels = 0;
        }
        self
    }
}

/// Configuration for an audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedStreamConfig {
    /// Configured sample rate of the requested stream. The opened stream can have a different
    /// sample rate, so don't rely on this parameter being correct at runtime.
    pub sample_rate: f64,
    /// Number of input channels requested
    pub input_channels: usize,
    /// Number of output channels requested
    pub output_channels: usize,
    /// Maximum number of frames the audio callback will receive
    pub max_frame_count: usize,
}

/// Trait for types describing audio devices. Audio devices have zero or more inputs and outputs,
/// and depending on the driver, can be duplex devices which can provide both of them at the same
/// time natively.
pub trait Device: ExtensionProvider {
    type Error: Send + Sync + std::error::Error;
    type StreamHandle<Callback: stream::Callback>: StreamHandle<Callback, Error: Into<Self::Error>>;
    
    fn name(&self) -> Cow<'_, str>;
    
    fn device_type(&self) -> DeviceType;

    /// Default configuration for this device. If [`Ok`], should return a [`StreamConfig`] that is supported (i.e.,
    /// returns `true` when passed to [`Self::is_config_supported`]).
    fn default_config(&self) -> Result<StreamConfig, Self::Error>;

    /// Returns the supported I/O buffer size range for the device.
    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), Self::Error> {
        Ok((None, None))
    }

    /// Not all configuration values make sense for a particular device, and this method tests a
    /// configuration to see if it can be used in an audio stream.
    fn is_config_supported(&self, config: &StreamConfig) -> bool;

    /// Creates an output stream with the provided stream configuration. For this call to be
    /// valid, [`AudioDevice::is_config_supported`] should have returned `true` on the provided
    /// configuration.
    ///
    /// An output callback is required to process the audio, whose ownership will be transferred
    /// to the audio stream.
    fn create_stream<Callback: 'static + Send + stream::Callback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error>;

    /// Create an output stream using the default configuration as returned by [`Self::default_output_config`].
    ///
    /// # Arguments
    ///
    /// - `callback`: Output callback to generate audio data with.
    fn default_stream<Callback: 'static + Send + stream::Callback>(
        &self,
        requested_type: DeviceType,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let config = self.default_config()?.restrict(requested_type);
        debug_assert!(
            self.is_config_supported(&config),
            "Default configuration is not supported"
        );
        self.create_stream(config, callback)
    }
}

/// Audio channel description.
#[derive(Debug, Clone)]
pub struct Channel<'a> {
    /// Index of the channel in the device
    pub index: usize,
    /// Display the name for the channel, if available, else a generic name like "Channel 1"
    pub name: Cow<'a, str>,
}

pub trait NamedChannels {
    fn channel_map(&self) -> impl Iterator<Item = Channel<'_>>;
}

pub trait ConfigurationList {
    fn enumerate_configurations(&self) -> impl Iterator<Item = StreamConfig>;
}
