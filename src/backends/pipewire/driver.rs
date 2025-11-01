use super::error::PipewireError;
use crate::backends::pipewire::device::PipewireDevice;
use crate::backends::pipewire::utils;
use crate::{AudioDriver, DeviceType};
use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;

pub struct PipewireDriver {
    __init: PhantomData<()>,
}

impl AudioDriver for PipewireDriver {
    type Error = PipewireError;
    type Device = PipewireDevice;
    const DISPLAY_NAME: &'static str = "Pipewire";

    fn version(&self) -> Result<Cow<'_, str>, Self::Error> {
        // TODO: Figure out how to get version
        Ok(Cow::Borrowed("unknown"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        Ok(Some(PipewireDevice {
            target_node: None,
            device_type,
            object_serial: None,
            stream_name: Cow::Borrowed("Interflow stream"),
            stream_properties: HashMap::new(),
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
                stream_properties: HashMap::new(),
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
