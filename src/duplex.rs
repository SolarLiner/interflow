//! Module for simultaneous input/output audio processing
//!
//! This module includes a proxy for gathering an input audio stream and optionally processing it to resample it to the
//! output sample rate.
use crate::audio_buffer::AudioRef;
use crate::channel_map::Bitset;
use crate::{
    AudioCallback, AudioCallbackContext, AudioDevice, AudioInput, AudioOutput, AudioStreamHandle,
    SendEverywhereButOnWeb, StreamConfig,
};
use fixed_resample::{PushStatus, ReadStatus, ResamplingChannelConfig};
use std::num::NonZeroUsize;
use std::ops::IndexMut;
use thiserror::Error;

const MAX_CHANNELS: usize = 64;

/// Type which handles both a duplex stream handle.
pub struct DuplexStream<Callback, Error> {
    _input_stream: Box<dyn AudioStreamHandle<InputProxy, Error = Error>>,
    _output_stream: Box<dyn AudioStreamHandle<DuplexCallback<Callback>, Error = Error>>,
}

/// Input proxy for transferring an input signal to a separate output callback to be processed as a duplex stream.
pub struct InputProxy {
    producer: Option<fixed_resample::ResamplingProd<f32, MAX_CHANNELS>>,
    scratch_buffer: Option<Box<[f32]>>, // TODO: switch to non-interleaved processing
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
                scratch_buffer: None,
                send_consumer,
            },
            produce_output_samplerate,
            receive_consumer,
        )
    }

    fn change_output_samplerate(
        &mut self,
        context: AudioCallbackContext,
        output_samplerate: u32,
    ) -> bool {
        let Some(num_channels) = NonZeroUsize::new(context.stream_config.output_channels) else {
            log::error!("Input proxy: no input channels given");
            return true;
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
        false
    }
}

impl AudioCallback for InputProxy {
    fn prepare(&mut self, context: AudioCallbackContext) {
        let len = context.stream_config.input_channels * context.stream_config.max_frame_count;
        self.scratch_buffer = Some(Box::from_iter(std::iter::repeat_n(0.0, len)));
    }

