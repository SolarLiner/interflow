//! # CoreAudio backend
//!
//! CoreAudio is the audio backend for macOS and iOS devices.

use std::borrow::Cow;
use std::convert::Infallible;

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    audio_unit_from_device_id, get_audio_device_ids_for_scope, get_default_device_id,
    get_device_name, get_supported_physical_stream_formats,
};
use coreaudio::audio_unit::render_callback::{data, Args};
use coreaudio::audio_unit::{AudioUnit, Element, SampleFormat, Scope, StreamFormat};
use coreaudio_sys::{
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyBufferFrameSizeRange,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeOutput, kAudioUnitProperty_MaximumFramesPerSlice,
    kAudioUnitProperty_SampleRate, kAudioUnitProperty_StreamFormat, AudioDeviceID,
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, AudioValueRange,
};

fn get_device_property<T>(
    device_id: AudioDeviceID,
    address: AudioObjectPropertyAddress,
) -> Result<T, coreaudio::Error> {
    let mut data = std::mem::MaybeUninit::<T>::uninit();
    let mut size = std::mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            data.as_mut_ptr() as *mut _,
        )
    };
    coreaudio::Error::from_os_status(status)?;
    Ok(unsafe { data.assume_init() })
}

fn set_device_property<T>(
    device_id: AudioDeviceID,
    address: AudioObjectPropertyAddress,
    data: &T,
) -> Result<(), coreaudio::Error> {
    let size = std::mem::size_of::<T>() as u32;
    let status = unsafe {
        coreaudio_sys::AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            size,
            data as *const T as *const _,
        )
    };
    coreaudio::Error::from_os_status(status)
}
use thiserror::Error;

use crate::audio_buffer::{AudioBuffer, Sample};
use crate::channel_map::Bitset;
use crate::prelude::{AudioMut, AudioRef, ChannelMap32};
use crate::timestamp::Timestamp;
use crate::{
    AudioCallback, AudioCallbackContext, AudioDevice, AudioDriver, AudioInput, AudioOutput,
    AudioStreamHandle, Channel, DeviceType, ResolvedStreamConfig, SendEverywhereButOnWeb,
    StreamConfig,
};

/// Type of errors from the CoreAudio backend
#[derive(Debug, Error)]
#[error("CoreAudio error:")]
pub enum CoreAudioError {
    /// Error originating from CoreAudio
    #[error("{0}")]
    BackendError(#[from] coreaudio::Error),
    /// The scope given to an audio device is invalid.
    #[error("Invalid scope {0:?}")]
    InvalidScope(Scope),
}

/// The CoreAudio driver.
#[derive(Debug, Copy, Clone)]
pub struct CoreAudioDriver;

impl AudioDriver for CoreAudioDriver {
    type Error = CoreAudioError;
    type Device = CoreAudioDevice;
    const DISPLAY_NAME: &'static str = "CoreAudio";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        Ok(Cow::Borrowed("unknown"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        let Some(device_id) = get_default_device_id(device_type.is_input()) else {
            return Ok(None);
        };
        Ok(Some(CoreAudioDevice::from_id(
            device_id,
            device_type.is_input(),
        )?))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        let per_scope = [Scope::Input, Scope::Output]
            .into_iter()
            .map(|scope| {
                let audio_ids = get_audio_device_ids_for_scope(scope)?;
                audio_ids
                    .into_iter()
                    .map(|id| CoreAudioDevice::from_id(id, matches!(scope, Scope::Input)))
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(per_scope.into_iter().flatten())
    }
}

/// Type of devices available from the CoreAudio driver.
#[derive(Debug, Clone, Copy)]
pub struct CoreAudioDevice {
    device_id: AudioDeviceID,
    device_type: DeviceType,
}

impl CoreAudioDevice {
    fn from_id(device_id: AudioDeviceID, is_input: bool) -> Result<Self, CoreAudioError> {
        let is_output = !is_input; // TODO: Interact with CoreAudio directly to be able to work with duplex devices
        let is_default = get_default_device_id(true) == Some(device_id)
            || get_default_device_id(false) == Some(device_id);
        let mut device_type = DeviceType::empty();
        device_type.set(DeviceType::INPUT, is_input);
        device_type.set(DeviceType::OUTPUT, is_output);
        device_type.set(DeviceType::DEFAULT, is_default);
        Ok(Self {
            device_id,
            device_type,
        })
    }

    /// Sets the device's buffer size if requested in the `StreamConfig`.
    /// This must be done before creating the AudioUnit.
    fn set_buffer_size_from_config(
        &self,
        stream_config: &StreamConfig,
    ) -> Result<(), CoreAudioError> {
        if let (Some(min), Some(max)) = stream_config.buffer_size_range {
            if min == max {
                let property_address = AudioObjectPropertyAddress {
                    mSelector: kAudioDevicePropertyBufferFrameSize,
                    mScope: if self.device_type.is_input() {
                        kAudioObjectPropertyScopeInput
                    } else {
                        kAudioObjectPropertyScopeOutput
                    },
                    mElement: kAudioObjectPropertyElementMaster,
                };
                set_device_property(self.device_id, property_address, &(min as u32))?;
            }
        }
        Ok(())
    }
}

impl AudioDevice for CoreAudioDevice {
    type StreamHandle<Callback: AudioCallback> = CoreAudioStream<Callback>;
    type Error = CoreAudioError;

