use crate::backends::pipewire::error::PipewireError;
use crate::DeviceType;
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
    let mut device_type = DeviceType::empty();
    device_type.set(DeviceType::INPUT, is_input(media_class));
    device_type.set(DeviceType::OUTPUT, is_output(media_class));
    Some(device_type)
}

fn get_device_object_serial(object: &GlobalObject<&DictRef>) -> Option<String> {
    let object_serial = object.props?.get("object.serial")?;
    Some(object_serial.to_owned())
}

pub fn get_devices() -> Result<Vec<(u32, DeviceType, String)>, PipewireError> {
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

                let device_type = get_device_type(global);
                let object_serial = get_device_object_serial(global);

                if let (Some(device_type), Some(object_serial)) = (device_type, object_serial) {
                    data.borrow_mut()
                        .push((global.id, device_type, object_serial));
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
