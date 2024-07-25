use std::error::Error;
use interflow::{AudioDevice, AudioDriver};
use interflow::backends::alsa::AlsaDriver;

mod enumerate;

fn main() -> Result<(), Box<dyn Error>> {
    enumerate::enumerate_devices(AlsaDriver::default())
}

