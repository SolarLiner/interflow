//! # Interflow CoreAudio
//!
//! Interflow backend using CoreAudio for macOS and iOS applications
#![warn(missing_docs)]
use std::borrow::Cow;
use std::convert::Infallible;
use std::num::NonZeroUsize;

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    get_audio_device_ids, get_audio_device_supports_scope, get_default_device_id, get_device_name,
};
use coreaudio::audio_unit::render_callback::{data, Args};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat};
use coreaudio_sys::{
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyBufferFrameSizeRange,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeOutput, kAudioOutputUnitProperty_EnableIO,
    kAudioUnitProperty_StreamFormat, AudioDeviceID, AudioObjectGetPropertyData,
    AudioObjectPropertyAddress, AudioValueRange,
};
use interflow_core::{
    buffer::AudioBuffer,
    device::{self, Device as _, ResolvedStreamConfig, StreamConfig},
    platform, stream,
    stream::{AudioInput, AudioOutput, CallbackContext, StreamProxy},
    timing::Timestamp,
    traits::{ExtensionProvider, Selector},
    DeviceType,
};

fn get_device_property<T>(
    device_id: AudioDeviceID,
    address: AudioObjectPropertyAddress,
) -> Result<T, coreaudio::Error> {
    let mut data = std::mem::MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
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
    let size = size_of::<T>() as u32;
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

/// Type of errors from the CoreAudio backend
#[derive(Debug, thiserror::Error)]
#[error("CoreAudio error: ")]
pub enum Error {
    /// Error originating from CoreAudio
    #[error(transparent)]
    Backend(#[from] coreaudio::Error),
    /// The scope given to an audio device is invalid.
    #[error("Invalid scope {0:?}")]
    InvalidScope(Scope),
    #[error("No matching devices for type: {0:?}")]
    NoMatchingDevices(DeviceType),
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

/// The CoreAudio driver.
pub struct Platform;

impl ExtensionProvider for Platform {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl platform::Platform for Platform {
    type Error = Error;

    type Device = Device;

    const NAME: &'static str = "CoreAudio";

    fn default_device(&self, device_type: DeviceType) -> Result<Self::Device, Self::Error> {
        if device_type.is_output() || !device_type.is_input() {
            return Ok(Device::DefaultOutput);
        }

        let Some(id) = get_default_device_id(device_type.is_input()) else {
            return Err(Error::NoMatchingDevices(device_type));
        };
        Ok(Device::Specific(id))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        Ok(get_audio_device_ids()?.into_iter().map(Device::Specific))
    }
}

/// CoreAudio device handle
#[derive(Debug, Copy, Clone)]
pub enum Device {
    /// Automatically connects to the default device output, generic stream.
    DefaultOutput,
    /// Automatically connects to the default device output, notification stream.
    SystemOutput,
    /// Connect to this specific output.
    Specific(AudioDeviceID),
}

impl Device {
    pub fn get_audio_unit(&self) -> Result<AudioUnit, Error> {
        let io_type = match self {
            Self::DefaultOutput => IOType::DefaultOutput,
            Self::SystemOutput => IOType::SystemOutput,
            Self::Specific(..) => IOType::HalOutput,
        };
        let mut unit = AudioUnit::new(io_type)?;
        let device_type = self.device_type();
        let value = if device_type.is_input() { 1u32 } else { 0 };
        unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&value),
        )?;
        let value = if device_type.is_output() { 1u32 } else { 0 };
        unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&value),
        )?;
        Ok(unit)
    }
}

impl ExtensionProvider for Device {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl device::Device for Device {
    type Error = Error;

    type StreamHandle<Callback: stream::Callback> = StreamHandle<Callback>;

    fn name(&self) -> Cow<'_, str> {
        match self {
            Self::DefaultOutput => Cow::Borrowed("Default output"),
            Self::SystemOutput => Cow::Borrowed("Notifications output"),
            &Self::Specific(id) => match get_device_name(id) {
                Ok(s) => Cow::Owned(s),
                Err(err) => {
                    log::error!("Cannot get device name for ID: {}: {err}", id);
                    Cow::Borrowed("<unknown>")
                }
            },
        }
    }

