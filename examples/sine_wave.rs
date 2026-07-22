use anyhow::Result;
use interflow::core::DeviceType;
use interflow_core::stream::StreamHandle;
use util::sine::SineWave;

mod util;

fn main() -> Result<()> {
    env_logger::init();

    let stream = interflow::default_stream(
        DeviceType::OUTPUT | DeviceType::PHYSICAL,
        SineWave::new(440.0),
    )?;
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject()?;
    println!("Stream ejected");
    Ok(())
}
