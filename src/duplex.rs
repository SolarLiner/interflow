//! Module for simultaneous input/output audio processing
//!
//! This module includes a proxy for gathering an input audio stream, and optionally process it to resample it to the
//! output sample rate.
use crate::{audio_buffer::AudioRef, channel_map::Bitset, device::AudioDevice};
use crate::device::{AudioInputDevice, AudioOutputDevice};
use crate::stream::{
    AudioCallbackContext, AudioInputCallback, AudioOutputCallback, AudioStreamHandle, StreamConfig,
};
use crate::stream::{AudioInput, AudioOutput};
use crate::SendEverywhereButOnWeb;
use std::error::Error;
use std::num::NonZeroUsize;
use fixed_resample::{PushStatus, ReadStatus, ResamplingChannelConfig};
use thiserror::Error;

const MAX_CHANNELS: usize = 64;

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
///
/// # Type Parameters
///
/// * `Callback` - The type of the callback implementation
/// * `Error` - The type of error that can occur
pub struct DuplexStream<Callback, Error> {
    _input_stream: Box<dyn AudioStreamHandle<InputProxy, Error = Error>>,
    _output_stream: Box<dyn AudioStreamHandle<DuplexCallback<Callback>, Error = Error>>,
}

/// Input proxy for transferring an input signal to a separate output callback to be processed as a duplex stream.
///
/// This struct handles the resampling of input audio data to match the output sample rate.
pub struct InputProxy {
    producer: Option<fixed_resample::ResamplingProd<f32, MAX_CHANNELS>>,
    receive_output_samplerate: rtrb::Consumer<u32>,
    send_consumer: rtrb::Producer<fixed_resample::ResamplingCons<f32>>,
}

impl InputProxy {
    /// Create a new input proxy for transferring an input stream, resample it, and make it available in an output
    /// stream.
    pub fn new() -> (
        Self,
        rtrb::Producer<u32>,
        rtrb::Consumer<fixed_resample::ResamplingCons<f32>>,
    ) {
        let (send_consumer, receive_consumer) = rtrb::RingBuffer::new(1);
        let (produce_output_samplerate, receive_output_samplerate) = rtrb::RingBuffer::new(1);
        (
            Self {
                producer: None,
                receive_output_samplerate,
                send_consumer,
            },
            produce_output_samplerate,
            receive_consumer,
        )
    }
}