    fn device_type(&self) -> DeviceType {
        let log_error = |result: Result<bool, coreaudio::Error>| {
            result
                .inspect_err(|err| {
                    log::error!("Cannot get device type for {:?}: {err}", self.name());
                })
                .unwrap_or(false)
        };
        let supports_scope = |scope| match self {
            Self::DefaultOutput | Self::SystemOutput if matches!(scope, Scope::Output) => true,
            &Self::Specific(id) => log_error(get_audio_device_supports_scope(id, scope)),
            _ => false,
        };
        let is_default = match self {
            Self::DefaultOutput | Self::SystemOutput => true,
            &Self::Specific(id) => {
                get_default_device_id(true) == Some(id) || get_default_device_id(false) == Some(id)
            }
        };

        let mut type_ = DeviceType::empty();
        type_.set(DeviceType::INPUT, supports_scope(Scope::Input));
        type_.set(DeviceType::OUTPUT, supports_scope(Scope::Output));
        type_.set(DeviceType::DEFAULT, is_default);
        return type_;
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        let au = self.get_audio_unit()?;
        let input_channels = au
            .input_stream_format()
            .map(|fmt| fmt.channels as usize)
            .unwrap_or(0);
        let output_channels = au
            .output_stream_format()
            .map(|fmt| fmt.channels as usize)
            .unwrap_or(0);
        let buffer_size_range = {
            match self {
                Self::DefaultOutput | Self::SystemOutput => (None, None),
                &Self::Specific(id) => {
                    let address = AudioObjectPropertyAddress {
                        mSelector: kAudioDevicePropertyBufferFrameSizeRange,
                        mScope: if self.device_type().is_input() {
                            kAudioObjectPropertyScopeInput
                        } else {
                            kAudioObjectPropertyScopeOutput
                        },
                        mElement: kAudioObjectPropertyElementMaster,
                    };
                    let range = get_device_property::<AudioValueRange>(id, address)?;
                    (Some(range.mMinimum as usize), Some(range.mMaximum as usize))
                }
            }
        };
        Ok(StreamConfig {
            sample_rate: au.sample_rate()?,
            input_channels,
            output_channels,
            buffer_size_range,
            exclusive: false,
        })
    }

    fn is_config_supported(&self, _config: &StreamConfig) -> bool {
        true
    }

    fn create_stream<Callback: 'static + Send + stream::Callback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        StreamHandle::new(self, stream_config, callback)
    }
}

/// Stream type created by opening up a stream on a [`Device`].
pub struct StreamHandle<Callback> {
    audio_unit: AudioUnit,
    callback_retrieve: oneshot::Sender<oneshot::Sender<Callback>>,
}

