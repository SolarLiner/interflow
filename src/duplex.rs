use crate::audio_buffer::AudioBuffer;
use crate::channel_map::Bitset;
use crate::{
    AudioCallbackContext, AudioDevice, AudioInput, AudioInputCallback, AudioInputDevice,
    AudioOutput, AudioOutputCallback, AudioOutputDevice, AudioStreamHandle, SendEverywhereButOnWeb,
    StreamConfig,
};
use ndarray::{ArrayView1, ArrayViewMut1};
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// Trait of types that can process both input and output audio streams at the same time.
pub trait AudioDuplexCallback: 'static + SendEverywhereButOnWeb {
    /// Processes audio data in a duplex stream.
    ///
    /// # Arguments
    /// * `context` - The context containing stream configuration and timing information
    /// * `input` - The input audio buffer containing captured audio data
    /// * `output` - The output audio buffer to be filled with processed audio data
    fn on_audio_data(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        output: AudioOutput<f32>,
    );
}

/// Type which handles both a duplex stream handle.
pub struct DuplexStream<Callback, Error> {
    input_stream: Box<dyn AudioStreamHandle<InputProxy, Error = Error>>,
    output_stream: Box<dyn AudioStreamHandle<DuplexCallback<Callback>, Error = Error>>,
}

/// Input proxy for transferring an input signal to a separate output callback to be processed as a duplex stream.
pub struct InputProxy {
    buffer: rtrb::Producer<f32>,
    output_sample_rate: Arc<AtomicU64>,
}

impl AudioInputCallback for InputProxy {
    /// Processes incoming audio data and stores it in the internal buffer.
    ///
    /// Handles sample rate conversion between input and output streams.
    ///
    /// # Arguments
    /// * `context` - The context containing stream configuration and timing information
    /// * `input` - The input audio buffer containing captured audio data
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>) {
        log::trace!(num_samples = input.buffer.num_samples(), num_channels = input.buffer.num_channels();
            "on_input_data");
        let input_slots = input.buffer.num_samples() * input.buffer.num_channels();
        if self.buffer.slots() < input_slots {
            log::error!(buffer_slots=self.buffer.slots(), input_slots; "Input proxy buffer underrun");
            return;
        }
        let mut scratch = [0f32; 32];
        let rate = self.output_sample_rate.load(Ordering::SeqCst) as f64
            / context.stream_config.samplerate;
        let out_len = (input.buffer.num_samples() as f64 * rate) as usize;
        let mut scratch =
            ArrayViewMut1::from(&mut scratch[..context.stream_config.channels.count()]);
        let rate_recip = rate.recip();
        for i in 0..out_len {
            let in_ix = i as f64 / rate_recip;
            let i = in_ix.floor() as usize;
            let j = i + 1;
            if j == out_len {
                scratch.assign(&input.buffer.get_frame(i));
            } else {
                lerp(
                    in_ix.fract() as _,
                    input.buffer.get_frame(i),
                    input.buffer.get_frame(j),
                    scratch.view_mut(),
                );
            }
            for sample in scratch.iter().copied() {
                let _ = self.buffer.push(sample);
            }
        }
    }
}

/// Performs linear interpolation between two arrays of samples.
///
/// # Arguments
/// * `x` - Interpolation factor between 0 and 1
/// * `a` - First array of samples
/// * `b` - Second array of samples
/// * `out` - Output array for interpolated results
fn lerp(x: f32, a: ArrayView1<f32>, b: ArrayView1<f32>, mut out: ArrayViewMut1<f32>) {
    assert_eq!(out.len(), a.len());
    assert_eq!(out.len(), b.len());
    for i in 0..out.len() {
        out[i] = lerpf(x, a[i], b[i]);
    }
}

/// Performs linear interpolation between two float values.
///
/// # Arguments
/// * `x` - Interpolation factor between 0 and 1
/// * `a` - First value
/// * `b` - Second value
///
/// # Returns
/// The interpolated value
fn lerpf(x: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * x
}

