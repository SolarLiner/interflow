use super::error::PipewireError;
use crate::backends::pipewire::device::PipewireDevice;
use crate::backends::pipewire::utils;
use crate::backends::pipewire::utils::get_info;
use crate::{AudioDriver, DeviceType};
use std::borrow::Cow;
use std::marker::PhantomData;

pub struct PipewireDriver {
    __init: PhantomData<()>,
}

impl AudioDriver for PipewireDriver {
    type Error = PipewireError;
    type Device = PipewireDevice;
    const DISPLAY_NAME: &'static str = "Pipewire";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        let info = get_info()?;

        if let Some(version) = info.get("version") {
            return Ok(Cow::Owned(version.to_owned()));
        }

        Ok(Cow::Borrowed("unknown"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        Ok(Some(PipewireDevice {
            target_node: None,
            device_type,
            object_serial: None,
            stream_name: Cow::Borrowed("Interflow stream"),
        }))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        Ok(utils::get_devices()?
            .into_iter()
            .map(|(id, device_type, object_serial)| PipewireDevice {
                target_node: Some(id),
                device_type,
                object_serial: Some(object_serial),
                stream_name: Cow::Borrowed("Interflow stream"),
            }))
    }
}

impl PipewireDriver {
    /// Initialize the Pipewire driver.
    pub fn new() -> Result<Self, PipewireError> {
        pipewire::init();
        Ok(Self {
            __init: PhantomData,
        })
    }
}