impl<Callback> stream::StreamHandle<Callback> for StreamHandle<Callback> {
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

struct DummyStreamProxy;

impl ExtensionProvider for DummyStreamProxy {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl StreamProxy for DummyStreamProxy {}

static STREAM_PROXY: DummyStreamProxy = DummyStreamProxy;

impl<Callback: 'static + Send + stream::Callback> StreamHandle<Callback> {
    fn new(
        device: &Device,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, Error> {
        let requested_type = stream_config.requested_device_type();
        assert!(
            !requested_type.is_duplex(),
            "CoreAudio does not support native duplex mode"
        );

        let unsupported = device.device_type() & !requested_type;
        if !unsupported.is_empty() {
            log::warn!(
                "Cannot request {unsupported:?} for {device}, ignoring",
                device = device.name()
            );
        }
        let unit = device.get_audio_unit()?;
        if requested_type.is_input() {
            Self::new_input(unit, stream_config, callback)
        } else {
            Self::new_output(unit, stream_config, callback)
        }
    }

    fn new_input(
        mut audio_unit: AudioUnit,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Result<Self, Error> {
        let asbd =
            input_stream_format(stream_config.sample_rate, stream_config.input_channels).to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&asbd),
        )?;
        let frame_count: u32 = audio_unit.get_property(
            kAudioDevicePropertyBufferFrameSize,
            Scope::Input,
            Element::Input,
        )?;
        let resolved_config = ResolvedStreamConfig {
            sample_rate: asbd.mSampleRate,
            input_channels: asbd.mChannelsPerFrame as _,
            output_channels: 0,
            max_frame_count: frame_count as _,
        };
        let mut buffer = AudioBuffer::zeroed(
            NonZeroUsize::new(frame_count as _).unwrap(),
            NonZeroUsize::new(asbd.mChannelsPerFrame as _).unwrap(),
        );

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        callback.prepare(CallbackContext {
            stream_config: &resolved_config,
            timestamp: Timestamp::new(asbd.mSampleRate),
            stream_proxy: &STREAM_PROXY,
        });
        let mut callback = Some(callback);

        audio_unit.set_input_callback(move |args: Args<data::Interleaved<i16>>| {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return Err(());
            }
            let num_frames = args.num_frames;
            let channels = asbd.mChannelsPerFrame as usize;
            let input_data = args.data.buffer;

            for frame_idx in 0..num_frames {
                for ch in 0..channels {
                    let sample = input_data[frame_idx * channels + ch];
                    buffer
                        .frame_mut(frame_idx)
                        .set(ch, (sample as f32) / (i16::MAX as f32));
                }
            }

            let timestamp = Timestamp::from_count(
                resolved_config.sample_rate,
                args.time_stamp.mSampleTime as _,
            );

            let input = AudioInput {
                buffer: buffer.as_ref(),
                timestamp,
                channel_flags: &[],
            };

            let mut dummy_buf = AudioBuffer::zeroed(
                NonZeroUsize::new(1).unwrap(),
                NonZeroUsize::new(channels as _).unwrap(),
            );
            let dummy_output = AudioOutput {
                buffer: dummy_buf.as_mut(),
                timestamp: Timestamp::new(asbd.mSampleRate),
                channel_flags: &[],
            };

            if let Some(callback) = &mut callback {
                callback.process_audio(
                    CallbackContext {
                        stream_config: &resolved_config,
                        timestamp,
                        stream_proxy: &STREAM_PROXY,
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
        mut audio_unit: AudioUnit,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Result<Self, Error> {
        let asbd = output_stream_format(stream_config.sample_rate, stream_config.output_channels)
            .to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd),
        )?;
        let frame_size: u32 = audio_unit.get_property(
            kAudioDevicePropertyBufferFrameSize,
            Scope::Output,
            Element::Output,
        )?;
        let resolved_config = ResolvedStreamConfig {
            sample_rate: asbd.mSampleRate,
            input_channels: 0,
            output_channels: asbd.mChannelsPerFrame as _,
            max_frame_count: frame_size as _,
        };
        let mut buffer = AudioBuffer::zeroed(
            NonZeroUsize::new(frame_size as _).unwrap(),
            NonZeroUsize::new(asbd.mChannelsPerFrame as _).unwrap(),
        );

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        callback.prepare(CallbackContext {
            stream_config: &resolved_config,
            timestamp: Timestamp::new(resolved_config.sample_rate),
            stream_proxy: &STREAM_PROXY,
        });
        let mut callback = Some(callback);

        audio_unit.set_render_callback(move |mut args: Args<data::NonInterleaved<f32>>| {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return Err(());
            }
            let timestamp = Timestamp::from_count(
                resolved_config.sample_rate,
                args.time_stamp.mSampleTime as _,
            );

            let dummy_buf = AudioBuffer::zeroed(
                NonZeroUsize::new(1).unwrap(),
                NonZeroUsize::new(asbd.mChannelsPerFrame as _).unwrap(),
            );
            let dummy_input = AudioInput {
                buffer: dummy_buf.as_ref(),
                timestamp: Timestamp::new(resolved_config.sample_rate),
                channel_flags: &[],
            };

            let output = AudioOutput {
                buffer: buffer.as_mut(),
                timestamp,
                channel_flags: &[],
            };

            if let Some(callback) = &mut callback {
                callback.process_audio(
                    CallbackContext {
                        stream_config: &resolved_config,
                        timestamp,
                        stream_proxy: &STREAM_PROXY,
                    },
                    dummy_input,
                    output,
                );
                for (out_ch, in_ch) in args.data.channels_mut().zip(buffer.iter_channels()) {
                    out_ch.copy_from_slice(in_ch);
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
    use coreaudio_sys::{kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeOutput};
    use interflow_core::platform::Platform as _;

    #[test]
    fn test_set_device_buffersize() {
        let platform = Platform;
        let Some(Device::Specific(device_id)) = platform
            .list_devices()
            .unwrap()
            .into_iter()
            .find(|d| d.device_type().is_output())
        else {
            println!("Skipping test: No specific output device found.");
            return;
        };

        let buffer_size = 256u32;

        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyBufferFrameSize,
            mScope: kAudioObjectPropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMaster,
        };
        set_device_property(device_id, property_address, &buffer_size).unwrap();

        let actual_buffer_size: u32 = get_device_property(device_id, property_address).unwrap();

        assert_eq!(buffer_size, actual_buffer_size);
    }
}
