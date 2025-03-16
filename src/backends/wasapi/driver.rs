use crate::backends::wasapi::device::{WasapiDevice, WasapiDeviceList};
use std::borrow::Cow;
use std::sync::OnceLock;
use windows::Win32::Media::Audio;
use windows::Win32::System::Com;

use super::{error, util};

use crate::device::DeviceType;
use crate::driver::AudioDriver;

/// The WASAPI driver.
#[derive(Debug, Clone, Default)]
pub struct WasapiDriver;

impl AudioDriver for WasapiDriver {
    type Error = error::WasapiError;
    type Device = WasapiDevice;

    const DISPLAY_NAME: &'static str = "WASAPI";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        Ok(Cow::Borrowed("unknown"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        audio_device_enumerator().get_default_device(device_type)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        audio_device_enumerator().get_device_list()
    }
}

pub fn audio_device_enumerator() -> &'static AudioDeviceEnumerator {
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

static ENUMERATOR: OnceLock<AudioDeviceEnumerator> = OnceLock::new();

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
pub struct AudioDeviceEnumerator(Audio::IMMDeviceEnumerator);

impl AudioDeviceEnumerator {
    // Returns the default output device.
    fn get_default_device(
        &self,
        device_type: DeviceType,
    ) -> Result<Option<WasapiDevice>, error::WasapiError> {
        let data_flow = match device_type {
            DeviceType::Input => Audio::eCapture,
            DeviceType::Output => Audio::eRender,
            _ => return Ok(None),
        };

        unsafe {
            let device = self.0.GetDefaultAudioEndpoint(data_flow, Audio::eConsole)?;

            Ok(Some(WasapiDevice::new(device, DeviceType::Output)))
        }
    }

    // Returns a chained iterator of output and input devices.
    fn get_device_list(
        &self,
    ) -> Result<impl IntoIterator<Item = WasapiDevice>, error::WasapiError> {
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