    fn name(&self) -> Cow<str> {
        match get_device_name(self.device_id) {
            Ok(std) => Cow::Owned(std),
            Err(err) => {
                log::error!("Cannot get audio device name: {err}");
                Cow::Borrowed("<unknown>")
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        let channels = match audio_unit_from_device_id(self.device_id, self.device_type.is_input())
        {
            Err(err) => {
                eprintln!("CoreAudio error getting audio unit: {err}");
                0
            }
            Ok(audio_unit) => {
                let stream_format = if self.device_type.is_input() {
                    audio_unit.input_stream_format().unwrap()
                } else {
                    audio_unit.output_stream_format().unwrap()
                };
                stream_format.channels as usize
            }
        };
        (0..channels).map(|ch| Channel {
            index: ch,
            name: Cow::Owned(format!("Channel {}", ch)),
        })
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_unit = audio_unit_from_device_id(self.device_id, self.device_type.is_input())?;
        let format = if self.device_type.is_input() {
            audio_unit.input_stream_format()?
        } else {
            audio_unit.output_stream_format()?
        };

        Ok(StreamConfig {
            samplerate: audio_unit.sample_rate()?,
            input_channels: if self.device_type.is_input() {
                format.channels as _
            } else {
                0
            },
            output_channels: if self.device_type.is_output() {
                format.channels as _
            } else {
                0
            },
            buffer_size_range: (None, None),
            exclusive: false,
        })
    }

    fn is_config_supported(&self, _config: &StreamConfig) -> bool {
        true
    }

    /// Returns the supported I/O buffer size range for the device.
    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), CoreAudioError> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyBufferFrameSizeRange,
            mScope: if self.device_type.is_input() {
                kAudioObjectPropertyScopeInput
            } else {
                kAudioObjectPropertyScopeOutput
            },
            mElement: kAudioObjectPropertyElementMaster,
        };

        let range: AudioValueRange = get_device_property(self.device_id, property_address)?;

        Ok((Some(range.mMinimum as usize), Some(range.mMaximum as usize)))
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        const TYPICAL_SAMPLERATES: [f64; 5] = [44100., 48000., 96000., 128000., 192000.];
        let supported_list = get_supported_physical_stream_formats(self.device_id)
            .inspect_err(|err| eprintln!("Error getting stream formats: {err}"))
            .ok()?;
        let device_type = self.device_type;
        Some(supported_list.into_iter().flat_map(move |asbd| {
            let samplerate_range = asbd.mSampleRateRange.mMinimum..asbd.mSampleRateRange.mMaximum;
            TYPICAL_SAMPLERATES
                .iter()
                .copied()
                .filter(move |sr| samplerate_range.contains(sr))
                .flat_map(move |sr| {
                    [false, true]
                        .into_iter()
                        .map(move |exclusive| (sr, exclusive))
                })
                .map(move |(samplerate, exclusive)| {
                    let channels = asbd.mFormat.mChannelsPerFrame;
                    let input_channels = if device_type.is_input() {
                        channels as _
                    } else {
                        0
                    };
                    let output_channels = if device_type.is_output() {
                        channels as _
                    } else {
                        0
                    };
                    StreamConfig {
                        samplerate,
                        input_channels,
                        output_channels,
                        buffer_size_range,
                        exclusive,
                    }
                })
        }))
    }

    fn create_stream<Callback: SendEverywhereButOnWeb + AudioCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        let mut device = *self;
        device.device_type = DeviceType::INPUT;
        device.set_buffer_size_from_config(&stream_config)?;
        CoreAudioStream::new(self.device_id, self.device_type, stream_config, callback)
    }
}

fn input_stream_format(sample_rate: f64, channel_count: usize) -> StreamFormat {
    StreamFormat {
        sample_rate,
        sample_format: SampleFormat::I16,
        flags: LinearPcmFlags::IS_SIGNED_INTEGER,
        channels: channel_count as _,
    }
}

fn output_stream_format(sample_rate: f64, channel_count: usize) -> StreamFormat {
    StreamFormat {
        sample_rate,
        sample_format: SampleFormat::F32,
        flags: LinearPcmFlags::IS_NON_INTERLEAVED | LinearPcmFlags::IS_FLOAT,
        channels: channel_count as _,
    }
}

/// Stream type created by opening up a stream on a [`CoreAudioDevice`].
pub struct CoreAudioStream<Callback> {
    audio_unit: AudioUnit,
    callback_retrieve: oneshot::Sender<oneshot::Sender<Callback>>,
}

impl<Callback> AudioStreamHandle<Callback> for CoreAudioStream<Callback> {
    type Error = Infallible;

