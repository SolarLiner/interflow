use crate::device::{Device, StreamConfig};
use crate::platform::Platform;
use crate::stream::{AudioInput, AudioOutput, CallbackContext};
use crate::traits::ExtensionProvider;
use crate::{stream, DeviceType};
use anyhow::{Context, Result};
use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;

/// Type alias for a reference-counted platform proxy.
pub type DynPlatform = Rc<dyn PlatformProxy>;

/// Type alias for a reference-counted device proxy.
pub type DynDevice = Rc<dyn DeviceProxy>;

/// Trait for platform proxies that provides access to audio devices.
pub trait PlatformProxy: ExtensionProvider {
    fn name(&self) -> Cow<'static, str>;
    fn list_devices(&self) -> Result<Vec<DynDevice>>;
    fn list_devices_matching(&self, device_type: DeviceType) -> Result<Vec<DynDevice>>;
    fn default_device(&self, device_type: DeviceType) -> Result<DynDevice>;
}

impl<P: Platform> PlatformProxy for P
where
    <<<P as Platform>::Device as Device>::StreamHandle<DynCallback> as stream::StreamHandle<
        DynCallback,
    >>::Error: Sync,
{
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(P::NAME)
    }

    fn list_devices(&self) -> Result<Vec<DynDevice>> {
        Ok(Vec::from_iter(
            Platform::list_devices(self)?
                .into_iter()
                .map(|dev| Rc::new(dev) as DynDevice),
        ))
    }

    fn list_devices_matching(&self, device_type: DeviceType) -> Result<Vec<DynDevice>> {
        Ok(Vec::from_iter(
            Platform::list_devices(self)?.into_iter().filter_map(|dev| {
                Device::device_type(&dev)
                    .contains(device_type)
                    .then(|| Rc::new(dev) as DynDevice)
            }),
        ))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<DynDevice> {
        let device = Platform::default_device(self, device_type)?;
        Ok(Rc::new(device) as DynDevice)
    }
}

/// Trait for device proxies that describes audio device capabilities.
pub trait DeviceProxy: ExtensionProvider {
    fn name(&self) -> Cow<'_, str>;
    fn device_type(&self) -> DeviceType;
    fn default_config(&self) -> Result<StreamConfig>;
    fn is_config_supported(&self, config: &StreamConfig) -> bool;
    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>)>;
    fn create_stream_raw(
        &self,
        config: StreamConfig,
        callback: DynCallback,
    ) -> Result<RawStreamHandle>;

    fn create_default_stream_raw(
        &self,
        requested_type: DeviceType,
        callback: DynCallback,
    ) -> Result<RawStreamHandle>;
}

impl<D: Device> DeviceProxy for D
where
    <D::StreamHandle<DynCallback> as stream::StreamHandle<DynCallback>>::Error: Sync,
{
    #[inline]
    fn name(&self) -> Cow<'_, str> {
        Device::name(self)
    }

    fn device_type(&self) -> DeviceType {
        Device::device_type(self)
    }

    fn default_config(&self) -> Result<StreamConfig> {
        Ok(Device::default_config(self)?)
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        Device::is_config_supported(self, config)
    }

    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>)> {
        Ok(Device::buffer_size_range(self)?)
    }

    fn create_stream_raw(
        &self,
        config: StreamConfig,
        callback: DynCallback,
    ) -> Result<RawStreamHandle> {
        let handle = Device::create_stream(self, config, callback).context("Cannot open stream")?;
        Ok(RawStreamHandle::from_handle(handle))
    }

    fn create_default_stream_raw(
        &self,
        requested_type: DeviceType,
        callback: DynCallback,
    ) -> Result<RawStreamHandle> {
        let handle =
            Device::default_stream(self, requested_type, callback).context("Cannot open stream")?;
        Ok(RawStreamHandle::from_handle(handle))
    }
}

pub struct DynCallback {
    type_id: TypeId,
    handle: NonNull<()>,
    prepare: unsafe fn(NonNull<()>, CallbackContext),
    process_audio: unsafe fn(NonNull<()>, CallbackContext, &AudioInput<f32>, &mut AudioOutput<f32>),
}

unsafe impl Send for DynCallback {}

