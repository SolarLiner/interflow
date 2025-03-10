//! Duplex audio streams allow for processing both input and output audio data in a single callback. 

use crate::audio_buffer::AudioBuffer;
use crate::channel_map::Bitset;
use crate::{
    AudioCallbackContext, AudioInput, AudioInputCallback, AudioInputDevice, AudioOutput,
    AudioOutputCallback, AudioOutputDevice, AudioStreamHandle, SendEverywhereButOnWeb,
    StreamConfig,
};
use ndarray::{ArrayView1, ArrayViewMut1};
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// A callback that processes both input and output audio data.
pub trait AudioDuplexCallback: 'static + SendEverywhereButOnWeb {
    /// Process audio data from both input and output streams.
    fn on_audio_data(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        output: AudioOutput<f32>,
    );
}

/// A duplex audio stream.
pub struct DuplexStream<Callback, Error> {
    input_stream: Box<dyn AudioStreamHandle<InputProxy, Error = Error>>,
    output_stream: Box<dyn AudioStreamHandle<DuplexCallback<Callback>, Error = Error>>,
}

/// A proxy for input audio data.
pub struct InputProxy {
    buffer: rtrb::Producer<f32>,
    output_sample_rate: Arc<AtomicU64>,
}

impl AudioInputCallback for InputProxy {
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>) {
        if self.buffer.slots() < input.buffer.num_samples() * input.buffer.num_channels() {
            eprintln!("Not enough slots to buffer input");
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

fn lerp(x: f32, a: ArrayView1<f32>, b: ArrayView1<f32>, mut out: ArrayViewMut1<f32>) {
    assert_eq!(out.len(), a.len());
    assert_eq!(out.len(), b.len());
    for i in 0..out.len() {
        out[i] = lerpf(x, a[i], b[i]);
    }
}

fn lerpf(x: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * x
}

/// Error type for duplex audio streams.
#[derive(Debug, Error)]
#[error(transparent)]
pub enum DuplexCallbackError<InputError, OutputError> {
    /// An error occurred in the input stream.
    InputError(InputError),
    /// An error occurred in the output stream.
    OutputError(OutputError),
    /// An error occurred in the callback.
    Other(Box<dyn Error>),
}

/// A callback that forwards input audio data to an output audio stream.
pub struct DuplexCallback<Callback> {
    input: rtrb::Consumer<f32>,
    callback: Callback,
    storage: AudioBuffer<f32>,
    output_sample_rate: Arc<AtomicU64>,
}

impl<Callback> DuplexCallback<Callback> {
    /// Extract the inner callback from the duplex callback.
    pub fn into_inner(self) -> Result<Callback, Box<dyn Error>> {
        Ok(self.callback)
    }
}

impl<Callback: AudioDuplexCallback> AudioOutputCallback for DuplexCallback<Callback> {
    fn on_output_data(&mut self, context: AudioCallbackContext, output: AudioOutput<f32>) {
        self.output_sample_rate
            .store(context.stream_config.samplerate as _, Ordering::SeqCst);
        let num_channels = self.storage.num_channels();
        for i in 0..output.buffer.num_samples() {
            let mut frame = self.storage.get_frame_mut(i);
            for ch in 0..num_channels {
                frame[ch] = self.input.pop().unwrap_or(0.0);
            }
        }
        let input = AudioInput {
            timestamp: context.timestamp,
            buffer: self.storage.slice(..output.buffer.num_samples()),
        };
        self.callback.on_audio_data(context, input, output);
    }
}

/// A handle to a duplex audio stream.
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

    fn eject(self) -> Result<Callback, Self::Error> {
        self.input_handle.eject().map_err(DuplexCallbackError::InputError)?;
        let duplex_callback = self.output_handle.eject().map_err(DuplexCallbackError::OutputError)?;
        Ok(duplex_callback.into_inner().map_err(DuplexCallbackError::Other)?)
    }
}

/// Create a new duplex audio stream with the given input and output devices, configurations, and
/// callback.
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
    let input_handle = input_device.create_input_stream(
        input_config,
        InputProxy {
            buffer: producer,
            output_sample_rate: output_sample_rate.clone(),
        },
    ).map_err(DuplexCallbackError::InputError)?;
    let output_handle = output_device.create_output_stream(
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
    ).map_err(DuplexCallbackError::OutputError)?;
    Ok(DuplexStreamHandle {
        input_handle,
        output_handle,
    })
}
