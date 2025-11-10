use crate::backends::pipewire::error::PipewireError;
use crate::DeviceType;
use libspa::utils::dict::DictRef;
use pipewire::context::Context;
use pipewire::main_loop::MainLoop;
use pipewire::registry::GlobalObject;
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::mpsc;

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

/// A little helper that holds user's callback and sends it out using a channel when it goes out of
/// scope. Dereferences to `Callback`, including mutably.
pub(super) struct CallbackHolder<Callback> {
    /// Invariant: `callback` is always `Some`, except in the second half of the [`Drop`] impl.
    callback: Option<Callback>,
    tx: mpsc::SyncSender<Callback>,
}

impl<Callback> CallbackHolder<Callback> {
    /// Returns a pair (self, rx), where `rx` should be used to fetch the callback when the holder
    /// goes out of scope.
    pub(super) fn new(callback: Callback) -> (Self, mpsc::Receiver<Callback>) {
        // Our first choice would be and `rtrb` channel, but that doesn't allow receiver to wait
        // for a message, which we need. It doesn't matter, we use a channel of capacity 1 and
        // we only use it exactly once, it never blocks in this case.
        let (tx, rx) = mpsc::sync_channel(1);
        let myself = Self {
            callback: Some(callback),
            tx,
        };
        (myself, rx)
    }
}

impl<Callback> Deref for CallbackHolder<Callback> {
    type Target = Callback;

    fn deref(&self) -> &Callback {
        self.callback
            .as_ref()
            .expect("never None outside destructor")
    }
}

impl<Callback> DerefMut for CallbackHolder<Callback> {
    fn deref_mut(&mut self) -> &mut Callback {
        self.callback
            .as_mut()
            .expect("never None outside destructor")
    }
}

impl<Callback> Drop for CallbackHolder<Callback> {
    fn drop(&mut self) {
        let callback = self.callback.take().expect("never None outside destructor");
        match self.tx.try_send(callback) {
            Ok(()) => (),
            Err(mpsc::TrySendError::Full(_)) => {
                panic!("The channel in CallbackHolder should be never full")
            }
            Err(mpsc::TrySendError::Disconnected(_)) => log::warn!(
                "Channel in CallbackHolder is disconnected, did PipeWire main loop already exit?"
            ),
        }
    }
}

/// Allows you to send to value to a black hole. It keeps at alive as long as it is in scope, but
/// you cannot get the value back in any way.
pub struct BlackHole<T>(T);

impl<T> BlackHole<T> {
    pub fn new(wrapped: T) -> Self {
        Self(wrapped)
    }
}