#[derive(Debug, Error)]
#[error(transparent)]
/// Represents errors that can occur during duplex stream operations.
pub enum DuplexCallbackError<InputError, OutputError> {
    /// An error occurred in the input stream
    InputError(InputError),
    /// An error occurred in the output stream
    OutputError(OutputError),
    /// An error that doesn't fit into other categories
    Other(Box<dyn Error>),
}

pub struct DuplexCallback<Callback> {
    input: rtrb::Consumer<f32>,
    callback: Callback,
    storage: AudioBuffer<f32>,
    output_sample_rate: Arc<AtomicU64>,
}

impl<Callback> DuplexCallback<Callback> {
    /// Consumes the DuplexCallback and returns the underlying callback implementation.
    ///
    /// # Returns
    /// The wrapped callback instance or an error if extraction fails
    pub fn into_inner(self) -> Result<Callback, Box<dyn Error>> {
        Ok(self.callback)
    }
}

impl<Callback: AudioDuplexCallback> AudioOutputCallback for DuplexCallback<Callback> {
    fn on_output_data(&mut self, context: AudioCallbackContext, output: AudioOutput<f32>) {
        self.output_sample_rate
            .store(context.stream_config.samplerate as _, Ordering::SeqCst);
        let num_channels = self.storage.num_channels();
        let num_samples = output.buffer.num_samples().min(self.storage.num_samples());
        for i in 0..num_samples {
            let mut frame = self.storage.get_frame_mut(i);
            for ch in 0..num_channels {
                frame[ch] = self.input.pop().unwrap_or(0.0);
            }
        }
        let input = AudioInput {
            timestamp: context.timestamp,
            buffer: self.storage.slice(..num_samples),
        };
        self.callback.on_audio_data(context, input, output);
    }
}

/// A handle for managing a duplex audio stream that combines input and output capabilities.
///
/// This struct provides a way to control and manage a duplex audio stream that processes both
/// input and output audio data simultaneously. It wraps the individual input and output stream
/// handles and provides unified control over the duplex operation.
///
/// # Type Parameters
///
/// * `InputHandle` - The type of the input stream handle, must implement `AudioStreamHandle<InputProxy>`
/// * `OutputHandle` - The type of the output stream handle, must implement `AudioStreamHandle<DuplexCallback<Callback>>`
///
/// # Example
///
/// ```no_run
/// use interflow::duplex::AudioDuplexCallback;
/// use interflow::prelude::*;
///
/// let input_device = default_input_device();
/// let output_device = default_output_device();
/// let input_config = input_device.default_input_config().unwrap();
/// let output_config = output_device.default_output_config().unwrap();
///
/// struct MyCallback;
///
/// impl MyCallback {
///     fn new() -> Self { Self }
/// }
///
/// impl AudioDuplexCallback for MyCallback {
///     fn on_audio_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>, output: AudioOutput<f32>) {
///         // Implementation left as an exercise to the reader
///     }
/// }
///
/// // Create and use a duplex stream
/// let stream_handle = create_duplex_stream(
///     input_device,
///     input_config,
///     output_device,
///     output_config,
///     MyCallback::new()
/// ).expect("Failed to create duplex stream");
///
/// // Later, stop the stream and retrieve the callback
/// let callback = stream_handle.eject().expect("Failed to stop stream");
/// ```
#[derive(Debug)]
pub struct DuplexStreamHandle<InputHandle, OutputHandle> {
    input_handle: InputHandle,
    output_handle: OutputHandle,
}

impl<
        Callback,
        InputHandle: AudioStreamHandle<InputProxy>,
        OutputHandle: AudioStreamHandle<DuplexCallback<Callback>>,
    > AudioStreamHandle<Callback> for DuplexStreamHandle<InputHandle, OutputHandle>
{
    type Error = DuplexCallbackError<InputHandle::Error, OutputHandle::Error>;

    /// Stops the duplex stream and retrieves the callback instance.
    ///
    /// # Returns
    ///
    /// The callback instance if successful, or an error if the stream cannot be stopped properly
    fn eject(self) -> Result<Callback, Self::Error> {
        self.input_handle
            .eject()
            .map_err(DuplexCallbackError::InputError)?;
        let duplex_callback = self
            .output_handle
            .eject()
            .map_err(DuplexCallbackError::OutputError)?;
        duplex_callback
            .into_inner()
            .map_err(DuplexCallbackError::Other)
    }
}

