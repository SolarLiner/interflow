use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use asio_sys as asio;
use num_traits::PrimInt;

use crate::{
    audio_buffer::{AudioMut, AudioRef},
    device::{
        AudioDevice, AudioDuplexDevice, AudioInputDevice, AudioOutputDevice, Channel, DeviceType,
    },
    duplex::AudioDuplexCallback,
    stream::{
        AudioCallbackContext, AudioInput, AudioInputCallback, AudioOutput, AudioOutputCallback,
        StreamConfig,
    },
    timestamp::{self, Timestamp},
    SendEverywhereButOnWeb,
};

use super::{error::AsioError, stream::AsioStream};

/// The ASIO device.
#[derive(Clone)]
pub struct AsioDevice {
    driver: Arc<asio::Driver>,
    device_type: DeviceType,
    asio_streams: Arc<Mutex<asio::AsioStreams>>,
}

impl AsioDevice {
    /// Create a new ASIO device.
    pub fn new(driver: Arc<asio::Driver>) -> Result<Self, AsioError> {
        let is_input = driver.channels()?.ins > 0;
        let is_output = driver.channels()?.outs > 0;
        let device_type = match (is_input, is_output) {
            (true, true) => DeviceType::DUPLEX,
            (true, false) => DeviceType::INPUT,
            (false, true) => DeviceType::OUTPUT,
            // todo
            (false, false) => return Err(AsioError::BackendError(asio::AsioError::NoDrivers)),
        };
        let asio_streams = Arc::new(Mutex::new(asio::AsioStreams {
            input: None,
            output: None,
        }));
        Ok(AsioDevice {
            driver,
            device_type,
            asio_streams,
        })
    }

    pub fn device_type(&self) -> DeviceType {
        self.device_type
    }

    /// Create an input stream with the given configuration.
    fn create_input_stream(&self, stream_config: StreamConfig) -> Result<usize, AsioError> {
        let num_channels = stream_config.channels as usize;
        let buffer_size = match stream_config.buffer_size_range {
            (Some(min), Some(max)) if min == max => Some(min as i32),

            _ => None,
        };

        self.driver.set_sample_rate(stream_config.samplerate)?;

        let mut streams = self.asio_streams.lock().unwrap();

        match streams.input {
            Some(ref input) => Ok(input.buffer_size as usize),
            None => {
                let output = streams.output.take();
                self.driver
                    .prepare_input_stream(output, num_channels, buffer_size)
                    .map(|new_streams| {
                        let bs = match new_streams.input {
                            Some(ref inp) => inp.buffer_size as usize,
                            None => unreachable!(),
                        };
                        *streams = new_streams;
                        bs
                    })
                    .map_err(AsioError::BackendError)
            }
        }
    }

    /// Creates an output stream with the given configuration.
    fn create_output_stream(&self, stream_config: StreamConfig) -> Result<usize, AsioError> {
        let num_channels = stream_config.channels as usize;
        let buffer_size = match stream_config.buffer_size_range {
            (Some(min), Some(max)) if min == max => Some(min as i32),
            _ => None,
        };

        self.driver.set_sample_rate(stream_config.samplerate)?;

        let mut streams = self.asio_streams.lock().unwrap();

        match streams.output {
            Some(ref output) => Ok(output.buffer_size as usize),
            None => {
                let input = streams.input.take();
                self.driver
                    .prepare_output_stream(input, num_channels, buffer_size)
                    .map(|new_streams| {
                        let bs = match new_streams.output {
                            Some(ref out) => out.buffer_size as usize,
                            None => unreachable!(),
                        };
                        *streams = new_streams;
                        bs
                    })
                    .map_err(AsioError::BackendError)
            }
        }
    }
}

impl AudioDevice for AsioDevice {
    type Error = AsioError;

