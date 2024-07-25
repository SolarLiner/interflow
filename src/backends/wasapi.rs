use std::{
    borrow::Cow,
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    sync::OnceLock,
};

use crate::{AudioDevice, AudioDriver, Channel, DeviceType, StreamConfig};
use thiserror::Error;
use windows::
    Win32::{
        Devices::Properties,
        Media::Audio,
        System::{
            Com::{self, StructuredStorage, STGM_READ},
            Variant::VT_LPWSTR,
        },
    };


mod util {
    use std::marker::PhantomData;

    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

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
}

/// Type of errors from the WASAPI backend.
#[derive(Debug, Error)]
#[error("WASAPI error: ")]
pub enum WasapiError {
    /// Error originating from WASAPI.
    BackendError(#[from] windows::core::Error),
}

/// The WASAPI driver.
#[derive(Debug, Clone, Default)]
pub struct WasapiDriver;

impl AudioDriver for WasapiDriver {
    type Error = WasapiError;
    type Device = WasapiDevice;

    const DISPLAY_NAME: &'static str = "WASAPI";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        Ok(Cow::Borrowed("WASAPI (version unknown)"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        audio_device_enumerator().get_default_device(device_type)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        audio_device_enumerator().get_device_list()
    }
}

/// Type of devices available from the WASAPI driver.
#[derive(Debug)]
pub struct WasapiDevice {
    device: windows::Win32::Media::Audio::IMMDevice,
    device_type: DeviceType,
}

impl AudioDevice for WasapiDevice {
    type Error = WasapiError;

    fn name(&self) -> Cow<str> {
        match get_device_name(&self.device) {
            Some(std) => Cow::Owned(std),
            None => {
                eprintln!("Cannot get audio device name");
                Cow::Borrowed("<unknown>")
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        todo!()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item=StreamConfig>> {
        None::<[StreamConfig; 0]>
    }

    fn channel_map(&self) -> impl IntoIterator<Item=Channel> {
        []
    }

}

impl WasapiDevice {
    fn new(device: Audio::IMMDevice, device_type: DeviceType) -> Self {
        WasapiDevice {
            device,
            device_type,
        }
    }
}

fn get_device_name(device: &windows::Win32::Media::Audio::IMMDevice) -> Option<String> {
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
                .GetValue(&Properties::DEVPKEY_Device_DeviceDesc as *const _ as *const _)).ok()?;

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
        let name = name_os_string.into_string().unwrap_or_else(|os_string| os_string.to_string_lossy().into());

        // Clean up.
        StructuredStorage::PropVariantClear(&mut property_value).ok()?;

        Some(name)
    }
}


static ENUMERATOR: OnceLock<AudioDeviceEnumerator> = OnceLock::new();

fn audio_device_enumerator() -> &'static AudioDeviceEnumerator {
    ENUMERATOR.get_or_init(|| {
        // Make sure COM is initialised.
        util::com_initializer();

        unsafe {
            let enumerator = Com::CoCreateInstance::<_, Audio::IMMDeviceEnumerator>(
                &Audio::MMDeviceEnumerator,
                None,
                Com::CLSCTX_ALL,
            )
            .unwrap();

            AudioDeviceEnumerator(enumerator)
        }
    })
}

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
struct AudioDeviceEnumerator(Audio::IMMDeviceEnumerator);

impl AudioDeviceEnumerator {

    // Returns the default output device.
    fn get_default_device(&self, device_type: DeviceType) -> Result<Option<WasapiDevice>, WasapiError> {
        let data_flow = match device_type {
            DeviceType::Input => Audio::eCapture,
            DeviceType::Output => Audio::eRender,
            _=> return Ok(None),
        };

        unsafe {
            let device = self
                .0
                .GetDefaultAudioEndpoint(data_flow, Audio::eConsole)?;

            Ok(Some(WasapiDevice::new(device, DeviceType::Output)))
        }
    }

    // Returns a chained iterator of output and input devices.
    fn get_device_list(&self) -> Result<impl IntoIterator<Item = WasapiDevice>, WasapiError> {

        // Create separate collections for output and input devices and then chain them.
        unsafe {
            let output_collection = self
                .0
                .EnumAudioEndpoints(Audio::eRender, Audio::DEVICE_STATE_ACTIVE)?;

            let count = output_collection.GetCount()?;

            let output_device_list = WasapiDeviceList {
                collection: output_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::Output,
            };

            let input_collection = self
                .0
                .EnumAudioEndpoints(Audio::eCapture, Audio::DEVICE_STATE_ACTIVE)?;

            let count = input_collection.GetCount()?;

            let input_device_list = WasapiDeviceList {
                collection: input_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::Input,
            };

            Ok(output_device_list.chain(input_device_list))
        }
    }
}

unsafe impl Send for AudioDeviceEnumerator {}
unsafe impl Sync for AudioDeviceEnumerator {}

/// An iterable collection WASAPI devices.
pub struct WasapiDeviceList {
    collection: Audio::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
    device_type: DeviceType,
}

unsafe impl Send for WasapiDeviceList {}
unsafe impl Sync for WasapiDeviceList {}

impl Iterator for WasapiDeviceList {
    type Item = WasapiDevice;

    fn next(&mut self) -> Option<WasapiDevice> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
            self.next_item += 1;
            Some(WasapiDevice::new(device, self.device_type))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rest = (self.total_count - self.next_item) as usize;
        (rest, Some(rest))
    }
}