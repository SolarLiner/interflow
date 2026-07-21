use crate::device::{Device, StreamConfig};
use crate::platform::Platform;
use crate::stream::StreamHandle;
use crate::traits::ExtensionProvider;
use crate::{stream, DeviceType};
use std::borrow::Cow;
use std::ptr::NonNull;
use std::rc::Rc;

pub type Error = Box<dyn Send + Sync + std::error::Error>;

pub type DynPlatform = Rc<dyn PlatformProxy>;
pub type DynDevice = Rc<dyn DeviceProxy>;

pub fn default_stream<Callback: 'static + stream::Callback>(
    platform: &dyn PlatformProxy,
    device_type: DeviceType,
    callback: Callback,
) -> Result<StreamProxy<Callback>, Error> {
    #[derive(Debug, thiserror::Error)]
    #[error("Cannot create dynamic stream")]
    struct CannotCreateDynStream;

    let device = platform.default_device(device_type)?;
    let Some(ext) = (&*device as &dyn ExtensionProvider).lookup::<dyn CreateStream<Callback>>()
    else {
        return Err(Box::new(CannotCreateDynStream));
    };
    ext.create_default_stream(callback)
}

pub trait PlatformProxy: ExtensionProvider {
    fn name(&self) -> Cow<'static, str>;
    fn enumerate_devices(&self) -> Result<Vec<DynDevice>, Error>;
    fn default_device(&self, device_type: DeviceType) -> Result<DynDevice, Error>;
}

impl<P: Platform> PlatformProxy for P {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(P::NAME)
    }

    fn enumerate_devices(&self) -> Result<Vec<DynDevice>, Error> {
        Ok(Vec::from_iter(
            Platform::list_devices(self)?
                .into_iter()
                .map(|dev| Rc::new(dev) as DynDevice),
        ))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<DynDevice, Error> {
        let device = Platform::default_device(self, device_type)?;
        Ok(Rc::new(device) as DynDevice)
    }
}

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

pub trait CreateStream<Callback: stream::Callback>: DeviceProxy {
    fn create_stream(
        &self,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<StreamProxy<Callback>, Error>;
    fn create_default_stream(&self, callback: Callback) -> Result<StreamProxy<Callback>, Error> {
        let config = self.default_config()?;
        self.create_stream(config, callback)
    }
}

impl<D: Device, Callback: 'static + stream::Callback> CreateStream<Callback> for D
where
    <D::StreamHandle<Callback> as StreamHandle<Callback>>::Error: 'static + Sync,
{
    fn create_stream(
        &self,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<StreamProxy<Callback>, Error> {
        let handle = Device::create_stream(self, config, callback)?;
        Ok(StreamProxy::from_handle(handle))
    }
}

pub struct StreamProxy<Callback: stream::Callback> {
    data: Option<NonNull<()>>,
    eject: unsafe fn(NonNull<()>) -> Result<Callback, Error>,
}

impl<Callback: stream::Callback> StreamProxy<Callback> {
    pub fn from_handle<Handle: stream::StreamHandle<Callback, Error: 'static + Sync>>(
        value: Handle,
    ) -> Self {
        let eject = |data: NonNull<()>| {
            // SAFETY: StreamProxy only calls this function when `data` actually points to a valid
            // `Handle` instance
            let handle = unsafe { data.cast::<Handle>().read() };
            Ok(handle.eject()?)
        };
        let value = Box::into_raw(Box::new(value));
        let data = Some(NonNull::new(value).unwrap().cast());
        Self { data, eject }
    }

    pub fn eject(mut self) -> Result<Callback, Error> {
        let data = self.data.take().expect("Cannot have ejected twice");
        // SAFETY: Using a `Option` guarantees that we only eject once, and we can only create a
        // pointer of the actual underlying type expected by `eject`.
        unsafe { (self.eject)(data) }
    }
}
