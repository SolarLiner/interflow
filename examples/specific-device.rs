use crate::util::sine::SineWave;
use anyhow::Context;
use dialoguer::console::Term;
use dialoguer::FuzzySelect;
use interflow::prelude::*;

mod util;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let term = Term::stdout();
    let platform = default_platform();
    let devices = platform
        .list_devices_matching(DeviceType::OUTPUT)
        .context("Cannot list devices")?;
    let Some(index) = FuzzySelect::new()
        .default(0)
        .items(devices.iter().map(|dev| dev.name()))
        .interact_on_opt(&term)
        .context("Cannot display list")?
    else {
        return Ok(());
    };

    let (callback, display) = util::prepare_display(SineWave::new(440.0));

    let handle = devices[index]
        .default_stream(DeviceType::OUTPUT, callback)
        .context("Cannot create stream")?;
    display()?;
    let _ = handle.eject()?;
    Ok(())
}
