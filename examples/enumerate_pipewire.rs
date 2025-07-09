use interflow::{prelude::pipewire::driver::PipewireDriver, AudioDevice, AudioDriver};

mod util;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

#[cfg(all(os_pipewire, feature = "pipewire"))]
fn main() -> Result {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::pipewire::driver::PipewireDriver;
    env_logger::init();
    let driver = PipewireDriver::new()?;
    enumerate_properties(&driver)?;
    enumerate_devices(driver)?;
    Ok(())
}

fn enumerate_properties(driver: &PipewireDriver) -> Result {
    eprintln!("Properties:");

    for device in driver.list_devices()? {
        eprintln!("\t{}", device.name());

        let Some(properties) = device.properties()? else {
            eprintln!("\tNo properties found");
            continue;
        };

        for (key, value) in properties.dict().iter() {
            eprintln!("\t\t{key}: {value}")
        }
    }

    Ok(())
}

#[cfg(not(all(os_pipewire, feature = "pipewire")))]
fn main() {
    println!("Pipewire feature is not enabled");
}
