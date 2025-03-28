use crate::{backends::pipewire::error::PipewireError, device::DeviceType};
use libspa::utils::dict::DictRef;
use pipewire::context::Context;
use pipewire::main_loop::MainLoop;
use pipewire::registry::GlobalObject;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn get_device_type(object: &GlobalObject<&DictRef>) -> Option<DeviceType> {
    fn is_input(media_class: &str) -> bool {
        let str = media_class.trim().to_lowercase();
        str == "audio/source"
    }

    fn is_output(str: &str) -> bool {
        let str = str.trim().to_lowercase();
        str == "audio/sink"
    }

    let media_class = object.props?.get("media.class")?;
    Some(match (is_input(media_class), is_output(media_class)) {
        (true, true) => DeviceType::Duplex,
        (true, _) => DeviceType::Input,
        (_, true) => DeviceType::Output,
        _ => return None,
    })
}

pub fn get_devices() -> Result<Vec<(u32, DeviceType)>, PipewireError> {
    let mainloop = MainLoop::new(None)?;
    let context = Context::new(&mainloop)?;
    let core = context.connect(None)?;
    let registry = core.get_registry()?;

    // To comply with Rust's safety rules, we wrap this variable in an `Rc` and  a `Cell`.
    let done = Rc::new(Cell::new(false));

    // Create new reference for each variable so that they can be moved into the closure.
    let done_clone = done.clone();
    let loop_clone = mainloop.clone();

    // Trigger the sync event. The server's answer won't be processed until we start the main loop,
    // so we can safely do this before setting up a callback. This lets us avoid using a Cell.
    let pending = core.sync(0)?;

    let _listener_core = core
        .add_listener_local()
        .done(move |id, seq| {
            log::debug!("[Core/Done] id: {id} seq: {}", seq.seq());
            if id == pipewire::core::PW_ID_CORE && seq == pending {
                done_clone.set(true);
                loop_clone.quit();
            }
        })
        .register();

    let data = Rc::new(RefCell::new(Vec::new()));
    let _listener_reg = registry
        .add_listener_local()
        .global({
            let data = data.clone();
            move |global| {
                log::debug!(
                    "object: id:{} type:{}/{}",
                    global.id,
                    global.type_,
                    global.version
                );
                if let Some(device_type) = get_device_type(global) {
                    data.borrow_mut().push((global.id, device_type));
                }
            }
        })
        .register();

    while !done.get() {
        mainloop.run();
    }
    drop(_listener_core);
    drop(_listener_reg);
    Ok(Rc::into_inner(data).unwrap().into_inner())
}

pub fn get_default_node_for(device_type: DeviceType) -> u32 {
    match device_type {
        DeviceType::Input => 0,
        DeviceType::Output => 1,
        DeviceType::Duplex => 2,
    }
}