    fn process_audio(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        output: AudioOutput<f32>,
    ) {
        debug_assert_eq!(
            0,
            output.buffer.num_channels(),
            "Input proxy should not be receiving audio output data"
        );
        log::trace!(num_samples = input.buffer.num_frames(), num_channels = input.buffer.num_channels();
            "InputProxy::process_audio");

        if let Ok(output_samplerate) = self.receive_output_samplerate.pop() {
            if self.change_output_samplerate(context, output_samplerate) {
                return;
            }
        }
        let Some(producer) = &mut self.producer else {
            log::debug!("No resampling producer available, dropping input data");
            return;
        };

        let scratch = self
            .scratch_buffer
            .as_mut()
            .unwrap()
            .index_mut(0..input.buffer.num_frames());
        let len = input.buffer.num_frames() * input.buffer.num_channels();
        debug_assert!(
            input.buffer.copy_into_interleaved(scratch),
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

#[derive(Debug, Error)]
#[error(transparent)]
/// Represents errors that can occur during duplex stream operations.
pub enum DuplexCallbackError<InputError, OutputError> {
    /// No input channels given
    #[error("No input channels given")]
    NoInputChannels,
    /// An error occurred in the input stream
    InputError(InputError),
    /// An error occurred in the output stream
    OutputError(OutputError),
}

/// [`AudioOutputCallback`] implementation for which runs the provided [`AudioDuplexCallback`].
pub struct DuplexCallback<Callback> {
    input: Option<fixed_resample::ResamplingCons<f32>>,
    receive_consumer: rtrb::Consumer<fixed_resample::ResamplingCons<f32>>,
    send_samplerate: rtrb::Producer<u32>,
    callback: Callback,
    storage_raw: Option<Box<[f32]>>,
    current_samplerate: u32,
    num_input_channels: usize,
    resample_config: ResamplingChannelConfig,
}

impl<Callback> DuplexCallback<Callback> {
    /// Consumes the DuplexCallback and returns the underlying callback implementation.
    ///
    /// # Returns
    /// The wrapped callback instance or an error if extraction fails
    pub fn into_inner(self) -> Callback {
        self.callback
    }
}

impl<Callback: AudioCallback> AudioCallback for DuplexCallback<Callback> {
    fn prepare(&mut self, context: AudioCallbackContext) {
        let len = context.stream_config.output_channels * context.stream_config.max_frame_count;
        self.storage_raw = Some(Box::from_iter(std::iter::repeat_n(0.0, len)));
        self.callback.prepare(context);
    }

    fn process_audio(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        output: AudioOutput<f32>,
    ) {
        debug_assert_eq!(
            0,
            input.buffer.num_channels(),
            "DuplexCallback should not be receiving audio input data"
        );
        log::trace!(num_samples = output.buffer.num_frames(), num_channels = output.buffer.num_channels();
            "DuplexCallback::process_audio");

        // If changed, send the new output samplerate to input proxy
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

        // Receive input from the resampled proxy
        let frames = output.buffer.num_frames();
        let storage = if let Some(input) = &mut self.input {
            let len = input.num_channels().get() * frames;
            let storage = self.storage_raw.as_mut().unwrap();
            let slice = storage.index_mut(..len);
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
        self.callback.process_audio(context, input, output);
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
/// * `InputHandle` - The type of the input stream handle must implement `AudioStreamHandle<InputProxy>`
/// * `OutputHandle` - The type of the output stream handle must implement `AudioStreamHandle<DuplexCallback<Callback>>`
///
/// # Example
///
/// ```no_run
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
/// impl AudioCallback for MyCallback {
///     fn prepare(&mut self, context: AudioCallbackContext) {}
///     fn process_audio(&mut self, context: AudioCallbackContext, input: AudioInput<f32>, output: AudioOutput<f32>) {
///         // Implementation left as an exercise to the reader
///     }
/// }
///
/// // Create and use a duplex stream
/// let config = output_device.default_config().unwrap();
/// let stream_handle = create_duplex_stream(
///     input_device,
///     output_device,
///     MyCallback::new(),
///     DuplexStreamConfig::new(config),
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
        Ok(duplex_callback.into_inner())
    }
}

/// Configuration type for manual duplex streams.
#[derive(Debug, Copy, Clone)]
pub struct DuplexStreamConfig {
    /// Input stream configuration
    pub stream_config: StreamConfig,
    /// Use high-quality resampling. Increases latency and CPU usage.
    pub high_quality_resampling: bool,
    /// Target latency. May be higher if the resampling takes too much latency.
    pub target_latency_secs: f32,
}

impl DuplexStreamConfig {
    pub(crate) fn input_config(&self) -> StreamConfig {
        StreamConfig {
            output_channels: 0,
            ..self.stream_config
        }
    }

    pub(crate) fn output_config(&self) -> StreamConfig {
        StreamConfig {
            input_channels: 0,
            ..self.stream_config
        }
    }
}

impl DuplexStreamConfig {
    /// Create a new duplex stream config with the provided input and output stream configuration, and default
    /// resampler values.
    pub fn new(stream_config: StreamConfig) -> Self {
        Self {
            stream_config,
            high_quality_resampling: false,
            target_latency_secs: 0.01,
        }
    }
}

/// Type alias of the result of creating a duplex stream.
pub type DuplexStreamResult<In, Out, Callback> = Result<
    DuplexStreamHandle<
        <In as AudioDevice>::StreamHandle<InputProxy>,
        <Out as AudioDevice>::StreamHandle<DuplexCallback<Callback>>,
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
/// impl AudioCallback for MyCallback {
///     fn prepare(&mut self, context: AudioCallbackContext) {}
///     fn process_audio(&mut self, context: AudioCallbackContext, input: AudioInput<f32>, output: AudioOutput<f32>) {
///         // Implementation left as an exercise to the reader
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
/// let config = output_device.default_config().unwrap();
/// let duplex_stream = create_duplex_stream(
///     input_device,
///     output_device,
///     callback,
///     DuplexStreamConfig::new(config),
/// ).expect("Failed to create duplex stream");
///
/// ```
#[allow(clippy::type_complexity)] // Allowing because moving to a type alias would be just as complex
pub fn create_duplex_stream<
    InputDevice: AudioDevice,
    OutputDevice: AudioDevice,
    Callback: AudioCallback,
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
        .create_stream(config.input_config(), proxy)
        .map_err(DuplexCallbackError::InputError)?;
    let output_handle = output_device
        .create_stream(
            config.output_config(),
            DuplexCallback {
                input: None,
                send_samplerate,
                receive_consumer,
                callback,
                storage_raw: None,
                current_samplerate: 0,
                num_input_channels: config.stream_config.input_channels,
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
