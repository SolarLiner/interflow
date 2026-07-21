use anyhow::Result;
use interflow::core::{proxies::DynDevice, DeviceType};
use util::sine::SineWave;

mod util;

fn main() -> Result<()> {
    env_logger::init();

    let device: DynDevice = todo!("default_device");
    println!("Using device {}", device.name());
    let stream = device
        .default_stream(DeviceType::OUTPUT, SineWave::new(440.0))
        .unwrap();
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject().unwrap();
    Ok(())
}
