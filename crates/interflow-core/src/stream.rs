use bitflags::bitflags;
use crate::buffer::{AudioMut, AudioRef};
use crate::device::ResolvedStreamConfig;
use crate::timing::Timestamp;
use crate::traits::ExtensionProvider;

pub trait StreamProxy: Send + Sync + ExtensionProvider {}

bitflags! {
    pub struct ChannelFlags: u32 {
        const INACTIVE = 0x0000_0001;
        const UNDERFLOW = 0x0000_0002;
        const OVERFLOW = 0x0000_0004;
    }
}

pub trait StreamLatency {
    fn input_latency(&self, channel: usize) -> Option<usize>;
    fn output_latency(&self, channel: usize) -> Option<usize>;
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
    pub channel_flags: &'a [ChannelFlags],
}

/// Plain-old-data object holding the passed-in stream configuration, as well as a general
/// callback timestamp, which can be different from the input and output streams in case of
/// cross-stream latencies; differences in timing can indicate desync.
pub struct CallbackContext<'a> {
    /// Passed-in stream configuration. Values have been updated where necessary to correspond to
    /// the actual stream properties.
    pub stream_config: &'a ResolvedStreamConfig,
    /// Callback-wide timestamp.
    pub timestamp: Timestamp,
    pub stream_proxy: &'a dyn StreamProxy,
}

/// Trait for types which handles an audio stream (input or output).
pub trait StreamHandle<Callback> {
    /// Type of errors which have caused the stream to fail.
    type Error: Send + std::error::Error;

    /// Eject the stream, returning ownership of the callback.
    ///
    /// An error can occur when an irrecoverable error has occured and ownership has been lost
    /// already.
    fn eject(self) -> Result<Callback, Self::Error>;
}

/// Trait of types which process audio data. This is the trait that users will want to
/// implement when processing audio from a device.
pub trait Callback: Send {
    /// Prepare the audio callback to process audio. This function is *not* real-time safe (i.e., allocations can be
    /// performed), in preparation for processing the stream with [`Self::process_audio`].
    fn prepare(&mut self, context: CallbackContext);

    /// Callback called when audio data can be processed.
    fn process_audio(
        &mut self,
        context: CallbackContext,
        input: AudioInput<f32>,
        output: AudioOutput<f32>,
    );
}
