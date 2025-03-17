
use std::{borrow::Cow, sync::{atomic::AtomicBool, Arc, Mutex}};


use asio_sys as asio;
use num_traits::PrimInt;

use crate::{audio_buffer::{AudioMut, AudioRef}, device::{AudioDevice, AudioDuplexDevice, AudioInputDevice, AudioOutputDevice, Channel, DeviceType}, duplex::AudioDuplexCallback, stream::{AudioCallbackContext, AudioInput, AudioInputCallback, AudioOutput, AudioOutputCallback, AudioStreamHandle, StreamConfig}, timestamp::{self, Timestamp}, SendEverywhereButOnWeb};

use super::{error::AsioError, stream::AsioStream};



#[derive(Clone)]
pub struct AsioDevice {
    driver: Arc<asio::Driver>,
    device_type: DeviceType,
    asio_streams: Arc<Mutex<asio::AsioStreams>>,
}

impl AsioDevice {
    pub fn new(driver: Arc<asio::Driver>) -> Result<Self, AsioError> {
        let is_input = driver.channels()?.ins > 0;
        let is_output = driver.channels()?.outs > 0;
        let device_type = match (is_input, is_output) {
            (true, true) => DeviceType::Duplex,
            (true, false) => DeviceType::Input,
            (false, true) => DeviceType::Output,
            // todo
            (false, false) => return Err(AsioError::BackendError(asio::AsioError::NoDrivers)),
        };
        let asio_streams = Arc::new(Mutex::new(asio::AsioStreams {
            input: None,
            output: None,
        }));
        Ok(AsioDevice { driver, device_type, asio_streams })
    }

    fn create_input_stream(&self, stream_config: StreamConfig) -> Result<usize, AsioError> {

        let num_channels = stream_config.channels as usize;
        let buffer_size = match stream_config.buffer_size_range {
            (Some(min), Some(max)) if min == max => {
                Some(min as i32)
            }


            _ => None
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
                    .map_err(|e| {
                        AsioError::BackendError(e)
                    })
            }
        }
        
    }

    fn create_output_stream(&self, stream_config: StreamConfig) -> Result<usize, AsioError> {
        let num_channels = stream_config.channels as usize;
        let buffer_size = match stream_config.buffer_size_range {
            (Some(min), Some(max)) if min == max => {
                Some(min as i32)
            }
            _ => None
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
                    .map_err(|e| {
                        AsioError::BackendError(e)
                    })
            }
        }
    }
}

impl AudioDevice for AsioDevice {
    type Error = AsioError;

    fn name(&self) -> Cow<str> {
        Cow::Borrowed(self.driver.name())
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        todo!()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }
}

