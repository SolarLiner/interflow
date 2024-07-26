use std::error::Error;

mod util;

#[cfg(os_coreaudio)]
fn main() -> Result<(), Box<dyn Error>> {
    use interflow::backends::coreaudio::CoreAudioDriver;
    use crate::util::enumerate::enumerate_devices;
    
    enumerate_devices(CoreAudioDriver)
}

#[cfg(not(os_coreaudio))]
fn main() {
    println!("CoreAudio is not available on this platform");
}
