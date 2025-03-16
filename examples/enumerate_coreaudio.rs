mod util;

#[cfg(os_coreaudio)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::coreaudio::CoreAudioDriver;

    enumerate_devices(CoreAudioDriver)
}

#[cfg(not(os_coreaudio))]
fn main() {
    println!("CoreAudio is not available on this platform");
}
