//! CoreAudio device implementation
use crate::stream::Handle;
use crate::{utils, Error};
use coreaudio::audio_unit::macos_helpers::{
    get_audio_device_supports_scope, get_default_device_id, get_device_name,
};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, Scope};
use coreaudio_sys::{
    kAudioDevicePropertyBufferFrameSizeRange, kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput,
    kAudioOutputUnitProperty_EnableIO, AudioDeviceID, AudioObjectPropertyAddress, AudioValueRange,
};
use interflow_core::device::StreamConfig;
use interflow_core::traits::{ExtensionProvider, Selector};
use interflow_core::{device, stream, DeviceType};
use std::borrow::Cow;
use std::cell::OnceCell;

/// Audio device request type. CoreAudio has separate code paths for getting the default device, and getting a
/// specific ID. Furthermore, "default device"s are automatically re-routed when the user changes the default device.
#[derive(Debug, Default, Copy, Clone)]
pub enum DeviceRequest {
    /// Automatically connects to the default device output, generic stream.
    #[default]
    DefaultOutput,
    /// Automatically connects to the default device output, notification stream.
    SystemOutput,
    /// Connect to this specific output.
    Specific(AudioDeviceID),
}

impl DeviceRequest {
    pub(crate) fn to_audio_unit(&self) -> Result<AudioUnit, Error> {
        match self {
            Self::DefaultOutput => Ok(AudioUnit::new(IOType::DefaultOutput)?),
            Self::SystemOutput => Ok(AudioUnit::new(IOType::SystemOutput)?),
            &Self::Specific(..) => {
                // TODO: Use provided ID
                let mut unit = AudioUnit::new(IOType::HalOutput)?;
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
    }

    /// Return the name associated with this [`DeviceRequest`]. This is a descriptive name, and does not uniquely
    /// identify the device.
    pub fn name(&self) -> Cow<'_, str> {
        match self {
            DeviceRequest::DefaultOutput => Cow::Borrowed("Default output"),
            DeviceRequest::SystemOutput => Cow::Borrowed("Notifications output"),
            &DeviceRequest::Specific(id) => match get_device_name(id) {
                Ok(s) => Cow::Owned(s),
                Err(err) => {
                    log::error!("Cannot get device name for ID: {}: {err}", id);
                    Cow::Borrowed("<unknown>")
                }
            },
        }
    }

    /// Returns the [`DeviceType`] that best describes this [`DeviceRequest`].
    pub fn device_type(&self) -> DeviceType {
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
        type_
    }

    /// Queries the accepted range of buffer sizes that the device accepts.
    pub fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), Error> {
        match self {
            Self::DefaultOutput | Self::SystemOutput => Ok((None, None)),
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
                let range = utils::get_device_property::<AudioValueRange>(id, address)?;
                Ok((Some(range.mMinimum as usize), Some(range.mMaximum as usize)))
            }
        }
    }
}

/// CoreAudio device type. Contains the requested device (including default output/notification stream) and
/// associated audio unit.
pub struct Device {
    pub(crate) request: DeviceRequest,
    audio_unit: OnceCell<AudioUnit>,
}

impl Device {
    /// Create a new device from a [`DeviceRequest`].
    pub fn new(request: DeviceRequest) -> Self {
        Self {
            request,
            audio_unit: OnceCell::new(),
        }
    }

    /// Create a device from a specific device ID.
    pub fn from_id(id: AudioDeviceID) -> Self {
        Self::new(DeviceRequest::Specific(id))
    }

    /// Create a device following the default output.
    pub fn default_output() -> Self {
        Self::new(DeviceRequest::DefaultOutput)
    }

    /// Consumes this device object, returning the associated Audio Unit.
    pub fn into_audio_unit(mut self) -> Result<AudioUnit, Error> {
        match self.audio_unit.take() {
            Some(unit) => Ok(unit),
            None => self.request.to_audio_unit(),
        }
    }
}

impl ExtensionProvider for Device {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector.register::<dyn CoreAudioDeviceExt>(self)
    }
}

impl device::Device for Device {
    type Error = Error;

    type StreamHandle<Callback: stream::Callback> = Handle<Callback>;

    fn name(&self) -> Cow<'_, str> {
        self.request.name()
    }

    fn device_type(&self) -> DeviceType {
        self.request.device_type()
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
        let buffer_size_range = self.request.buffer_size_range()?;
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
        Handle::new(self, stream_config, callback)
    }
}

/// Extension trait for [`Device`]. Either use directly, or lookup the trait with `device.lookup::<dyn CoreAudioDeviceExt>()`.
pub trait CoreAudioDeviceExt {
    /// Return the audio unit associated with this device. Creates it at most once; subsequent calls return the same
    /// object.
    fn get_audio_unit(&self) -> Result<&AudioUnit, Error>;
}

impl CoreAudioDeviceExt for Device {
    fn get_audio_unit(&self) -> Result<&AudioUnit, Error> {
        if let Some(unit) = self.audio_unit.get() {
            return Ok(unit);
        }
        let unit = self.request.to_audio_unit()?;
        let result = self.audio_unit.set(unit);
        assert!(result.is_ok());
        Ok(self.audio_unit.get().unwrap())
    }
}
