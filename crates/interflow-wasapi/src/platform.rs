use crate::device::{Device, DeviceList};
use crate::util::MMDevice;
use crate::Error;
use bitflags::bitflags_match;
#[cfg(feature = "collect")]
use interflow_core::collect;
use interflow_core::platform;
use interflow_core::proxies::PlatformProxy;
use interflow_core::traits::{ExtensionProvider, Selector};
use interflow_core::DeviceType;
#[cfg(feature = "collect")]
use std::rc::Rc;
use std::sync::OnceLock;
use windows::Win32::Media::Audio;
use windows::Win32::Media::Audio::{EDataFlow, ERole};
use windows::Win32::System::Com;

pub struct Platform;

impl ExtensionProvider for Platform {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector.register::<dyn DefaultForRole>(self)
    }
}

impl platform::Platform for Platform {
    type Error = Error;
    type Device = Device;
    const NAME: &'static str = "WASAPI";

    fn default_device(&self, device_type: DeviceType) -> Result<Self::Device, Self::Error> {
        let Some(device) = audio_device_enumerator().get_default_device(device_type)? else {
            return Err(Error::ConfigurationNotAvailable);
        };
        Ok(device)
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
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
        let com = crate::util::com().unwrap();
        unsafe {
            let enumerator = com
                .create_instance::<_, Audio::IMMDeviceEnumerator>(
                    &Audio::MMDeviceEnumerator,
                    None,
                    Com::CLSCTX_ALL,
                )
                .unwrap();
            AudioDeviceEnumerator(enumerator)
        }
    })
}

struct AudioDeviceEnumerator(Audio::IMMDeviceEnumerator);

unsafe impl Send for AudioDeviceEnumerator {}
unsafe impl Sync for AudioDeviceEnumerator {}

impl AudioDeviceEnumerator {
    fn get_default_device(&self, device_type: DeviceType) -> Result<Option<Device>, Error> {
        let Some(flow) = bitflags_match!(device_type, {
            DeviceType::INPUT => Some(Audio::eCapture),
            DeviceType::OUTPUT => Some(Audio::eRender),
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

    fn get_device_list(&self) -> Result<impl IntoIterator<Item = Device>, Error> {
        unsafe {
            let output_collection = self
                .0
                .EnumAudioEndpoints(Audio::eRender, Audio::DEVICE_STATE_ACTIVE)?;
            let count = output_collection.GetCount()?;
            let output_device_list = DeviceList {
                collection: output_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::OUTPUT,
            };

            let input_collection = self
                .0
                .EnumAudioEndpoints(Audio::eCapture, Audio::DEVICE_STATE_ACTIVE)?;
            let count = input_collection.GetCount()?;
            let input_device_list = DeviceList {
                collection: input_collection,
                total_count: count,
                next_item: 0,
                device_type: DeviceType::INPUT,
            };

            Ok(output_device_list.chain(input_device_list))
        }
    }
}

pub trait DefaultForRole: PlatformProxy {
    fn default_for_role(&self, flow: Audio::EDataFlow, role: Audio::ERole)
        -> Result<Device, Error>;
}

impl DefaultForRole for Platform {
    fn default_for_role(&self, flow: EDataFlow, role: ERole) -> Result<Device, Error> {
        Ok(audio_device_enumerator().get_default_device_with_role(flow, role)?)
    }
}

#[cfg(feature = "collect")]
#[scattered_collect::scatter(collect::REGISTRAR)]
static WASAPI_PLATFORM_REGISTRATION: collect::Registration = collect::Registration {
    constructor: || Some(Rc::new(Platform)),
    priority: collect::DEFAULT_PLATFORM_PRIORITY,
};
