//! # Backends
//!
//! Home of the various backends supported by the library.
//!
//! Each backend is provided in its own submodule. Types should be public so that the user isn't
//! limited to going through the main API if they want to choose a specific backend.

use crate::{
    AudioDriver, AudioDuplexDevice, AudioDuplexDriver, AudioInputDevice, AudioOutputDevice,
    DeviceType,
};

#[cfg(unsupported)]
compile_error!("Unsupported platform (supports ALSA, CoreAudio, and WASAPI)");

#[cfg(os_alsa)]
pub mod alsa;

#[cfg(os_coreaudio)]
pub mod coreaudio;

#[cfg(os_wasapi)]
pub mod wasapi;

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
/// | **Platform** | **Driver** |
/// |:------------:|:----------:|
/// |     Linux    |    ALSA    |
/// |     macOS    |  CoreAudio |
/// |    Windows   |   WASAPI   |
#[cfg(any(os_alsa, os_coreaudio, os_wasapi))]
#[allow(clippy::needless_return)]
pub fn default_driver() -> impl AudioDriver {
    #[cfg(os_alsa)]
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
        .default_device(DeviceType::Input)
        .expect("Audio driver error")
        .expect("No default device found")
}

/// Default input device from the default driver for this platform.
///
/// "Default" here means both in terms of platform support but also can include runtime selection.
/// Therefore, it is better to use this method directly rather than first getting the default
/// driver from [`default_driver`].
#[cfg(any(os_alsa, os_coreaudio, os_wasapi))]
#[allow(clippy::needless_return)]
pub fn default_input_device() -> impl AudioInputDevice {
    #[cfg(os_alsa)]
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
        .default_device(DeviceType::Output)
        .expect("Audio driver error")
        .expect("No default device found")
}

/// Default output device from the default driver for this platform.
///
/// "Default" here means both in terms of platform support but also can include runtime selection.
/// Therefore, it is better to use this method directly rather than first getting the default
/// driver from [`default_driver`].
#[cfg(any(os_alsa, os_coreaudio, os_wasapi))]
#[allow(clippy::needless_return)]
pub fn default_output_device() -> impl AudioOutputDevice {
    #[cfg(os_alsa)]
    return default_output_device_from(&alsa::AlsaDriver);
    #[cfg(os_coreaudio)]
    return default_output_device_from(&coreaudio::CoreAudioDriver);
    #[cfg(os_wasapi)]
    return default_output_device_from(&wasapi::WasapiDriver);
}

/// Default duplex device from the default driver of this platform.
///
/// "Default" here means both in terms of platform support but also can include runtime selection.
/// Therefore, it is better to use this method directly rather than first getting the default
/// driver from [`default_driver`].
#[allow(clippy::non_minimal_cfg)]
#[allow(clippy::needless_return)]
#[cfg(any(os_alsa))]
pub fn default_duplex_device() -> impl AudioDuplexDevice {
    #[cfg(os_alsa)]
    return default_duplex_device_from(&alsa::AlsaDriver);
}

/// Returns the default duplex device for the given audio driver.
///
/// The default device is usually the one the user has selected in its system settings.
pub fn default_duplex_device_from<D: AudioDuplexDriver>(driver: &D) -> D::DuplexDevice
where
    D::Device: AudioInputDevice + AudioOutputDevice,
{
    driver
        .default_duplex_device()
        .expect("Audio driver error")
        .unwrap_or_else(|| {
            driver
                .device_from_input_output(
                    default_input_device_from(driver),
                    default_output_device_from(driver),
                )
                .expect("Audio driver error")
        })
}
