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
use coreaudio::sys::{
    kAudioUnitProperty_SampleRate, kAudioUnitProperty_StreamFormat, AudioDeviceID,
};
use thiserror::Error;

use crate::audio_buffer::{AudioBuffer, Sample};
use crate::channel_map::Bitset;
use crate::prelude::ChannelMap32;
use crate::timestamp::Timestamp;
use crate::{
    AudioCallbackContext, AudioDevice, AudioDriver, AudioInput, AudioInputCallback,
    AudioInputDevice, AudioOutput, AudioOutputCallback, AudioOutputDevice, AudioStreamHandle,
    Channel, DeviceType, SendEverywhereButOnWeb, StreamConfig,
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
                audio_ids
                    .into_iter()
                    .map(|id| CoreAudioDevice::from_id(scope, id))
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

    fn is_config_supported(&self, _config: &StreamConfig) -> bool {
        true
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        const TYPICAL_SAMPLERATES: [f64; 5] = [44100., 48000., 96000., 128000., 192000.];
        let supported_list = get_supported_physical_stream_formats(self.device_id)
            .inspect_err(|err| eprintln!("Error getting stream formats: {err}"))
            .ok()?;
        Some(supported_list.into_iter().flat_map(|asbd| {
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
                    let channels = 1 << (asbd.mFormat.mChannelsPerFrame - 1);
                    StreamConfig {
                        samplerate,
                        channels,
                        buffer_size_range: (None, None),
                        exclusive,
                    }
                })
        }))
    }
}

fn input_stream_format(sample_rate: f64, channels: ChannelMap32) -> StreamFormat {
    StreamFormat {
        sample_rate,
        sample_format: SampleFormat::I16,
        flags: LinearPcmFlags::IS_SIGNED_INTEGER,
        channels: channels.count() as _,
    }
}

impl AudioInputDevice for CoreAudioDevice {
    type StreamHandle<Callback: AudioInputCallback> = CoreAudioStream<Callback>;

    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_unit = audio_unit_from_device_id(self.device_id, true)?;
        let samplerate = audio_unit.get_property::<f64>(
            kAudioUnitProperty_SampleRate,
            Scope::Input,
            Element::Input,
        )?;
        Ok(StreamConfig {
            channels: 0b11,
            samplerate,
            buffer_size_range: (None, None),
            exclusive: false,
        })
    }

    fn create_input_stream<Callback: SendEverywhereButOnWeb + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        CoreAudioStream::new_input(self.device_id, stream_config, callback)
    }
}

fn output_stream_format(sample_rate: f64, channels: ChannelMap32) -> StreamFormat {
    StreamFormat {
        sample_rate,
        sample_format: SampleFormat::F32,
        flags: LinearPcmFlags::IS_NON_INTERLEAVED | LinearPcmFlags::IS_FLOAT,
        channels,
    }
}

impl AudioOutputDevice for CoreAudioDevice {
    type StreamHandle<Callback: AudioOutputCallback> = CoreAudioStream<Callback>;

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_unit = audio_unit_from_device_id(self.device_id, false)?;
        let samplerate = audio_unit.sample_rate()?;
        Ok(StreamConfig {
            samplerate,
            buffer_size_range: (None, None),
            channels: 0b11,
            exclusive: false,
        })
    }

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
        self.audio_unit.free_input_callback();
        self.audio_unit.free_render_callback();
        Ok(callback)
    }
}

impl<Callback: 'static + Send + AudioInputCallback> CoreAudioStream<Callback> {
    fn new_input(
        device_id: AudioDeviceID,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, CoreAudioError> {
        let mut audio_unit = audio_unit_from_device_id(device_id, true)?;
        let asbd = input_stream_format(stream_config.samplerate, stream_config.channels).to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&asbd),
        )?;
        let mut buffer = AudioBuffer::zeroed(
            stream_config.channels.count(),
            stream_config.samplerate as _,
        );

        // Set up the callback retrieval process, without needing to make the callback `Sync`
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
            if let Some(callback) = &mut callback {
                callback.on_input_data(
                    AudioCallbackContext {
                        stream_config,
                        timestamp,
                    },
                    input,
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
}

impl<Callback: 'static + Send + AudioOutputCallback> CoreAudioStream<Callback> {
    fn new_output(
        device_id: AudioDeviceID,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, CoreAudioError> {
        let mut audio_unit = audio_unit_from_device_id(device_id, false)?;
        let asbd = output_stream_format(stream_config.samplerate, stream_config.channels).to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd),
        )?;
        let mut buffer = AudioBuffer::zeroed(
            stream_config.channels.count(),
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
            let output = AudioOutput {
                buffer: buffer.as_mut(),
                timestamp,
            };
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
            Ok(())
        })?;
        audio_unit.start()?;
        Ok(Self {
            audio_unit,
            callback_retrieve: tx,
        })
    }
}