impl AudioInputDevice for AsioDevice {
    fn input_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    type StreamHandle<Callback: AudioInputCallback> = AsioStream<Callback>;

    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        let channels = self.driver.channels()?.ins as u32;
        let samplerate = self.driver.sample_rate()?;
        let (min_buffer_size, max_buffer_size) = self.driver.buffersize_range()?;
        Ok(StreamConfig {
            channels,
            samplerate,
            buffer_size_range: (Some(min_buffer_size as usize), Some(max_buffer_size as usize)),
            exclusive: false,
        })
    }

    fn create_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let input_data_type = self.driver.input_data_type()?;

        let num_channels = stream_config.channels as usize;

        let buffer_size = self.create_input_stream(stream_config)?;
        let num_samples = buffer_size * num_channels;

        let mut buffer = vec![0.0f32; num_samples];

        let asio_streams = self.asio_streams.clone();

        let stream_playing = Arc::new(AtomicBool::new(false));
        let playing = Arc::clone(&stream_playing);

        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            let streams = asio_streams.lock().unwrap();
            let asio_stream = match &streams.input {
                Some(asio_stream) => asio_stream,
                None => return
            };


            let buffer_index = callback_info.buffer_index as usize;

            unsafe fn create_buffer<'a, SampleType: Copy>(
                asio_stream: &asio::AsioStream,
                buffer: &'a mut [f32],
                buffer_index: usize,
                num_channels: usize,
                from_endian: impl Fn(SampleType) -> SampleType,
                to_f32: impl Fn(SampleType) -> f32,
            ) -> AudioRef<'a, f32> {
                
                
                for channel_index in 0..num_channels {
                    let channel_buffer = asio_channel_slice::<SampleType>(asio_stream, buffer_index, channel_index);
                    for (frame, asio_sample) in buffer.chunks_mut(num_channels).zip(channel_buffer) {
                        frame[channel_index] = to_f32(from_endian(*asio_sample));
                    }
                }
                AudioRef::from_interleaved(buffer, num_channels).unwrap()
            }
            

            let audio_buffer = match &input_data_type {
                asio::AsioSampleType::ASIOSTInt16MSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, from_be, i16_to_f32)
                },
                asio::AsioSampleType::ASIOSTInt16LSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, from_le, i16_to_f32)
                },
                asio::AsioSampleType::ASIOSTInt24MSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, from_be, i24_to_f32)
                },
                asio::AsioSampleType::ASIOSTInt24LSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, from_le, i24_to_f32)
                },
                asio::AsioSampleType::ASIOSTInt32MSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, from_be, i32_to_f32)
                },
                asio::AsioSampleType::ASIOSTInt32LSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, from_le, i32_to_f32)
                },
                asio::AsioSampleType::ASIOSTFloat32MSB => {
                    create_buffer::<u32>(&asio_stream, &mut buffer, buffer_index, num_channels, from_be, f32::from_bits)
                },
                asio::AsioSampleType::ASIOSTFloat32LSB => {
                    create_buffer::<u32>(&asio_stream, &mut buffer, buffer_index, num_channels, from_le, f32::from_bits)
                },
                // asio::AsioSampleType::ASIOSTFloat64MSB => todo!(),
                // asio::AsioSampleType::ASIOSTFloat64LSB => todo!(),
                unsupported_format => unreachable!(
                    "returned with unsupported format {:?}",
                    unsupported_format
                ),
            };

            let timestamp = Timestamp {
                samplerate: 41000.0,
                counter: 0,
            };
            let context = AudioCallbackContext {
                stream_config,
                timestamp,
            };
            let input = AudioInput {
                timestamp,
                buffer: audio_buffer,
            };

            callback.on_input_data(context, input);
        });

        Ok(AsioStream::new(playing, callback_id))
    }
}

