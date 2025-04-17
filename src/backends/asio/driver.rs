use std::{borrow::Cow, sync::Arc};

use asio_sys as asio;

use crate::{device::DeviceType, driver::AudioDriver};
use super::{device::AsioDevice, error::AsioError};

/// The ASIO driver.
#[derive(Debug, Clone, Default)]
pub struct AsioDriver {
    asio: Arc<asio::Asio>,
}

impl AsioDriver {
    /// Create a new ASIO driver.
    pub fn new() -> Result<Self, AsioError> {
        let asio = Arc::new(asio::Asio::new());
        Ok(AsioDriver { asio })
    }
}

impl AudioDriver for AsioDriver {
    type Error = AsioError;
    type Device = AsioDevice;

    const DISPLAY_NAME: &'static str = "ASIO";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        Ok(Cow::Borrowed("unknown"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        let mut iter = AsioDeviceList::new(self.asio.clone())?;

        let dd = iter.find(|device| device_type.intersects(device.device_type()));
        Ok(dd)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        AsioDeviceList::new(self.asio.clone())
    }
}

pub struct AsioDeviceList {
    asio: Arc<asio::Asio>,
    drivers: std::vec::IntoIter<String>,
}

impl AsioDeviceList {
    pub fn new(asio: Arc<asio::Asio>) -> Result<Self, AsioError> {
        let drivers = asio.driver_names().into_iter();
        Ok(AsioDeviceList { asio, drivers })
    }
}

impl Iterator for AsioDeviceList {
    type Item = AsioDevice;

    fn next(&mut self) -> Option<AsioDevice> {
        loop {
            match self.drivers.next() {
                Some(name) => match self.asio.load_driver(&name) {
                    Ok(driver) => {
                        let driver = Arc::new(driver);
                        return AsioDevice::new(driver).ok();
                    }
                    Err(_) => continue,
                },
                None => return None,
            }
        }
    }
}