    fn name(&self) -> Cow<str> {
        Cow::Borrowed(self.driver.name())
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        let sample_rate = config.samplerate;
        self.driver.can_sample_rate(sample_rate).unwrap_or(false)
            && self.driver.channels().is_ok_and(|channels| {
                let num_channels = config.channels as i32;
                if self.device_type.contains(DeviceType::DUPLEX) {
                    channels.ins >= num_channels && channels.outs >= num_channels
                } else if self.device_type.contains(DeviceType::INPUT) {
                    channels.ins >= num_channels
                } else if self.device_type.contains(DeviceType::OUTPUT) {
                    channels.outs >= num_channels
                } else {
                    false
                }
            })
            && self.driver.buffersize_range().is_ok_and(|(min, max)| {
                match config.buffer_size_range {
                    (Some(min_config), Some(max_config)) => {
                        min_config >= min as usize && max_config <= max as usize
                    }
                    _ => false,
                }
            })
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        let sample_rates = [44100.0, 48000.0, 96000.0, 192000.0, 384000.0, 768000.0];

        let buffer_size_range = self
            .driver
            .buffersize_range()
            .ok()
            .map(|(min, max)| (Some(min as usize), Some(max as usize)))?;

        let channels = self.driver.channels().ok().and_then(|channels| {
            if self.device_type.contains(DeviceType::DUPLEX) {
                Some(channels.ins.max(channels.outs) as u32)
            } else if self.device_type.contains(DeviceType::INPUT) {
                Some(channels.ins as u32)
            } else if self.device_type.contains(DeviceType::OUTPUT) {
                Some(channels.outs as u32)
            } else {
                // Return None if device type is neither input nor output
                None
            }
        })?;

        Some(sample_rates.into_iter().filter_map(move |samplerate| {
            self.driver
                .can_sample_rate(samplerate)
                .ok()
                .filter(|&can| can)
                .map(|_| StreamConfig {
                    channels,
                    samplerate,
                    buffer_size_range,
                    exclusive: false,
                })
        }))
    }
}

impl AudioInputDevice for AsioDevice {
    type StreamHandle<Callback: AudioInputCallback> = AsioStream<Callback>;

    fn input_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        let channels = self.driver.channels()?.ins as u32;
        let samplerate = self.driver.sample_rate()?;
        let (min_buffer_size, max_buffer_size) = self.driver.buffersize_range()?;
        Ok(StreamConfig {
            channels,
            samplerate,
            buffer_size_range: (
                Some(min_buffer_size as usize),
                Some(max_buffer_size as usize),
            ),
            exclusive: false,
        })
    }

    fn create_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let input_data_type = self.driver.input_data_type()?;

        let num_channels = stream_config.channels as usize;

        let buffer_size = self.create_input_stream(stream_config)?;
        let num_samples = buffer_size * num_channels;

        let mut buffer = vec![0.0f32; num_samples];

        let mut streams = self.asio_streams.lock().unwrap();
        let input_stream = streams.input.take().ok_or(AsioError::MultipleStreams)?;

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        let mut callback = Some(callback);

        let mut timestamp = Timestamp {
            samplerate: stream_config.samplerate,
            counter: 0,
        };

        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return;
            }

            let buffer_index = callback_info.buffer_index as usize;

            timestamp += input_stream.buffer_size as u64;

            let context = AudioCallbackContext {
                stream_config,
                timestamp,
            };

            let input = create_input(
                &input_data_type,
                &input_stream,
                &mut buffer,
                buffer_index,
                num_channels,
                timestamp,
            );

            if let Some(callback) = &mut callback {
                callback.on_input_data(context, input);
            }
        });

        self.driver.start()?;

        Ok(AsioStream {
            driver: self.driver.clone(),
            callback_id,
            callback_retrieve: tx,
        })
    }
}

impl AudioOutputDevice for AsioDevice {
    type StreamHandle<Callback: AudioOutputCallback> = AsioStream<Callback>;

    fn output_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        let channels = self.driver.channels()?.outs as u32;
        let samplerate = self.driver.sample_rate()?;
        let (min_buffer_size, max_buffer_size) = self.driver.buffersize_range()?;
        Ok(StreamConfig {
            channels,
            samplerate,
            buffer_size_range: (
                Some(min_buffer_size as usize),
                Some(max_buffer_size as usize),
            ),
            exclusive: false,
        })
    }

    fn create_output_stream<Callback: SendEverywhereButOnWeb + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let output_data_type = self.driver.output_data_type()?;

        let num_channels = stream_config.channels as usize;

        let buffer_size = self.create_output_stream(stream_config)?;
        let num_samples = buffer_size * num_channels;

        let mut buffer = vec![0.0f32; num_samples];

        let mut streams = self.asio_streams.lock().unwrap();
        let mut output_stream = streams.output.take().ok_or(AsioError::MultipleStreams)?;

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        let mut callback = Some(callback);

        let mut timestamp = Timestamp {
            samplerate: stream_config.samplerate,
            counter: 0,
        };

        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return;
            }

            let buffer_index = callback_info.buffer_index as usize;

            timestamp += output_stream.buffer_size as u64;

            let context = AudioCallbackContext {
                stream_config,
                timestamp,
            };

            let output = create_output(
                &output_data_type,
                &mut output_stream,
                &mut buffer,
                buffer_index,
                num_channels,
                timestamp,
            );

            if let Some(callback) = &mut callback {
                callback.on_output_data(context, output);
            }
        });

        self.driver.start()?;

        Ok(AsioStream {
            driver: self.driver.clone(),
            callback_id,
            callback_retrieve: tx,
        })
    }
}

impl AudioDuplexDevice for AsioDevice {
    type StreamHandle<Callback: AudioDuplexCallback> = AsioStream<Callback>;

