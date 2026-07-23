use anyhow::Result;
use interflow::prelude::*;
use util::sine::SineWave;

mod util;

fn main() -> Result<()> {
    env_logger::init();

    let stream = default_stream(
        DeviceType::OUTPUT | DeviceType::PHYSICAL,
        SineWave::new(440.0),
    )?;
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject()?;
    println!("Stream ejected");
    Ok(())
}
