mod util;

#[cfg(feature = "pipewire")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::pipewire::driver::PipewireDriver;
    env_logger::init();
    enumerate_devices(PipewireDriver::new()?)?;
    Ok(())
}

#[cfg(not(feature = "pipewire"))]
fn main() {
    println!("Pipewire feature is not enabled");
}
