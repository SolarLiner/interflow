use std::error::Error;

mod util;

#[cfg(os_alsa)]
fn main() -> Result<(), Box<dyn Error>> {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::alsa::AlsaDriver;
    
    env_logger::init();
    
    enumerate_devices(AlsaDriver::default())
}

#[cfg(not(os_alsa))]
fn main() {
    println!("ALSA driver is not available on this platform");
}

