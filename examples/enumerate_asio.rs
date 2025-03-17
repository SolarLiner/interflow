mod util;

#[cfg(os_asio)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crate::util::enumerate::enumerate_devices;
    use interflow::backends::asio::AsioDriver;
    enumerate_devices(AsioDriver::new()?)?;
    Ok(())
}

#[cfg(not(os_asio))]
fn main() {
    println!("ASIO driver is not available on this platform");
}
