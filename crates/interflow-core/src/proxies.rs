use std::borrow::Cow;
use crate::device::{Device, StreamConfig};
use crate::DeviceType;
use crate::traits::ExtensionProvider;

pub type Error = Box<dyn Send + Sync + std::error::Error>;

pub trait DeviceProxy: ExtensionProvider {
    fn name(&self) -> Cow<'_, str>;
    fn device_type(&self) -> DeviceType;
    fn default_config(&self) -> Result<StreamConfig, Error>;
    fn is_config_supported(&self, config: &StreamConfig) -> bool;
    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), Error>;
}

impl<D: Device> DeviceProxy for D {
    #[inline]
    fn name(&self) -> Cow<'_, str> {
        Device::name(self)
    }
    
    fn device_type(&self) -> DeviceType {
        Device::device_type(self)
    }
    
    fn default_config(&self) -> Result<StreamConfig, Error> {
        Ok(Device::default_config(self)?)
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        Device::is_config_supported(self, config)
    }
    
    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), Error> {
        Ok(Device::buffer_size_range(self)?)
    }
}

pub trait IntoDeviceProxy {
    fn into_device_proxy(self) -> Box<dyn DeviceProxy>;
}

impl<D: Device> IntoDeviceProxy for D {
    #[inline]
    fn into_device_proxy(self) -> Box<dyn DeviceProxy> {
        Box::new(self)
    }
}