    fn default_duplex_config(&self) -> Result<StreamConfig, Self::Error> {
        let channels = self.driver.channels()?.ins as u32;
        let samplerate = self.driver.sample_rate()?;
        let (min_buffer_size, max_buffer_size) = self.driver.buffersize_range()?;
        Ok(StreamConfig {
            channels,
            samplerate,
            buffer_size_range: (
                Some(min_buffer_size as usize),
                Some(max_buffer_size as usize),
            ),
            exclusive: false,
        })
    }

    fn create_duplex_stream<Callback: SendEverywhereButOnWeb + AudioDuplexCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let output_data_type = self.driver.output_data_type()?;
        let input_data_type = self.driver.input_data_type()?;

        let num_channels = stream_config.channels as usize;

        // This creates both input and output streams if available
        let buffer_size = self.create_output_stream(stream_config)?;
        let num_samples = buffer_size * num_channels;

        let mut input_buffer = vec![0.0f32; num_samples];
        let mut output_buffer = vec![0.0f32; num_samples];

        let mut streams = self.asio_streams.lock().unwrap();
        let mut output_stream = streams.output.take().ok_or(AsioError::MultipleStreams)?;
        let input_stream = streams.input.take().ok_or(AsioError::MultipleStreams)?;

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        let mut callback = Some(callback);

        let mut timestamp = Timestamp {
            samplerate: stream_config.samplerate,
            counter: 0,
        };

        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return;
            }

            let buffer_index = callback_info.buffer_index as usize;

            timestamp += output_stream.buffer_size as u64;

            let input = create_input(
                &input_data_type,
                &input_stream,
                &mut input_buffer,
                buffer_index,
                num_channels,
                timestamp,
            );

            let output = create_output(
                &output_data_type,
                &mut output_stream,
                &mut output_buffer,
                buffer_index,
                num_channels,
                timestamp,
            );

            let context = AudioCallbackContext {
                stream_config,
                timestamp,
            };

            if let Some(callback) = &mut callback {
                callback.on_audio_data(context, input, output);
            }
        });

        self.driver.start()?;

        Ok(AsioStream {
            driver: self.driver.clone(),
            callback_id,
            callback_retrieve: tx,
        })
    }
}

// HELPERS

/// Create an `AudioOutput` from the ASIO stream and the buffer.
unsafe fn create_output<'a>(
    output_data_type: &asio::AsioSampleType,
    asio_stream: &mut asio::AsioStream,
    buffer: &'a mut [f32],
    buffer_index: usize,
    num_channels: usize,
    timestamp: timestamp::Timestamp,
) -> AudioOutput<'a, f32> {
    let audio_output_buffer = match output_data_type {
        asio::AsioSampleType::ASIOSTInt16MSB => create_output_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_be,
            f32_to_i16,
        ),
        asio::AsioSampleType::ASIOSTInt16LSB => create_output_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_le,
            f32_to_i16,
        ),
        asio::AsioSampleType::ASIOSTInt24MSB => create_output_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_be,
            f32_to_i24,
        ),
        asio::AsioSampleType::ASIOSTInt24LSB => create_output_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_le,
            f32_to_i24,
        ),
        asio::AsioSampleType::ASIOSTInt32MSB => create_output_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_be,
            f32_to_i32,
        ),
        asio::AsioSampleType::ASIOSTInt32LSB => create_output_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_le,
            f32_to_i32,
        ),
        asio::AsioSampleType::ASIOSTFloat32MSB => create_output_buffer::<u32>(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_be,
            f32::to_bits,
        ),
        asio::AsioSampleType::ASIOSTFloat32LSB => create_output_buffer::<u32>(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            to_le,
            f32::to_bits,
        ),
        unsupported_format => {
            unreachable!("returned with unsupported format {:?}", unsupported_format)
        }
    };

    AudioOutput {
        timestamp,
        buffer: audio_output_buffer,
    }
}

/// Create an `AudioInput` from the ASIO stream and the buffer.
unsafe fn create_input<'a>(
    input_data_type: &asio::AsioSampleType,
    asio_stream: &asio::AsioStream,
    buffer: &'a mut [f32],
    buffer_index: usize,
    num_channels: usize,
    timestamp: timestamp::Timestamp,
) -> AudioInput<'a, f32> {
    let audio_input_buffer = match input_data_type {
        asio::AsioSampleType::ASIOSTInt16MSB => create_input_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_be,
            i16_to_f32,
        ),
        asio::AsioSampleType::ASIOSTInt16LSB => create_input_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_le,
            i16_to_f32,
        ),
        asio::AsioSampleType::ASIOSTInt24MSB => create_input_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_be,
            i24_to_f32,
        ),
        asio::AsioSampleType::ASIOSTInt24LSB => create_input_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_le,
            i24_to_f32,
        ),
        asio::AsioSampleType::ASIOSTInt32MSB => create_input_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_be,
            i32_to_f32,
        ),
        asio::AsioSampleType::ASIOSTInt32LSB => create_input_buffer(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_le,
            i32_to_f32,
        ),
        asio::AsioSampleType::ASIOSTFloat32MSB => create_input_buffer::<u32>(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_be,
            f32::from_bits,
        ),
        asio::AsioSampleType::ASIOSTFloat32LSB => create_input_buffer::<u32>(
            asio_stream,
            buffer,
            buffer_index,
            num_channels,
            from_le,
            f32::from_bits,
        ),
        unsupported_format => {
            unreachable!("returned with unsupported format {:?}", unsupported_format)
        }
    };

    AudioInput {
        timestamp,
        buffer: audio_input_buffer,
    }
}

