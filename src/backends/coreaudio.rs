//! # CoreAudio backend
//!
//! CoreAudio is the audio backend for macOS and iOS devices.

use std::borrow::Cow;
use std::convert::Infallible;

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    audio_unit_from_device_id, find_matching_physical_format, get_audio_device_ids_for_scope,
    get_default_device_id, get_device_name, get_supported_physical_stream_formats,
    set_device_physical_stream_format,
};
use coreaudio::audio_unit::render_callback::{data, Args};
use coreaudio::audio_unit::{AudioUnit, Element, SampleFormat, Scope, StreamFormat};
use coreaudio::sys::{kAudioUnitProperty_StreamFormat, AudioDeviceID};
use thiserror::Error;

use crate::audio_buffer::AudioBuffer;
use crate::channel_map::Bitset;
use crate::timestamp::Timestamp;
use crate::{
    AudioCallbackContext, AudioDevice, AudioDriver, AudioInputCallback, AudioInputDevice,
    AudioOutput, AudioOutputCallback, AudioOutputDevice, AudioStreamHandle, Channel, DeviceType,
    SendEverywhereButOnWeb, StreamConfig,
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
        Ok(Cow::Borrowed("CoreAudio (version unknown)"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        let is_input = matches!(device_type, DeviceType::Input);
        let Some(device_id) = get_default_device_id(is_input) else {
            return Ok(None);
        };
        Ok(Some(CoreAudioDevice {
            device_id,
            device_type,
        }))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        let per_scope = [Scope::Input, Scope::Output]
            .into_iter()
            .map(|scope| {
                let audio_ids = get_audio_device_ids_for_scope(scope)?;
                Ok::<_, CoreAudioError>(
                    audio_ids
                        .into_iter()
                        .map(|id| CoreAudioDevice::from_id(scope, id))
                        .collect::<Result<Vec<_>, _>>()?,
                )
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
    fn from_id(scope: Scope, device_id: AudioDeviceID) -> Result<Self, CoreAudioError> {
        let device_type =
            Self::scope_to_valid_device_type(scope).ok_or(CoreAudioError::InvalidScope(scope))?;
        Ok(Self {
            device_id,
            device_type,
        })
    }

    fn scope_to_valid_device_type(scope: Scope) -> Option<DeviceType> {
        match scope {
            Scope::Input => Some(DeviceType::Input),
            Scope::Output => Some(DeviceType::Output),
            _ => None,
        }
    }
}

impl AudioDevice for CoreAudioDevice {
    type Error = CoreAudioError;

    fn name(&self) -> Cow<str> {
        match get_device_name(self.device_id) {
            Ok(std) => Cow::Owned(std),
            Err(err) => {
                eprintln!("Cannot get audio device name: {err}");
                Cow::Borrowed("<unknown>")
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        let is_input = matches!(self.device_type, DeviceType::Input);
        let channels = match audio_unit_from_device_id(self.device_id, is_input) {
            Err(err) => {
                eprintln!("CoreAudio error getting audio unit: {err}");
                0
            }
            Ok(audio_unit) => {
                let stream_format = if is_input {
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

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        let stream_format = StreamFormat {
            sample_rate: config.samplerate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_NON_INTERLEAVED
                | LinearPcmFlags::IS_PACKED
                | LinearPcmFlags::IS_FLOAT,
            channels: config.channels.count() as u32,
        };
        let Some(asbd) = find_matching_physical_format(self.device_id, stream_format) else {
            return true;
        };
        if let Some(min_size) = config.buffer_size_range.0 {
            if (asbd.mFramesPerPacket as usize) < min_size {
                return false;
            }
        }
        if let Some(max_size) = config.buffer_size_range.1 {
            if (asbd.mFramesPerPacket as usize) > max_size {
                return false;
            }
        }
        return true;
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        const TYPICAL_SAMPLERATES: [f64; 5] = [44100., 48000., 96000., 128000., 192000.];
        let supported_list = get_supported_physical_stream_formats(self.device_id).ok()?;
        Some(supported_list.into_iter().flat_map(|asbd| {
            let samplerate_range = asbd.mSampleRateRange.mMinimum..asbd.mSampleRateRange.mMaximum;
            TYPICAL_SAMPLERATES
                .iter()
                .copied()
                .filter(move |sr| samplerate_range.contains(sr))
                .map(move |sr| {
                    let channels = 1 << asbd.mFormat.mChannelsPerFrame as u32 - 1;
                    let buf_size = asbd.mFormat.mFramesPerPacket as usize;
                    StreamConfig {
                        samplerate: sr,
                        channels,
                        buffer_size_range: (Some(buf_size), Some(buf_size)),
                    }
                })
        }))
    }
}

impl AudioInputDevice for CoreAudioDevice {
    type StreamHandle<Callback: AudioInputCallback> = CoreAudioStream<Callback>;

    fn create_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        todo!()
    }
}

impl AudioOutputDevice for CoreAudioDevice {
    type StreamHandle<Callback: AudioOutputCallback> = CoreAudioStream<Callback>;

    fn create_output_stream<Callback: SendEverywhereButOnWeb + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        CoreAudioStream::new_output(self.device_id, stream_config, callback)
    }
}

pub struct CoreAudioStream<Callback> {
    audio_unit: AudioUnit,
    callback_retrieve: oneshot::Sender<oneshot::Sender<Callback>>,
}

impl<Callback> AudioStreamHandle<Callback> for CoreAudioStream<Callback> {
    type Error = Infallible;

    fn eject(mut self) -> Result<Callback, Self::Error> {
        let (tx, rx) = oneshot::channel();
        self.callback_retrieve.send(tx).unwrap();
        let callback = rx.recv().unwrap();
        self.audio_unit.free_render_callback();
        Ok(callback)
    }
}

impl<Callback: 'static + Send + AudioOutputCallback> CoreAudioStream<Callback> {
    fn new_output(
        device_id: AudioDeviceID,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, CoreAudioError> {
        let mut audio_unit = audio_unit_from_device_id(device_id, false)?;
        let hw_stream_format = StreamFormat {
            sample_rate: stream_config.samplerate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_NON_INTERLEAVED
                | LinearPcmFlags::IS_PACKED
                | LinearPcmFlags::IS_FLOAT,
            channels: stream_config.channels.count() as _,
        };
        let hw_asbd = find_matching_physical_format(device_id, hw_stream_format)
            .ok_or(coreaudio::Error::UnsupportedStreamFormat)?;
        set_device_physical_stream_format(device_id, hw_asbd)?;
        let asbd = hw_stream_format.to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd),
        )?;
        let mut buffer = AudioBuffer::zeroed(
            hw_asbd.mChannelsPerFrame as _,
            stream_config.samplerate as _,
        );

        // Set up the callback retrieval process, without needing to make the callback `Sync`
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
            let output = AudioOutput { buffer: buffer.as_mut(), timestamp };
            if let Some(callback) = &mut callback {
                callback.on_output_data(
                    AudioCallbackContext {
                        stream_config,
                        timestamp,
                    },
                    output,
                );
                for (output, inner) in args.data.channels_mut().zip(buffer.channels()) {
                    output.copy_from_slice(inner.as_slice().unwrap());
                }
            }
            //timestamp += args.num_frames as u64;
            Ok(())
        })?;
        audio_unit.start()?;
        Ok(Self {
            audio_unit,
            callback_retrieve: tx,
        })
    }
}
