//! # CoreAudio backend
//! 
//! CoreAudio is the audio backend for macOS and iOS devices.

use std::borrow::Cow;

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    audio_unit_from_device_id, find_matching_physical_format, get_audio_device_ids_for_scope,
    get_default_device_id, get_device_name, get_supported_physical_stream_formats,
};
use coreaudio::audio_unit::{AudioUnit, SampleFormat, Scope, StreamFormat};
use coreaudio::sys::AudioDeviceID;
use thiserror::Error;

use crate::channel_map::Bitset;
use crate::{AudioDevice, AudioDriver, Channel, DeviceType, StreamConfig};

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
        let audio_unit = audio_unit_from_device_id(device_id, is_input)?;
        Ok(Some(CoreAudioDevice {
            device_id,
            audio_unit,
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
pub struct CoreAudioDevice {
    device_id: AudioDeviceID,
    audio_unit: AudioUnit,
    device_type: DeviceType,
}

impl CoreAudioDevice {
    fn from_id(scope: Scope, device_id: AudioDeviceID) -> Result<Self, CoreAudioError> {
        let device_type =
            Self::scope_to_valid_device_type(scope).ok_or(CoreAudioError::InvalidScope(scope))?;
        let is_input = matches!(scope, Scope::Input);
        let audio_unit = audio_unit_from_device_id(device_id, is_input)?;
        Ok(Self {
            device_id,
            audio_unit,
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
        let channels = match self.device_type {
            DeviceType::Input => self.audio_unit.input_stream_format().unwrap().channels as usize,
            DeviceType::Output => self.audio_unit.output_stream_format().unwrap().channels as usize,
            _ => 0,
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
            return false;
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
