#![allow(missing_docs)]

use crate::{
    duplex::AudioDuplexCallback, AudioDevice, AudioDriver, AudioInputDevice, AudioOutputDevice, AudioStreamHandle, DeviceType, SendEverywhereButOnWeb, StreamConfig
};
use std::{any::Any, borrow::Cow, marker::PhantomData};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E=Error> = std::result::Result<T, E>;

pub type Driver = Box<dyn RawAudioDriver>;
pub type Device = Box<dyn RawAudioDevice>;

/// Dyn-safe version of the `AudioDriver` trait.
pub trait RawAudioDriver: SendEverywhereButOnWeb + Sync {
    /// Driver display name.
    fn display_name(&self) -> &'static str;

    /// Runtime version of the audio driver.
    fn version(&self) -> Result<Cow<str>>;

    /// Default device of the given type.
    fn default_device(&self, device_type: DeviceType) -> Result<Option<Device>>;

    /// List all devices available through this audio driver.
    fn list_devices(&self) -> Result<Vec<Device>>;
}

/// Dyn-safe version of the `AudioDevice` trait.
pub trait RawAudioDevice: SendEverywhereButOnWeb + Sync {
    /// Device display name
    fn name(&self) -> Cow<str>;

    /// Device type. Either input, output, or duplex.
    fn device_type(&self) -> DeviceType;

    /// Iterator of the available channels in this device.
    fn channel_map(&self) -> Vec<crate::Channel>;

    /// Test a configuration to see if it can be used in an audio stream.
    fn is_config_supported(&self, config: &StreamConfig) -> bool;

    /// Enumerate all possible configurations this device supports.
    fn enumerate_configurations(&self) -> Vec<StreamConfig>;
}

pub trait DynAudioCallback: Any + AudioDuplexCallback + SendEverywhereButOnWeb {}

impl<T: Any + SendEverywhereButOnWeb + AudioDuplexCallback> DynAudioCallback for T {}

#[repr(transparent)]
pub struct RawCallback {
    data: Option<Box<dyn DynAudioCallback>>,
}

pub struct StreamHandle<T: AudioDuplexCallback> {
    callback: Option<RawCallback>,
    __callback: PhantomData<fn(T) -> T>,
}

impl<T: AudioDuplexCallback> StreamHandle<T> {
    pub fn eject(self) -> Result<T> {
        let callback = self.callback.unwrap();
        let data = callback.data.unwrap();
        data.downcast::<T>().unwrap().into()
    }
}

/// Implement `RawAudioDriver` for any type that implements `AudioDriver`.
impl<T: SendEverywhereButOnWeb + Sync + AudioDriver<Error: 'static + SendEverywhereButOnWeb + Sync>> RawAudioDriver for T {
    fn display_name(&self) -> &'static str {
        T::DISPLAY_NAME
    }

    fn version(&self) -> Result<Cow<str>> {
        Ok(AudioDriver::version(self)?)
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Device>> {
        Ok(AudioDriver::default_device(self, device_type)?.map(|device| Device::new(device)))
    }

    fn list_devices(&self) -> Result<Vec<Device>> {
        let devices = AudioDriver::list_devices(self)?.into_iter().map(|device| Device::new(device));
        Ok(devices.collect())
    }
}

/// Implement `RawAudioDevice` for any type that implements `AudioDevice`.
impl<T: SendEverywhereButOnWeb + Sync + AudioDevice<Error: 'static + SendEverywhereButOnWeb + Sync>> RawAudioDevice for T {
    fn name(&self) -> Cow<str> {
        AudioDevice::name(self)
    }

    fn device_type(&self) -> DeviceType {
        AudioDevice::device_type(self)
    }

    fn channel_map(&self) -> Vec<crate::Channel> {
        AudioDevice::channel_map(self).into_iter().collect()
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        AudioDevice::is_config_supported(self, config)
    }

    fn enumerate_configurations(&self) -> Vec<StreamConfig> {
        AudioDevice::enumerate_configurations(self).into_iter().flatten().collect()
    }
}

pub trait RawInputDevice: RawAudioDevice {
    fn create_input_stream_raw(&self, callback: RawCallback) -> Box<dyn Any>;
}

impl dyn RawInputDevice {
    pub fn default_input_config(&self) -> Result<StreamConfig
    pub fn create_stream<Callback: 'static + Send + AudioDuplexCallback>(&self, callback: Callback) -> Result<StreamHandle<Callback>> {
        let callback = RawCallback { data: Some(Box::new(callback)) };
        Ok(StreamHandle { callback: Some(self.create_stream_raw(callback)?), __callback: PhantomData })
    }
}

/// Implement `RawAudioInputDevice` for any type that implements `AudioInputDevice`.
impl<T: AudioInputDevice> RawInputDevice for T {
    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        AudioInputDevice::default_input_config(self)
    }

    fn create_input_stream(
        &self,
        stream_config: StreamConfig,
        callback: Self::Callback,
    ) -> Result<Self::StreamHandle, Self::Error> {
        AudioInputDevice::create_input_stream(self, stream_config, callback)
    }
}

/// Implement `RawAudioOutputDevice` for any type that implements `AudioOutputDevice`.
impl<T: AudioOutputDevice> RawAudioOutputDevice for T {
    type StreamHandle = T::StreamHandle<Self::Callback>;
    type Callback = dyn crate::AudioOutputCallback;

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        AudioOutputDevice::default_output_config(self)
    }

    fn create_output_stream(
        &self,
        stream_config: StreamConfig,
        callback: Self::Callback,
    ) -> Result<Self::StreamHandle, Self::Error> {
        AudioOutputDevice::create_output_stream(self, stream_config, callback)
    }
}

/// Implement `RawAudioStreamHandle` for any type that implements `AudioStreamHandle`.
impl<Callback: 'static + SendEverywhereButOnWeb, T: AudioStreamHandle<Callback>> RawAudioStreamHandle<Callback> for T {
    type Error = T::Error;

    fn eject(self) -> Result<Callback, Self::Error> {
        AudioStreamHandle::eject(self)
    }
}
