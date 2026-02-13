pub mod device;
mod util;
mod stream;

use std::sync::OnceLock;
use bitflags::bitflags_match;
use windows::Win32::Media::Audio;
use windows::Win32::System::Com;
use device::Device;
use interflow_core::{platform, DeviceType};
use interflow_core::traits::{ExtensionProvider, Selector};
use crate::util::MMDevice;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error originating from WASAPI.
    #[error("{} (code {})", .0.message(), .0.code())]
    BackendError(#[from] windows::core::Error),
    /// Requested WASAPI device configuration is not available
    #[error("Configuration not available")]
    ConfigurationNotAvailable,
    /// Windows Foundation error
    #[error("Win32 error: {0}")]
    FoundationError(String),
    /// Duplex stream requested, unsupported by WASAPI
    #[error("Unsupported duplex stream requested")]
    DuplexStreamRequested,
}

#[derive(Debug, Copy, Clone)]
pub struct Platform;

impl ExtensionProvider for Platform {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector.register::<dyn DefaultByRole>(self)
    }
}

impl platform::Platform for Platform {
    type Error = Error;
    type Device = Device;
    const NAME: &'static str = "";

    fn default_device(device_type: DeviceType) -> Result<Self::Device, Self::Error> {
        let Some(device) = audio_device_enumerator().get_default_device(device_type)? else {
            return Err(Error::ConfigurationNotAvailable);
        };
        Ok(device)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item=Self::Device>, Self::Error> {
        audio_device_enumerator().get_device_list()
    }
}

pub trait DefaultByRole {
    fn default_by_role(&self, flow: Audio::EDataFlow, role: Audio::ERole) -> Result<Device, Error>;
}

impl DefaultByRole for Platform {
    fn default_by_role(&self, flow: Audio::EDataFlow, role: Audio::ERole) -> Result<Device, Error> {
        audio_device_enumerator().get_default_device_with_role(flow, role)
    }
}

fn audio_device_enumerator() -> &'static AudioDeviceEnumerator {
    static ENUMERATOR: OnceLock<AudioDeviceEnumerator> = OnceLock::new();
    ENUMERATOR.get_or_init(|| {
        // Make sure COM is initialised.
        let com = util::com().unwrap();

        unsafe {
            let enumerator = com.create_instance::<_, Audio::IMMDeviceEnumerator>(
                &Audio::MMDeviceEnumerator,
                None,
                Com::CLSCTX_ALL,
            ).unwrap();

            AudioDeviceEnumerator(enumerator)
        }
    })
}

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
pub struct AudioDeviceEnumerator(Audio::IMMDeviceEnumerator);

impl AudioDeviceEnumerator {
    // Returns the default output device.
    fn get_default_device(
        &self,
        device_type: DeviceType,
    ) -> Result<Option<Device>, Error> {
        let Some(flow) = bitflags_match!(device_type, {
            DeviceType::INPUT | DeviceType::PHYSICAL => Some(Audio::eCapture),
            DeviceType::OUTPUT | DeviceType::PHYSICAL => Some(Audio::eRender),
            _ => None,
        }) else {
            return Ok(None);
        };

        self.get_default_device_with_role(flow, Audio::eConsole)
            .map(Some)
    }

    fn get_default_device_with_role(
        &self,
        flow: Audio::EDataFlow,
        role: Audio::ERole,
    ) -> Result<Device, Error> {
        unsafe {
            let device = self.0.GetDefaultAudioEndpoint(flow, role)?;
            let device_type = match flow {
                Audio::eRender => DeviceType::OUTPUT,
                _ => DeviceType::INPUT,
            };
            Ok(Device {
                handle: MMDevice::new(device),
                device_type: DeviceType::PHYSICAL | device_type,
            })
        }
    }

    // Returns a chained iterator of output and input devices.
    fn get_device_list(
        &self,
    ) -> Result<impl IntoIterator<Item = Device>, Error> {
        // Create separate collections for output and input devices and then chain them.
        unsafe {
            let output_collection = self
                .0
                .EnumAudioEndpoints(Audio::eRender, Audio::DEVICE_STATE_ACTIVE)?;

            let count = output_collection.GetCount()?;

            let output_device_list = device::DeviceList {
                collection: output_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::OUTPUT,
            };

            let input_collection = self
                .0
                .EnumAudioEndpoints(Audio::eCapture, Audio::DEVICE_STATE_ACTIVE)?;

            let count = input_collection.GetCount()?;

            let input_device_list = device::DeviceList {
                collection: input_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::INPUT,
            };

            Ok(output_device_list.chain(input_device_list))
        }
    }
}

unsafe impl Send for AudioDeviceEnumerator {}

unsafe impl Sync for AudioDeviceEnumerator {}
