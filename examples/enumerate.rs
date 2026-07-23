use interflow::prelude::*;

pub fn enumerate_devices(platform: &dyn PlatformProxy) -> anyhow::Result<()> {
    eprintln!("Driver name   : {}", platform.name());
    eprintln!("Default device");
    for (s, device_type) in [("Input", DeviceType::INPUT), ("Output", DeviceType::OUTPUT)] {
        let device_type = device_type | DeviceType::PHYSICAL;
        eprint!("\t{s}:\t");
        let Ok(device) = platform.default_device(device_type) else {
            println!("no default device");
            continue;
        };
        println!("{}", device.name());
    }

    eprintln!("\nAll devices");
    for device in platform.list_devices()? {
        eprintln!("\t{} ({:?})", device.name(), device.device_type());
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let platform = default_platform();
    enumerate_devices(&*platform)?;
    Ok(())
}