unsafe fn create_input_buffer<'a, SampleType: Copy>(
    asio_stream: &asio::AsioStream,
    buffer: &'a mut [f32],
    buffer_index: usize,
    num_channels: usize,
    from_endian: impl Fn(SampleType) -> SampleType,
    to_f32: impl Fn(SampleType) -> f32,
) -> AudioRef<'a, f32> {
    for channel_index in 0..num_channels {
        let channel_buffer =
            asio_channel_slice::<SampleType>(asio_stream, buffer_index, channel_index);
        for (frame, asio_sample) in buffer.chunks_mut(num_channels).zip(channel_buffer) {
            frame[channel_index] = to_f32(from_endian(*asio_sample));
        }
    }
    AudioRef::from_interleaved(buffer, num_channels).unwrap()
}

unsafe fn create_output_buffer<'a, SampleType: Copy>(
    asio_stream: &mut asio::AsioStream,
    buffer: &'a mut [f32],
    buffer_index: usize,
    num_channels: usize,
    to_endian: impl Fn(SampleType) -> SampleType,
    from_f32: impl Fn(f32) -> SampleType,
) -> AudioMut<'a, f32> {
    for channel_index in 0..num_channels {
        let channel_buffer =
            asio_channel_slice_mut::<SampleType>(asio_stream, buffer_index, channel_index);
        for (frame, asio_sample) in buffer.chunks_mut(num_channels).zip(channel_buffer) {
            *asio_sample = to_endian(from_f32(frame[channel_index]));
        }
    }
    AudioMut::from_interleaved_mut(buffer, num_channels).unwrap()
}

unsafe fn asio_channel_slice<T>(
    asio_stream: &asio::AsioStream,
    buffer_index: usize,
    channel_index: usize,
) -> &[T] {
    let buffer_size = asio_stream.buffer_size as usize;
    let buff_ptr: *const T =
        asio_stream.buffer_infos[channel_index].buffers[buffer_index] as *const _;
    std::slice::from_raw_parts(buff_ptr, buffer_size)
}

unsafe fn asio_channel_slice_mut<T>(
    asio_stream: &mut asio::AsioStream,
    buffer_index: usize,
    channel_index: usize,
) -> &mut [T] {
    let buffer_size = asio_stream.buffer_size as usize;
    let buff_ptr: *mut T = asio_stream.buffer_infos[channel_index].buffers[buffer_index] as *mut _;
    std::slice::from_raw_parts_mut(buff_ptr, buffer_size)
}

/// Helper function to convert from little endianness.
fn from_le<T: PrimInt>(t: T) -> T {
    T::from_le(t)
}

/// Helper function to convert from big endianness.
fn from_be<T: PrimInt>(t: T) -> T {
    T::from_be(t)
}

/// Helper function to convert to little endianness.
fn to_le<T: PrimInt>(t: T) -> T {
    t.to_le()
}

/// Helper function to convert to big endianness.
fn to_be<T: PrimInt>(t: T) -> T {
    t.to_be()
}

/// Helper function to convert from i16 to f32.
fn i16_to_f32(i: i16) -> f32 {
    i as f32 / i16::MAX as f32
}

/// Helper function to convert from i32 to f32.
fn i32_to_f32(i: i32) -> f32 {
    i as f32 / i32::MAX as f32
}

/// Helper function to convert from i24 to f32.
fn i24_to_f32(i: i32) -> f32 {
    i as f32 / 0x7FFFFF as f32
}

/// Helper function to convert from f32 to i16.
fn f32_to_i16(f: f32) -> i16 {
    (f * i16::MAX as f32) as i16
}

/// Helper function to convert from f32 to i32.
fn f32_to_i32(f: f32) -> i32 {
    (f * i32::MAX as f32) as i32
}

/// Helper function to convert from f32 to i24.
fn f32_to_i24(f: f32) -> i32 {
    (f * 0x7FFFFF as f32) as i32
}
