use std::ffi::OsString;
use std::marker::PhantomData;
use std::ops;
use std::os::windows::ffi::OsStringExt;
use std::ptr::{self, NonNull};
use std::sync::OnceLock;
use windows::core::Interface;
use windows::Win32::Devices::Properties;
use windows::Win32::Media::Audio;
use windows::Win32::System::Com::{self, CoTaskMemFree, CLSCTX, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, StructuredStorage, STGM_READ};
use windows::Win32::System::Variant::VT_LPWSTR;

/// RAII object that guards the fact that COM is initialized.
///
// We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
// object.
struct ComponentObjectModel(PhantomData<()>);

impl ComponentObjectModel {
    pub unsafe fn create_instance<
        P1: windows::core::Param<windows::core::IUnknown>,
        T: Interface,
    >(
        &self,
        guid: *const windows::core::GUID,
        param1: P1,
        class_context: CLSCTX,
    ) -> windows::core::Result<T> {
        Com::CoCreateInstance(guid, param1, class_context)
    }
}

impl Drop for ComponentObjectModel {
    #[inline]
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Ensures that COM is initialized in this thread.
#[inline]
pub fn com() -> windows::core::Result<&'static ComponentObjectModel> {
    static VALUE: OnceLock<ComponentObjectModel> = OnceLock::new();
    let Some(value) = VALUE.get() else {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        let _ = VALUE.set(ComponentObjectModel(PhantomData));
        return Ok(VALUE.get().unwrap());
    };
    Ok(value)
}

#[derive(Debug, Clone)]
pub struct MMDevice(Audio::IMMDevice);

unsafe impl Send for MMDevice {}

impl MMDevice {
    pub(crate) fn new(device: Audio::IMMDevice) -> Self {
        Self(device)
    }

    pub(crate) fn activate<T: Interface>(&self) -> Result<T, crate::Error> {
        unsafe {
            self.0
                .Activate::<T>(Com::CLSCTX_ALL, None)
                .map_err(crate::Error::BackendError)
        }
    }

    pub(crate) fn name(&self) -> String {
        get_device_name(&self.0)
    }
}

fn get_device_name(device: &Audio::IMMDevice) -> String {
    unsafe {
        // Open the device's property store.
        let property_store = device
            .OpenPropertyStore(STGM_READ)
            .expect("could not open property store");

        // Get the endpoint's friendly-name property, else the interface's friendly-name, else the device description.
        let mut property_value = property_store
            .GetValue(&Properties::DEVPKEY_Device_FriendlyName as *const _ as *const _)
            .or(property_store.GetValue(
                &Properties::DEVPKEY_DeviceInterface_FriendlyName as *const _ as *const _,
            ))
            .or(property_store
                .GetValue(&Properties::DEVPKEY_Device_DeviceDesc as *const _ as *const _))
            .unwrap();

        let prop_variant = &property_value.Anonymous.Anonymous;

        // Read the friendly-name from the union data field, expecting a *const u16.
        assert_eq!(VT_LPWSTR, prop_variant.vt);

        let ptr_utf16 = *(&prop_variant.Anonymous as *const _ as *const *const u16);

        // Find the length of the friendly name.
        let mut len = 0;
        while *ptr_utf16.offset(len) != 0 {
            len += 1;
        }

        // Convert to a string.
        let name_slice = std::slice::from_raw_parts(ptr_utf16, len as usize);
        let name_os_string: OsString = OsStringExt::from_wide(name_slice);
        let name = name_os_string
            .into_string()
            .unwrap_or_else(|os_string| os_string.to_string_lossy().into());

        // Clean up.
        StructuredStorage::PropVariantClear(&mut property_value).unwrap();

        name
    }
}

#[repr(transparent)]
pub(super) struct CoTask<T> {
    ptr: NonNull<T>,
}

impl<T> ops::Deref for CoTask<T> {
    type Target = NonNull<T>;
    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}

impl<T> ops::DerefMut for CoTask<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ptr
    }
}

impl<T> Drop for CoTask<T> {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.ptr.as_ptr().cast()));
        }
    }
}

impl<T> CoTask<T> {
    pub(super) const unsafe fn new(ptr: NonNull<T>) -> Self {
        Self { ptr }
    }

    pub(super) unsafe fn construct(func: impl FnOnce(*mut *mut T) -> bool) -> Option<Self> {
        let mut ptr = ptr::null_mut();
        if !func(&mut ptr) {
            return None;
        }
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }
}
