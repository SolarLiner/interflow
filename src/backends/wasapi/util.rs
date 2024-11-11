use crate::prelude::wasapi::error;
use std::marker::PhantomData;
use windows::core::Interface;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::Audio;
use windows::Win32::System::Com;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, StructuredStorage, COINIT_APARTMENTTHREADED, STGM_READ};
use windows::Win32::Devices::Properties;
use windows::Win32::System::Variant::VT_LPWSTR;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

thread_local!(static COM_INITIALIZER: ComInitializer = {
    unsafe {
        // Try to initialize COM with STA by default to avoid compatibility issues with the ASIO
        // backend (where CoInitialize() is called by the ASIO SDK) or winit (where drag and drop
        // requires STA).
        // This call can fail with RPC_E_CHANGED_MODE if another library initialized COM with MTA.
        // That's OK though since COM ensures thread-safety/compatibility through marshalling when
        // necessary.
        let result = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if result.is_ok() || result == RPC_E_CHANGED_MODE {
            ComInitializer {
                result,
                _ptr: PhantomData,
            }
        } else {
            // COM initialization failed in another way, something is really wrong.
            panic!(
                "Failed to initialize COM: {}",
                std::io::Error::from_raw_os_error(result.0)
            );
        }
    }
});

/// RAII object that guards the fact that COM is initialized.
///
// We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
// object.
struct ComInitializer {
    result: windows::core::HRESULT,
    _ptr: PhantomData<*mut ()>,
}

impl Drop for ComInitializer {
    #[inline]
    fn drop(&mut self) {
        // Need to avoid calling CoUninitialize() if CoInitializeEx failed since it may have
        // returned RPC_E_MODE_CHANGED - which is OK, see above.
        if self.result.is_ok() {
            unsafe { CoUninitialize() };
        }
    }
}

/// Ensures that COM is initialized in this thread.
#[inline]
pub fn com_initializer() {
    COM_INITIALIZER.with(|_| {});
}

#[derive(Debug, Clone)]
pub struct WasapiMMDevice(Audio::IMMDevice);

unsafe impl Send for WasapiMMDevice {}

impl WasapiMMDevice {
    pub(crate) fn new(device: Audio::IMMDevice) -> Self {
        Self(device)
    }

    pub(crate) fn activate<T: Interface>(&self) -> Result<T, error::WasapiError> {
        unsafe {
            self.0
                .Activate::<T>(Com::CLSCTX_ALL, None)
                .map_err(|err| error::WasapiError::BackendError(err))
        }
    }
    
    pub(crate) fn name(&self) -> Option<String> {
        get_device_name(&self.0)
    }
}

fn get_device_name(device: &Audio::IMMDevice) -> Option<String> {
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
            .ok()?;

        let prop_variant = &property_value.as_raw().Anonymous.Anonymous;

        // Read the friendly-name from the union data field, expecting a *const u16.
        if prop_variant.vt != VT_LPWSTR.0 {
            return None;
        }

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
        StructuredStorage::PropVariantClear(&mut property_value).ok()?;

        Some(name)
    }
}