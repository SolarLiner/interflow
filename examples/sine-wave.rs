use anyhow::Result;
use interflow::prelude::*;
use util::sine::SineWave;

mod util;

fn main() -> Result<()> {
    env_logger::init();
    let (callback, display) = util::prepare_display(SineWave::new(440.0));
    let stream = default_stream(DeviceType::OUTPUT | DeviceType::PHYSICAL, callback)?;

    display()?;

    let _ = stream.eject()?;
    Ok(())
}