impl DynCallback {
    pub fn from_callback<Callback: 'static + stream::Callback>(callback: Callback) -> Self {
        Self {
            type_id: TypeId::of::<Callback>(),
            handle: unsafe { create_nonnull(callback).cast() },
            prepare: |handle, context| {
                unsafe { handle.cast::<Callback>().as_mut() }.prepare(context)
            },
            process_audio: |handle, context, input, output| {
                unsafe { handle.cast::<Callback>().as_mut() }.process_audio(context, input, output)
            },
        }
    }

    pub fn into_inner<Callback: 'static + stream::Callback>(self) -> Option<Box<Callback>> {
        if self.type_id != TypeId::of::<Callback>() {
            return None;
        }
        Some(unsafe { consume_nonnull(self.handle.cast::<Callback>()) })
    }
}

impl stream::Callback for DynCallback {
    fn prepare(&mut self, context: CallbackContext) {
        unsafe { (self.prepare)(self.handle, context) }
    }

    fn process_audio(
        &mut self,
        context: CallbackContext,
        input: &AudioInput<f32>,
        output: &mut AudioOutput<f32>,
    ) {
        unsafe { (self.process_audio)(self.handle, context, input, output) }
    }
}

pub struct RawStreamHandle {
    handle: Option<NonNull<()>>,
    eject: unsafe fn(NonNull<()>) -> Result<DynCallback>,
    drop: unsafe fn(NonNull<()>),
}

impl RawStreamHandle {
    pub fn from_handle<Handle: stream::StreamHandle<DynCallback, Error: 'static + Sync>>(
        handle: Handle,
    ) -> Self {
        let handle = Box::into_raw(Box::new(handle));
        let handle = Some(unsafe { NonNull::new_unchecked(handle).cast() });
        Self {
            handle,
            eject: |ptr| {
                let handle = unsafe { Box::from_raw(ptr.cast::<Handle>().as_ptr()) };
                Ok(handle.eject()?)
            },
            drop: |ptr| {
                let _ = unsafe { Box::from_raw(ptr.cast::<Handle>().as_ptr()) };
            },
        }
    }

    pub fn eject(mut self) -> Result<DynCallback> {
        let Some(handle) = self.handle.take() else {
            anyhow::bail!("Stream already ejected");
        };
        Ok(unsafe { (self.eject)(handle)? })
    }
}

impl Drop for RawStreamHandle {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        unsafe { (self.drop)(handle) };
    }
}

pub trait CreateStreamExt: DeviceProxy {
    fn create_stream<Callback: 'static + stream::Callback>(
        &self,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<StreamHandle<Callback>> {
        let callback = DynCallback::from_callback(callback);
        let handle = self.create_stream_raw(config, callback)?;
        Ok(unsafe { StreamHandle::from_raw(handle) })
    }

    fn default_stream<Callback: 'static + stream::Callback>(
        &self,
        requested_type: DeviceType,
        callback: Callback,
    ) -> Result<StreamHandle<Callback>> {
        let callback = DynCallback::from_callback(callback);
        let handle = self.create_default_stream_raw(requested_type, callback)?;
        Ok(unsafe { StreamHandle::from_raw(handle) })
    }
}

impl<C: ?Sized + DeviceProxy> CreateStreamExt for C {}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct AnyError(anyhow::Error);

pub struct StreamHandle<Callback: stream::Callback> {
    __callback: PhantomData<Callback>,
    raw: RawStreamHandle,
}

impl<Callback: stream::Callback> StreamHandle<Callback> {
    unsafe fn from_raw(raw: RawStreamHandle) -> Self {
        Self {
            __callback: PhantomData,
            raw,
        }
    }
}

impl<Callback: Any + stream::Callback> stream::StreamHandle<Box<Callback>>
    for StreamHandle<Callback>
{
    type Error = AnyError;

    fn eject(self) -> std::result::Result<Box<Callback>, Self::Error> {
        (|| -> Result<Box<Callback>> {
            let raw_callback = self.raw.eject()?;
            Ok(raw_callback.into_inner().unwrap())
        })()
        .map_err(AnyError)
    }
}

unsafe fn create_nonnull<T>(value: T) -> NonNull<T> {
    NonNull::new_unchecked(Box::into_raw(Box::new(value)))
}

unsafe fn consume_nonnull<T: ?Sized>(handle: NonNull<T>) -> Box<T> {
    Box::from_raw(handle.as_ptr())
}
