use interflow::backends::coreaudio::CoreAudioDriver;
use interflow::{AudioDevice, AudioDriver};
use std::error::Error;

mod enumerate;

fn main() -> Result<(), Box<dyn Error>> {
    enumerate::enumerate_devices(CoreAudioDriver)
}