    fn eject(mut self) -> Result<Callback, Self::Error> {
        let (tx, rx) = oneshot::channel();
        self.callback_retrieve
            .send(tx)
            .expect("Callback receiver cannot have been dropped yet");
        let callback = rx.recv().expect("Oneshot receiver must be used");
        self.audio_unit.free_input_callback();
        self.audio_unit.free_render_callback();
        Ok(callback)
    }
}

impl<Callback: 'static + Send + AudioCallback> CoreAudioStream<Callback> {
    fn new(
        device_id: AudioDeviceID,
        device_type: DeviceType,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, CoreAudioError> {
        if device_type.is_input() && !device_type.is_output() {
            Self::new_input(device_id, stream_config, callback)
        } else {
            Self::new_output(device_id, stream_config, callback)
        }
    }

    fn new_input(
        device_id: AudioDeviceID,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, CoreAudioError> {
        let mut audio_unit = audio_unit_from_device_id(device_id, true)?;
        let asbd =
            input_stream_format(stream_config.samplerate, stream_config.input_channels).to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&asbd),
        )?;
        let stream_config = ResolvedStreamConfig {
            samplerate: asbd.mSampleRate,
            input_channels: asbd.mChannelsPerFrame as _,
            output_channels: 0,
            max_frame_count: asbd.mFramesPerPacket as _,
        };
        let mut buffer =
            AudioBuffer::zeroed(asbd.mChannelsPerFrame as _, stream_config.samplerate as _);

        // Set up the callback retrieval process without needing to make the callback `Sync`
        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        let mut callback = Some(callback);
        audio_unit.set_input_callback(move |args: Args<data::Interleaved<i16>>| {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return Err(());
            }
            let mut buffer = buffer.slice_mut(..args.num_frames);
            for (out, inp) in buffer
                .as_interleaved_mut()
                .iter_mut()
                .zip(args.data.buffer.iter())
            {
                *out = inp.into_float();
            }
            let timestamp =
                Timestamp::from_count(stream_config.samplerate, args.time_stamp.mSampleTime as _);
            let input = AudioInput {
                buffer: buffer.as_ref(),
                timestamp,
            };
            let dummy_output = AudioOutput {
                buffer: AudioMut::empty(),
                timestamp: Timestamp::new(asbd.mSampleRate),
            };
            if let Some(callback) = &mut callback {
                callback.process_audio(
                    AudioCallbackContext {
                        stream_config,
                        timestamp,
                    },
                    input,
                    dummy_output,
                );
            }
            Ok(())
        })?;
        audio_unit.start()?;
        Ok(Self {
            audio_unit,
            callback_retrieve: tx,
        })
    }

    fn new_output(
        device_id: AudioDeviceID,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, CoreAudioError> {
        let mut audio_unit = audio_unit_from_device_id(device_id, false)?;
        let asbd =
            output_stream_format(stream_config.samplerate, stream_config.output_channels).to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd),
        )?;
        let stream_config = ResolvedStreamConfig {
            samplerate: asbd.mSampleRate,
            input_channels: 0,
            output_channels: asbd.mChannelsPerFrame as _,
            max_frame_count: asbd.mFramesPerPacket as _,
        };
        let mut buffer =
            AudioBuffer::zeroed(stream_config.output_channels, stream_config.samplerate as _);
        // Set up the callback retrieval process without needing to make the callback `Sync`
        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        let mut callback = Some(callback);
        audio_unit.set_render_callback(move |mut args: Args<data::NonInterleaved<f32>>| {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return Err(());
            }
            let mut buffer = buffer.slice_mut(..args.num_frames);
            let timestamp =
                Timestamp::from_count(stream_config.samplerate, args.time_stamp.mSampleTime as _);
            let dummy_input = AudioInput {
                buffer: AudioRef::empty(),
                timestamp: Timestamp::new(asbd.mSampleRate),
            };
            let output = AudioOutput {
                buffer: buffer.as_mut(),
                timestamp,
            };

            if let Some(callback) = &mut callback {
                callback.process_audio(
                    AudioCallbackContext {
                        stream_config,
                        timestamp,
                    },
                    dummy_input,
                    output,
                );
                for (output, inner) in args.data.channels_mut().zip(buffer.channels()) {
                    output.copy_from_slice(inner.as_slice().unwrap());
                }
            }
            Ok(())
        })?;
        audio_unit.start()?;
        Ok(Self {
            audio_unit,
            callback_retrieve: tx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_device_buffersize() {
        let driver = CoreAudioDriver;
        if let Ok(Some(device)) = driver.default_device(DeviceType::OUTPUT) {
            let buffer_size = 256;

            // Set the buffer size on the device.
            let property_address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyBufferFrameSize,
                mScope: kAudioObjectPropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMaster,
            };
            set_device_property(device.device_id, property_address, &buffer_size).unwrap();

            // Read it back to confirm.
            let actual_buffer_size: u32 =
                get_device_property(device.device_id, property_address).unwrap();

            assert_eq!(buffer_size, actual_buffer_size);
        } else {
            println!("Skipping test: No default output device found.");
        }
    }
}
