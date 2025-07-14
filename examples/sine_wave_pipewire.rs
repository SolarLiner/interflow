use interflow::prelude::*;
use util::sine::SineWave;

mod util;

#[cfg(all(os_pipewire, feature = "pipewire"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use interflow::prelude::pipewire::driver::PipewireDriver;

    env_logger::init();

    let driver = PipewireDriver::new()?;
    let mut device = driver.default_device(DeviceType::OUTPUT)?.unwrap();
    println!("Using device {}", device.name());

    let config = device.default_output_config()?;
    device.with_stream_name("Interflow sine wave");
    let properties = [("node.custom-property".into(), "interflow".into())].into();
    device.with_stream_properties(properties);
    let stream = device.create_output_stream(config, SineWave::new(440.0))?;

    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject().unwrap();
    Ok(())
}
