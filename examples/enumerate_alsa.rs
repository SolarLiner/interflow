use crate::util::enumerate::enumerate_duplex_devices;

mod util;

#[cfg(os_alsa)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::alsa::AlsaDriver;

    env_logger::init();

    enumerate_devices(AlsaDriver)?;
    enumerate_duplex_devices(AlsaDriver)?;
    Ok(())
}

#[cfg(not(os_alsa))]
fn main() {
    println!("ALSA driver is not available on this platform");
}