impl AudioInputCallback for InputProxy {
    /// Processes incoming audio data and stores it in the internal buffer.
    ///
    /// This method handles sample rate conversion between input and output streams.
    ///
    /// Handles sample rate conversion between input and output streams.
    ///
    /// # Arguments
    /// * `context` - The context containing stream configuration and timing information
    /// * `input` - The input audio buffer containing captured audio data
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>) {
        log::trace!(num_samples = input.buffer.num_samples(), num_channels = input.buffer.num_channels();
            "on_input_data");
        if let Ok(output_samplerate) = self.receive_output_samplerate.pop() {
            let Some(num_channels) = NonZeroUsize::new(context.stream_config.channels.count())
            else {
                log::error!("Input proxy: no input channels given");
                return;
            };
            let input_samplerate = context.stream_config.samplerate as _;
            log::debug!(
                "Creating resampling channel ({} Hz) -> ({} Hz) ({} channels)",
                input_samplerate,
                output_samplerate,
                num_channels.get()
            );
            let (tx, rx) = fixed_resample::resampling_channel(
                num_channels,
                input_samplerate,
                output_samplerate,
                ResamplingChannelConfig {
                    latency_seconds: 0.01,
                    quality: fixed_resample::ResampleQuality::Low,
                    ..Default::default()
                },
            );
            self.producer.replace(tx);
            match self.send_consumer.push(rx) {
                Ok(_) => {
                    log::debug!(
                        "Input proxy: resampling channel ({} Hz) sent",
                        context.stream_config.samplerate
                    );
                }
                Err(err) => {
                    log::error!("Input proxy: cannot send resampling channel: {}", err);
                }
            }
        }
        let Some(producer) = &mut self.producer else {
            log::debug!("No resampling producer available, dropping input data");
            return;
        };

        let mut scratch = [0f32; 32 * MAX_CHANNELS];
        for slice in input.buffer.chunks(32) {
            let len = slice.num_samples() * slice.num_channels();
            debug_assert!(
                slice.copy_into_interleaved(&mut scratch[..len]),
                "Cannot fail: len is computed from slice itself"
            );
            match producer.push_interleaved(&scratch[..len]) {
                PushStatus::OverflowOccurred { .. } => {
                    log::error!("Input proxy: overflow occurred");
                }
                PushStatus::UnderflowCorrected { .. } => {
                    log::error!("Input proxy: underflow corrected");
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug, Error)]
#[error(transparent)]
/// Represents errors that can occur during duplex stream operations.
///
/// # Type Parameters
///
/// * `InputError` - The type of error that can occur in the input stream
/// * `OutputError` - The type of error that can occur in the output stream
pub enum DuplexCallbackError<InputError, OutputError> {
    /// No input channels given
    #[error("No input channels given")]
    NoInputChannels,
    /// An error occurred in the input stream
    InputError(InputError),
    /// An error occurred in the output stream
    OutputError(OutputError),
    /// An error that doesn't fit into other categories
    Other(Box<dyn Error>),
}

/// [`AudioOutputCallback`] implementation for which runs the provided [`AudioDuplexCallback`].
///
/// This struct handles the processing of audio data in a duplex stream.
pub struct DuplexCallback<Callback> {
    input: Option<fixed_resample::ResamplingCons<f32>>,
    receive_consumer: rtrb::Consumer<fixed_resample::ResamplingCons<f32>>,
    send_samplerate: rtrb::Producer<u32>,
    callback: Callback,
    storage_raw: Box<[f32]>,
    current_samplerate: u32,
    num_input_channels: usize,
    resample_config: ResamplingChannelConfig,
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
        // If changed, send new output samplerate to input proxy
        let samplerate = context.stream_config.samplerate as u32;
        if samplerate != self.current_samplerate && self.send_samplerate.push(samplerate).is_ok() {
            log::debug!("Output samplerate changed to {}", samplerate);
            self.current_samplerate = samplerate;
        }

        // Receive updated resample channel
        if let Ok(input) = self.receive_consumer.pop() {
            log::debug!(
                "Output resample channel received ({}/{} Hz)",
                input.out_sample_rate(),
                input.in_sample_rate()
            );
            self.num_input_channels = input.num_channels().get();
            self.input.replace(input);
        }

        // Receive input from proxy
        let frames = output.buffer.num_samples();
        let storage = if let Some(input) = &mut self.input {
            let len = input.num_channels().get() * frames;
            let slice = &mut self.storage_raw[..len];
            match input.read_interleaved(slice) {
                ReadStatus::UnderflowOccurred { .. } => {
                    log::error!("Output resample channel underflow occurred");
                }
                ReadStatus::OverflowCorrected { .. } => {
                    log::error!("Output resample channel overflow corrected");
                }
                _ => {}
            }
            AudioRef::from_interleaved(slice, input.num_channels().get()).unwrap()
        } else {
            AudioRef::from_interleaved(&[], self.num_input_channels).unwrap()
        };

        let input = AudioInput {
            timestamp: context.timestamp,
            buffer: storage,
        };
        // Run user callback
        self.callback.on_audio_data(context, input, output);
    }
}

/// A handle for managing a duplex audio stream that combines input and output capabilities.
///
/// This struct provides a way to control and manage a duplex audio stream that processes both
/// input and output audio data simultaneously. It wraps the individual input and output stream
/// handles and provides unified control over the duplex operation.
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
/// use interflow::duplex::{AudioDuplexCallback, DuplexStreamConfig};
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
///     output_device,
///     MyCallback::new(),
///     DuplexStreamConfig::new(input_config, output_config),
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

/// Configuration type for manual duplex streams.
#[derive(Debug, Copy, Clone)]
pub struct DuplexStreamConfig {
    /// Input stream configuration
    pub input: StreamConfig,
    /// Output stream configuration
    pub output: StreamConfig,
    /// Use high quality resampling. Increases latency and CPU usage.
    pub high_quality_resampling: bool,
    /// Target latency. May be higher if the resampling takes too much latency.
    pub target_latency_secs: f32,
}

impl DuplexStreamConfig {
    /// Create a new duplex stream config with the provided input and output stream configuration, and default
    /// resampler values.
    pub fn new(input: StreamConfig, output: StreamConfig) -> Self {
        Self {
            input,
            output,
            high_quality_resampling: false,
            target_latency_secs: 0.01,
        }
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
/// use interflow::duplex::{AudioDuplexCallback, DuplexStreamConfig};
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
///     output_device,
///     callback,
///     DuplexStreamConfig::new(input_config, output_config),
/// ).expect("Failed to create duplex stream");
///
/// ```
#[allow(clippy::type_complexity)] // Allowing because moving to a type alias would be just as complex
pub fn create_duplex_stream<
    InputDevice: AudioInputDevice,
    OutputDevice: AudioOutputDevice,
    Callback: AudioDuplexCallback,
>(
    input_device: InputDevice,
    output_device: OutputDevice,
    callback: Callback,
    config: DuplexStreamConfig,
) -> Result<
    DuplexStreamHandle<
        InputDevice::StreamHandle<InputProxy>,
        OutputDevice::StreamHandle<DuplexCallback<Callback>>,
    >,
    DuplexCallbackError<InputDevice::Error, OutputDevice::Error>,
> {
    let (proxy, send_samplerate, receive_consumer) = InputProxy::new();
    let input_handle = input_device
        .create_input_stream(config.input, proxy)
        .map_err(DuplexCallbackError::InputError)?;
    let output_handle = output_device
        .create_output_stream(
            config.output,
            DuplexCallback {
                input: None,
                send_samplerate,
                receive_consumer,
                callback,
                storage_raw: vec![0f32; 8192 * MAX_CHANNELS].into_boxed_slice(),
                current_samplerate: 0,
                num_input_channels: config.input.channels.count(),
                resample_config: ResamplingChannelConfig {
                    capacity_seconds: (2.0 * config.target_latency_secs as f64).max(0.5),
                    latency_seconds: config.target_latency_secs as f64,
                    subtract_resampler_delay: true,
                    quality: if config.high_quality_resampling {
                        fixed_resample::ResampleQuality::High
                    } else {
                        fixed_resample::ResampleQuality::Low
                    },
                    ..Default::default()
                },
            },
        )
        .map_err(DuplexCallbackError::OutputError)?;
    Ok(DuplexStreamHandle {
        input_handle,
        output_handle,
    })
}
