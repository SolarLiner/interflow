use interflow::prelude::*;
use interflow_core::collect::get_registry;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    for ctor in get_registry() {
        let Some(platform) = ctor() else { continue };
        println!("Platform: {}", platform.name());
        if let Ok(device) = platform.default_device(DeviceType::INPUT) {
            println!("\tDefault input device: {}", device.name());
        }
        if let Ok(device) = platform.default_device(DeviceType::OUTPUT) {
            println!("\tDefault output device: {}", device.name());
        }
    }
    Ok(())
}