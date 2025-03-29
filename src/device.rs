use crate::duplex::AudioDuplexCallback;
use crate::stream::{AudioInputCallback, AudioOutputCallback, AudioStreamHandle, StreamConfig};
use crate::SendEverywhereButOnWeb;
use std::borrow::Cow;

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

    /// Not all configuration values make sense for a particular device, and this method tests a
    /// configuration to see if it can be used in an audio stream.
    fn is_config_supported(&self, config: &StreamConfig) -> bool;

    /// Enumerate all possible configurations this device supports. If that is not provided by
    /// the device, and not easily generated manually, this will return `None`.
    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>>;
}

/// Trait for types which can provide input streams.
///
/// Input devices require a [`AudioInputCallback`] which receives the audio data from the input
/// device, and processes it.
pub trait AudioInputDevice: AudioDevice {
    /// Map of input channels. This can be used to get the index of channels to open when creating a stream.
    fn input_channel_map(&self) -> impl Iterator<Item = Channel>;

    /// Type of the resulting stream. This stream can be used to control the audio processing
    /// externally, or stop it completely and give back ownership of the callback with
    /// [`AudioStreamHandle::eject`].
    type StreamHandle<Callback: AudioInputCallback>: AudioStreamHandle<Callback>;

    /// Return the default configuration for an input stream.
    fn default_input_config(&self) -> Result<StreamConfig, Self::Error>;

    /// Creates an input stream with the provided stream configuration. For this call to be
    /// valid, [`AudioDevice::is_config_supported`] should have returned `true` on the provided
    /// configuration.
    ///
    /// An input callback is required to process the audio, whose ownership will be transferred
    /// to the audio stream.
    fn create_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error>;

    /// Creates an input stream from the default configuration given by [`Self::default_input_configuration`].
    fn default_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        self.create_input_stream(self.default_input_config()?, callback)
    }
}

/// Trait for types which can provide output streams.
///
/// Output devices require a [`AudioOutputCallback`] which receives the audio data from the output
/// device, and processes it.
pub trait AudioOutputDevice: AudioDevice {
    /// Map of output channels. This can be used to get the index of channels to open when creating a stream.
    fn output_channel_map(&self) -> impl Iterator<Item = Channel>;

    /// Type of the resulting stream. This stream can be used to control the audio processing
    /// externally, or stop it completely and give back ownership of the callback with
    /// [`AudioStreamHandle::eject`].
    type StreamHandle<Callback: AudioOutputCallback>: AudioStreamHandle<Callback>;

    /// Return the default configuration for an output stream.
    fn default_output_config(&self) -> Result<StreamConfig, Self::Error>;

    /// Creates an output stream with the provided stream configuration. For this call to be
    /// valid, [`AudioDevice::is_config_supported`] should have returned `true` on the provided
    /// configuration.
    ///
    /// An output callback is required to process the audio, whose ownership will be transferred
    /// to the audio stream.
    fn create_output_stream<Callback: SendEverywhereButOnWeb + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error>;

    /// Creates an output stream from the default configuration given by [`Self::default_output_configuration`].
    fn default_output_stream<Callback: SendEverywhereButOnWeb + AudioOutputCallback>(
        &self,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        self.create_output_stream(self.default_output_config()?, callback)
    }
}

/// Trait for types which can provide duplex streams.
///
/// Output devices require a [`AudioDuplexCallback`] which receives the audio data from the device, and processes it.
pub trait AudioDuplexDevice: AudioDevice {
    /// Type of the resulting stream. This stream can be used to control the audio processing
    /// externally, or stop it completely and give back ownership of the callback with
    /// [`AudioStreamHandle::eject`].
    type StreamHandle<Callback: AudioDuplexCallback>: AudioStreamHandle<Callback>;

    /// Return the default configuration for a duplex stream.
    fn default_duplex_config(&self) -> Result<StreamConfig, Self::Error>;

    /// Creates a duplex stream with the provided stream configuration. For this call to be
    /// valid, [`AudioDevice::is_config_supported`] should have returned `true` on the provided
    /// configuration.
    ///
    /// A duplex callback is required to process the audio, whose ownership will be transferred
    /// to the audio stream.
    fn create_duplex_stream<Callback: SendEverywhereButOnWeb + AudioDuplexCallback>(
        &self,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<<Self as AudioDuplexDevice>::StreamHandle<Callback>, Self::Error>;

    /// Creates a duplex stream from the default configuration given by [`Self::default_duplex_configuration`].
    fn default_duplex_stream<Callback: SendEverywhereButOnWeb + AudioDuplexCallback>(
        &self,
        callback: Callback,
    ) -> Result<<Self as AudioDuplexDevice>::StreamHandle<Callback>, Self::Error> {
        self.create_duplex_stream(self.default_duplex_config()?, callback)
    }
}

/// Devices are either inputs or outputs. Duplex devices are not covered by this type,
/// as they are covered by an entirely different trait: [`AudioDuplexDevice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    /// Device only supports inputs.
    Input,
    /// Device only supports outputs.
    Output,
}

/// Audio channel description.
#[derive(Debug, Clone)]
pub struct Channel<'a> {
    /// Index of the channel in the device
    pub index: usize,
    /// Display name for the channel, if available, else a generic name like "Channel 1"
    pub name: Cow<'a, str>,
}
