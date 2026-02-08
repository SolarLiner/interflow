use std::borrow::Cow;
use crate::device::Device;
use crate::DeviceType;
use crate::traits::ExtensionProvider;

/// Trait for platforms which provide audio devices.
pub trait Platform: ExtensionProvider {
    type Error: Send + Sync + std::error::Error;
    type Device: Device<Error: Into<Self::Error>>;
    const NAME: &'static str;
    
    fn default_device(device_type: DeviceType) -> Result<Self::Device, Self::Error>;
    
    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error>;
}

pub trait ServerInfo {
    fn version(&self) -> Cow<'_, str>;
}