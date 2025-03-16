use crate::audio_buffer::{AudioMut, AudioRef};
use crate::channel_map::ChannelMap32;
use crate::timestamp::Timestamp;

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
    /// Range of preferential buffer sizes. The library will make a bast-effort attempt at
    /// honoring this setting, and in future versions may provide additional buffering to ensure
    /// it, but for now you should not make assumptions on buffer sizes based on this setting.
    pub buffer_size_range: (Option<usize>, Option<usize>),
    /// Whether the device should be exclusively held (meaning no other application can open the
    /// same device).
    pub exclusive: bool,
}

#[duplicate::duplicate_item(
    name            bufty;
    [AudioInput]    [AudioRef < 'a, T >];
    [AudioOutput]   [AudioMut < 'a, T >];
)]
/// Plain-old-data object holding references to the audio buffer and the associated time-keeping
/// [`Timestamp`]. This timestamp is associated with the stream, and in the cases where the
/// driver provides timing information, it is used instead of relying on sample-counting.
pub struct name<'a, T> {
    /// Associated time stamp for this callback. The time represents the duration for which the
    /// stream has been opened, and is either provided by the driver if available, or is kept up
    /// manually by the library.
    pub timestamp: Timestamp,
    /// Audio buffer data.
    pub buffer: bufty,
}

/// Trait for types which handles an audio stream (input or output).
pub trait AudioStreamHandle<Callback> {
    /// Type of errors which have caused the stream to fail.
    type Error: std::error::Error;

    /// Eject the stream, returning ownership of the callback.
    ///
    /// An error can occur when an irrecoverable error has occurred and ownership has been lost
    /// already.
    fn eject(self) -> Result<Callback, Self::Error>;
}

/// Plain-old-data object holding the passed-in stream configuration, as well as a general
/// callback timestamp, which can be different from the input and output streams in case of
/// cross-stream latencies; differences in timing can indicate desync.
pub struct AudioCallbackContext {
    /// Passed-in stream configuration. Values have been updated where necessary to correspond to
    /// the actual stream properties.
    pub stream_config: StreamConfig,
    /// Callback-wide timestamp.
    pub timestamp: Timestamp,
}

/// Trait of types which process input audio data. This is the trait that users will want to
/// implement when processing an input device.
pub trait AudioInputCallback {
    /// Callback called when input data is available to be processed.
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>);
}

/// Trait of types which process output audio data. This is the trait that users will want to
/// implement when processing an output device.
pub trait AudioOutputCallback {
    /// Callback called when output data is available to be processed.
    fn on_output_data(&mut self, context: AudioCallbackContext, input: AudioOutput<f32>);
}
