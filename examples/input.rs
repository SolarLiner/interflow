use crate::util::noop::Noop;
use interflow::prelude::*;

mod util;

fn main() -> anyhow::Result<()> {
    let (callback, display) = util::prepare_display(Noop);
    let handle = default_stream(DeviceType::INPUT, callback)?;
    display()?;
    let _ = handle.eject()?;
    Ok(())
}
