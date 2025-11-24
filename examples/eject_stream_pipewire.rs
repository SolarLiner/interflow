//! This example showcases the problem with ejecting stopped pipewire streams described in
//! https://github.com/SolarLiner/interflow/issues/105
//!
//! It would be best as an integration test, but it has nontrivial prerequisites on the environment:
//! - running PipeWire daemon
//! - at least one PipeWire audio output device
//! - the `pw-link` program installed (bundled with pipewire)

use interflow::prelude::*;
use std::ops::Deref;
use std::thread;
use util::sine::SineWave;

mod util;

#[cfg(all(os_pipewire, feature = "pipewire"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use interflow::prelude::pipewire::driver::PipewireDriver;
    use std::{process, time::Duration};

    env_logger::init();

    let driver = PipewireDriver::new()?;

    // Select the highest-priority output device. Use this rather than `driver.default_device()`
    // because we need its real name for disconnecting it below.
    let devices = driver.list_devices()?;
    let mut device = devices
        .into_iter()
        .filter(|d| d.device_type().is_output())
        .max_by_key(device_session_priority)
        .expect("No output PipeWire devices?");
    println!("Using device {}", device.name());

    let config = device.default_output_config()?;
    device.with_stream_name("Interflow eject test 1");
    let stream_1 = device.create_output_stream(config, SineWave::new(440.0))?;

    println!("Playing sine wave for 1 second, then ejecting");
    thread::sleep(Duration::from_secs(1));
    let callback = stream_1.eject().unwrap();

    println!("Playing sine wave for another second in a new stream but old callback");
    let stream_2 = device.create_output_stream(config, callback)?;
    thread::sleep(Duration::from_secs(1));

    // Disconnect our node from the device node. Call external program, doing this programmatically
    // using pipewire-rs would be much more involved.
    let mut command = process::Command::new("pw-link");
    command
        .arg("--disconnect")
        .arg("eject_stream_pipewire")
        .arg(device.name().deref());
    println!("Disconnecting playback pipewire node from its device using {command:?}");
    let status = command.status()?;
    assert!(status.success());

    println!("Ejecting the callback from the new stream");
    // The hang occurred right in this call
    stream_2.eject()?;

    println!("Exiting cleanly");
    Ok(())
}

#[cfg(all(os_pipewire, feature = "pipewire"))]
fn device_session_priority(device: &pipewire::device::PipewireDevice) -> Option<i32> {
    let properties = device
        .properties()
        .expect("Cannot get pipewire device properties")?;

    let priority_property = properties.get("priority.session")?;
    let priority = priority_property
        .parse()
        .expect("Cannot parse priority.session as i32");
    Some(priority)
}
