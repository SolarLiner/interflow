use interflow::{prelude::pipewire::driver::PipewireDriver, AudioDevice, AudioDriver};

mod util;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

#[cfg(all(os_pipewire, feature = "pipewire"))]
fn main() -> Result {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::pipewire::driver::PipewireDriver;
    env_logger::init();
    let driver = PipewireDriver::new()?;
    enumerate_props(&driver)?;
    enumerate_devices(driver)?;
    Ok(())
}

fn enumerate_props(driver: &PipewireDriver) -> Result {
    eprintln!("Props:");

    for device in driver.list_devices()? {
        let Some(props) = device.props()? else {
            continue;
        };

        eprintln!("\t{:?}", device.device_type());
        eprintln!("\t\tdescription: {}", props.description);
        eprintln!("\t\tname: {}", props.name);
        eprintln!("\t\tnick: {}", props.nick);
    }

    Ok(())
}

#[cfg(not(all(os_pipewire, feature = "pipewire")))]
fn main() {
    println!("Pipewire feature is not enabled");
}
