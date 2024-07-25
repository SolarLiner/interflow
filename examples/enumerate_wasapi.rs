mod util;

#[cfg(os_wasapi)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::wasapi::WasapiDriver;
    enumerate_devices(WasapiDriver)
}

#[cfg(not(os_wasapi))]
fn main() {
    println!("WASAPI driver is not available on this platform");
}

