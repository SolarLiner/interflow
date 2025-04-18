//! # Backends
//!
//! Home of the various backends supported by the library.
//!
//! Each backend is provided in its own submodule. Types should be public so that the user isn't
//! limited to going through the main API if they want to choose a specific backend.

use crate::{AudioDriver, AudioInputDevice, AudioOutputDevice, DeviceType};

#[cfg(unsupported)]
compile_error!("Unsupported platform (supports ALSA, CoreAudio, and WASAPI)");

#[cfg(os_alsa)]
pub mod alsa;

#[cfg(os_coreaudio)]
pub mod coreaudio;

#[cfg(os_wasapi)]
pub mod wasapi;

#[cfg(all(os_pipewire, feature = "pipewire"))]
pub mod pipewire;

/// Returns the default driver.
///
/// "Default" here means that it is a supported driver that is available on the platform.
///
/// The signature makes it unfortunately impossible to do runtime selection, and could change in
/// the future to make it possible. Until now, the "default" driver is the lowest common
/// denominator.
///
/// Selects the following driver depending on platform:
///
/// | **Platform** |           **Driver**        |
/// |:------------:|:---------------------------:|
/// |     Linux    | Pipewire (if enabled), ALSA |
/// |     macOS    |          CoreAudio          |
/// |    Windows   |           WASAPI            |
#[cfg(any(os_alsa, os_coreaudio, os_wasapi))]
#[allow(clippy::needless_return)]
pub fn default_driver() -> impl AudioDriver {
    #[cfg(all(os_pipewire, feature = "pipewire"))]
    return pipewire::driver::PipewireDriver::new().unwrap();
    #[cfg(all(not(all(os_pipewire, feature = "pipewire")), os_alsa))]
    return alsa::AlsaDriver;
    #[cfg(os_coreaudio)]
    return coreaudio::CoreAudioDriver;
    #[cfg(os_wasapi)]
    return wasapi::WasapiDriver;
}

/// Returns the default input device for the given audio driver.
///
/// The default device is usually the one the user has selected in its system settings.
pub fn default_input_device_from<Driver: AudioDriver>(driver: &Driver) -> Driver::Device
where
    Driver::Device: AudioInputDevice,
{
    driver
        .default_device(DeviceType::PHYSICAL | DeviceType::INPUT)
        .expect("Audio driver error")
        .expect("No default device found")
}

/// Default input device from the default driver for this platform.
///
/// "Default" here means both in terms of platform support but also can include runtime selection.
/// Therefore, it is better to use this method directly rather than first getting the default
/// driver from [`default_driver`].
#[cfg(any(feature = "pipewire", os_alsa, os_coreaudio, os_wasapi))]
#[allow(clippy::needless_return)]
pub fn default_input_device() -> impl AudioInputDevice {
    #[cfg(all(os_pipewire, feature = "pipewire"))]
    return default_input_device_from(&pipewire::driver::PipewireDriver::new().unwrap());
    #[cfg(all(not(all(os_pipewire, feature = "pipewire")), os_alsa))]
    return default_input_device_from(&alsa::AlsaDriver);
    #[cfg(os_coreaudio)]
    return default_input_device_from(&coreaudio::CoreAudioDriver);
    #[cfg(os_wasapi)]
    return default_input_device_from(&wasapi::WasapiDriver);
}

/// Returns the default input device for the given audio driver.
///
/// The default device is usually the one the user has selected in its system settings.
pub fn default_output_device_from<Driver: AudioDriver>(driver: &Driver) -> Driver::Device
where
    Driver::Device: AudioOutputDevice,
{
    driver
        .default_device(DeviceType::PHYSICAL | DeviceType::OUTPUT)
        .expect("Audio driver error")
        .expect("No default device found")
}

/// Default output device from the default driver for this platform.
///
/// "Default" here means both in terms of platform support but also can include runtime selection.
/// Therefore, it is better to use this method directly rather than first getting the default
/// driver from [`default_driver`].
#[cfg(any(os_alsa, os_coreaudio, os_wasapi, feature = "pipewire"))]
#[allow(clippy::needless_return)]
pub fn default_output_device() -> impl AudioOutputDevice {
    #[cfg(all(os_pipewire, feature = "pipewire"))]
    return default_output_device_from(&pipewire::driver::PipewireDriver::new().unwrap());
    #[cfg(all(not(all(os_pipewire, feature = "pipewire")), os_alsa))]
    return default_output_device_from(&alsa::AlsaDriver);
    #[cfg(os_coreaudio)]
    return default_output_device_from(&coreaudio::CoreAudioDriver);
    #[cfg(os_wasapi)]
    return default_output_device_from(&wasapi::WasapiDriver);
}
