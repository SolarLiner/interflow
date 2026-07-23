//! CoreAudio platform implementation
use crate::device::Device;
use crate::Error;
use coreaudio::audio_unit::macos_helpers::{get_audio_device_ids, get_default_device_id};
use interflow_core::traits::{ExtensionProvider, Selector};
use interflow_core::{platform, DeviceType};

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
            return Ok(Device::default_output());
        }

        let Some(id) = get_default_device_id(device_type.is_input()) else {
            return Err(Error::NoMatchingDevices(device_type));
        };
        Ok(Device::from_id(id))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        Ok(get_audio_device_ids()?.into_iter().map(Device::from_id))
    }
}
