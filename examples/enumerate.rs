use interflow::core::{proxies::PlatformProxy, DeviceType};
use interflow::default_platform;

pub fn enumerate_devices(platform: &dyn PlatformProxy) -> anyhow::Result<()> {
    eprintln!("Driver name   : {}", platform.name());
    eprintln!("Default device");
    for (s, device_type) in [("Input", DeviceType::INPUT), ("Output", DeviceType::OUTPUT)] {
        let device_type = device_type | DeviceType::PHYSICAL;
        eprint!("\t{s}:\t");
        let device = platform.default_device(device_type)?;
    }

    eprintln!("All devices");
    for device in platform.list_devices()? {
        eprintln!("\t{} ({:?})", device.name(), device.device_type());
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let platform = default_platform();
    enumerate_devices(&*platform)?;
    Ok(())
}