impl AudioOutputDevice for AsioDevice {
    fn output_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    type StreamHandle<Callback: AudioOutputCallback> = AsioStream<Callback>;

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        let channels = self.driver.channels()?.outs as u32;
        let samplerate = self.driver.sample_rate()?;
        let (min_buffer_size, max_buffer_size) = self.driver.buffersize_range()?;
        Ok(StreamConfig {
            channels,
            samplerate,
            buffer_size_range: (Some(min_buffer_size as usize), Some(max_buffer_size as usize)),
            exclusive: false,
        })
    }

    fn create_output_stream<Callback: SendEverywhereButOnWeb + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let output_data_type = self.driver.output_data_type()?;

        let num_channels = stream_config.channels as usize;

        let buffer_size = self.create_output_stream(stream_config)?;
        let num_samples = buffer_size * num_channels;

        let mut buffer = vec![0.0f32; num_samples];

        let asio_streams = self.asio_streams.clone();

        let stream_playing = Arc::new(AtomicBool::new(false));
        let playing = Arc::clone(&stream_playing);

        let callback_id = self.driver.add_callback(move |callback_info| unsafe {
            let streams = asio_streams.lock().unwrap();
            let asio_stream = match &streams.output {
                Some(asio_stream) => asio_stream,
                None => return
            };

            let buffer_index = callback_info.buffer_index as usize;

            unsafe fn create_buffer<'a, SampleType: Copy>(
                asio_stream: &asio::AsioStream,
                buffer: &'a mut [f32],
                buffer_index: usize,
                num_channels: usize,
                to_endian: impl Fn(SampleType) -> SampleType,
                from_f32: impl Fn(f32) -> SampleType,
            ) -> AudioMut<'a, f32> {
                
                
                for channel_index in 0..num_channels {
                    let channel_buffer = asio_channel_slice_mut::<SampleType>(asio_stream, buffer_index, channel_index);
                    for (frame, asio_sample) in buffer.chunks_mut(num_channels).zip(channel_buffer) {
                        *asio_sample = to_endian(from_f32(frame[channel_index]));
                    }
                }
                AudioMut::from_interleaved_mut(buffer, num_channels).unwrap()
            }
            

            let audio_buffer = match &output_data_type {
                asio::AsioSampleType::ASIOSTInt16MSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, to_be, f32_to_i16)
                },
                asio::AsioSampleType::ASIOSTInt16LSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, to_le, f32_to_i16)
                },
                asio::AsioSampleType::ASIOSTInt24MSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, to_be, f32_to_i24)
                },
                asio::AsioSampleType::ASIOSTInt24LSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, to_le, f32_to_i24)
                },
                asio::AsioSampleType::ASIOSTInt32MSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, to_be, f32_to_i32)
                },
                asio::AsioSampleType::ASIOSTInt32LSB => {
                    create_buffer(&asio_stream, &mut buffer, buffer_index, num_channels, to_le, f32_to_i32)
                },
                asio::AsioSampleType::ASIOSTFloat32MSB => {
                    create_buffer::<u32>(&asio_stream, &mut buffer, buffer_index, num_channels, to_be, f32::to_bits)
                },
                asio::AsioSampleType::ASIOSTFloat32LSB => {
                    create_buffer::<u32>(&asio_stream, &mut buffer, buffer_index, num_channels, to_le, f32::to_bits)
                },
                // asio::AsioSampleType::ASIOSTFloat64MSB => todo!(),
                // asio::AsioSampleType::ASIOSTFloat64LSB => todo!(),
                unsupported_format => unreachable!(
                    "returned with unsupported format {:?}",
                    unsupported_format
                ),
            };

            let timestamp = Timestamp {
                samplerate: 41000.0,
                counter: 0,
            };
            let context = AudioCallbackContext {
                stream_config,
                timestamp,
            };
            let output = AudioOutput {
                timestamp,
                buffer: audio_buffer,
            };

            callback.on_output_data(context, output);
        });

        self.driver.start()?;

        Ok(AsioStream::new(playing, callback_id))
    }
}

impl AudioDuplexDevice for AsioDevice {
    fn default_duplex_config(&self) -> Result<StreamConfig, Self::Error> {
        let channels = self.driver.channels()?.ins as u32;
        let samplerate = self.driver.sample_rate()?;
        let (min_buffer_size, max_buffer_size) = self.driver.buffersize_range()?;
        Ok(StreamConfig {
            channels,
            samplerate,
            buffer_size_range: (Some(min_buffer_size as usize), Some(max_buffer_size as usize)),
            exclusive: false,
        })
    }

    fn create_duplex_stream<Callback: SendEverywhereButOnWeb + AudioDuplexCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        todo!()
    }
    
    type StreamHandle<Callback: AudioDuplexCallback> = AsioStream<Callback>;
}

// HELPERS

unsafe fn asio_channel_slice<T>(
    asio_stream: &asio::AsioStream,
    buffer_index: usize,
    channel_index: usize,
) -> &[T] {
    let buffer_size = asio_stream.buffer_size as usize;
    let buff_ptr: *const T =
        asio_stream.buffer_infos[channel_index].buffers[buffer_index as usize] as *const _;
    std::slice::from_raw_parts(buff_ptr, buffer_size)
}

unsafe fn asio_channel_slice_mut<T>(
    asio_stream: &asio::AsioStream,
    buffer_index: usize,
    channel_index: usize,
) -> &mut [T] {
    let buffer_size = asio_stream.buffer_size as usize;
    let buff_ptr: *mut T =
        asio_stream.buffer_infos[channel_index].buffers[buffer_index as usize] as *mut _;
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


// fn asio_ns_to_double(val: sys::bindings::asio_import::ASIOTimeStamp) -> f64 {
//     let two_raised_to_32 = 4294967296.0;
//     val.lo as f64 + val.hi as f64 * two_raised_to_32
// }

// fn system_time_to_timestamp(system_time: asio::AsioTime) -> timestamp::Timestamp {
//     let systime_ns = asio_ns_to_double(system_time);
//     let secs = systime_ns as i64 / 1_000_000_000;
//     let nanos = (systime_ns as i64 - secs * 1_000_000_000) as u32;

// }