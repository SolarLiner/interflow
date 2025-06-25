use interflow::{AudioDevice, AudioDriver, DeviceType};
use std::error::Error;

pub fn enumerate_devices<Driver: AudioDriver>(driver: Driver) -> Result<(), Box<dyn Error>>
where
    <Driver as AudioDriver>::Error: 'static,
{
    eprintln!("Driver name   : {}", Driver::DISPLAY_NAME);
    eprintln!("Driver version: {}", driver.version()?);
    eprintln!("Default device");
    for (s, device_type) in [("Input", DeviceType::INPUT), ("Output", DeviceType::OUTPUT)] {
        let device_type = device_type | DeviceType::PHYSICAL;
        eprint!("\t{s}:\t");
        if let Some(device) = driver.default_device(device_type)? {
            eprintln!("{}, {}", device.name(), device.description());
        } else {
            eprintln!("None");
        }
    }

    eprintln!("All devices");
    for device in driver.list_devices()? {
        eprintln!(
            "\t{}, {} ({:?})",
            device.name(),
            device.description(),
            device.device_type()
        );
    }
    Ok(())
}
