use anyhow::Result;
use util::sine::SineWave;
use interflow::prelude::*;

mod util;

fn main() -> Result<()> {
    env_logger::init();

    let device = default_output_device();
    println!("Using device {}", device.name());
    let stream = device
        .default_output_stream(SineWave::new(440.0))
        .unwrap();
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject().unwrap();
    Ok(())
}