/// Type alias of the result of creating a duplex stream.
pub type DuplexStreamResult<In, Out, Callback> = Result<
    DuplexStreamHandle<
        <In as AudioInputDevice>::StreamHandle<InputProxy>,
        <Out as AudioOutputDevice>::StreamHandle<DuplexCallback<Callback>>,
    >,
    DuplexCallbackError<<In as AudioDevice>::Error, <Out as AudioDevice>::Error>,
>;

/// Creates a duplex audio stream that handles both input and output simultaneously.
///
/// This function sets up a full-duplex audio stream by creating separate input and output streams
/// and connecting them through a ring buffer. The input stream captures audio data and stores it
/// in the buffer, while the output stream retrieves and processes this data before playback.
///
/// # Arguments
///
/// * `input_device` - The audio input device to capture audio from
/// * `input_config` - Configuration parameters for the input stream
/// * `output_device` - The audio output device to play audio through
/// * `output_config` - Configuration parameters for the output stream
/// * `callback` - The callback implementation that processes audio data
///
/// # Returns
///
/// A Result containing either:
/// * A `DuplexStreamHandle` that can be used to manage the duplex stream
/// * A `DuplexCallbackError` if stream creation fails
///
/// # Example
///
/// ```no_run
/// use interflow::duplex::AudioDuplexCallback;
/// use interflow::prelude::*;
///
/// struct MyCallback;
///
/// impl MyCallback {
///     pub fn new() -> Self {
///         Self
///     }
/// }
///
/// impl AudioDuplexCallback for MyCallback {
///     fn on_audio_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>, output: AudioOutput<f32>) {
///         // Implementation left as exercise to the reader
///     }
/// }
///
/// let input_device = default_input_device();
/// let output_device = default_output_device();
/// let input_config = input_device.default_input_config().unwrap();
/// let output_config = output_device.default_output_config().unwrap();
///
/// let callback = MyCallback::new();
///
/// let duplex_stream = create_duplex_stream(
///     input_device,
///     input_config,
///     output_device,
///     output_config,
///     callback
/// ).expect("Failed to create duplex stream");
///
/// ```
pub fn create_duplex_stream<
    InputDevice: AudioInputDevice,
    OutputDevice: AudioOutputDevice,
    Callback: AudioDuplexCallback,
>(
    input_device: InputDevice,
    input_config: StreamConfig,
    output_device: OutputDevice,
    output_config: StreamConfig,
    callback: Callback,
) -> Result<
    DuplexStreamHandle<
        InputDevice::StreamHandle<InputProxy>,
        OutputDevice::StreamHandle<DuplexCallback<Callback>>,
    >,
    DuplexCallbackError<InputDevice::Error, OutputDevice::Error>,
> {
    let (producer, consumer) = rtrb::RingBuffer::new(input_config.samplerate as _);
    let output_sample_rate = Arc::new(AtomicU64::new(0));
    let input_handle = input_device
        .create_input_stream(
            input_config,
            InputProxy {
                buffer: producer,
                output_sample_rate: output_sample_rate.clone(),
            },
        )
        .map_err(DuplexCallbackError::InputError)?;
    let output_handle = output_device
        .create_output_stream(
            output_config,
            DuplexCallback {
                input: consumer,
                callback,
                storage: AudioBuffer::zeroed(
                    input_config.channels.count(),
                    input_config.samplerate as _,
                ),
                output_sample_rate,
            },
        )
        .map_err(DuplexCallbackError::OutputError)?;
    Ok(DuplexStreamHandle {
        input_handle,
        output_handle,
    })
}
