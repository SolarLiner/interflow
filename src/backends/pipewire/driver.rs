use super::error::PipewireError;
use crate::backends::pipewire::device::PipewireDevice;
use crate::{AudioDevice, AudioDriver, DeviceType};
use libspa::pod::Value;
use log::kv::Source;
use pipewire::context::Context;
use pipewire::core::Core;
use pipewire::main_loop::MainLoop;
use pipewire::types::ObjectType;
use std::borrow::Cow;
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::sync_channel;

pub struct PipewireDriver {
    __init: PhantomData<()>,
}

impl AudioDriver for PipewireDriver {
    type Error = PipewireError;
    type Device = PipewireDevice;
    const DISPLAY_NAME: &'static str = "Pipewire";

    fn version(&self) -> Result<Cow<str>, Self::Error> {
        // TODO: Figure out how to get version
        Ok(Cow::Borrowed("unkonwn"))
    }

    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error> {
        Ok(Some(PipewireDevice {
            target_node: None,
            device_type,
        }))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        let (tx, rx) = sync_channel(16);
        let _listener = run_sync(move |_, core| {
            core.get_registry()
                .unwrap()
                .add_listener_local()
                .global(move |obj| {
                    log::debug!("Object id: {:?}", obj.id);
                    log::debug!("Object type: {:?}", obj.type_);
                    log::debug!("Object props: {:?}", obj.props);
                    if obj.type_ == ObjectType::Node {
                        if let Some(props) = obj.props {
                            if let Some(media_class) = props.get("media.class".into()) {
                                log::debug!("\tMedia class: {:?}", media_class);
                                tx.send((obj.id, is_input(media_class), is_output(media_class)))
                                    .unwrap();
                            }
                        }
                    }
                })
                .register()
        })?;
        Ok(rx.into_iter().filter_map(|(id, is_input, is_output)| {
            Some(PipewireDevice {
                target_node: Some(id),
                device_type: match (is_input, is_output) {
                    (true, true) => DeviceType::Duplex,
                    (true, _) => DeviceType::Input,
                    (_, true) => DeviceType::Output,
                    _ => return None,
                },
            })
        }))
    }
}

fn is_input(media_class: &str) -> bool {
    let str = media_class.to_lowercase();
    ["input", "source", "capture"]
        .iter()
        .any(|s| str.contains(s))
}

fn is_output(str: &str) -> bool {
    let str = str.to_lowercase();
    ["output", "sink", "playback"]
        .iter()
        .any(|s| str.contains(s))
}

impl PipewireDriver {
    /// Initialize the Pipewire driver.
    pub fn new() -> Result<Self, PipewireError> {
        pipewire::init();
        Ok(Self {
            __init: PhantomData,
        })
    }
}

fn run_sync<R>(run: impl FnOnce(Context, Core) -> R) -> Result<R, PipewireError> {
    let main_loop = MainLoop::new(None)?;
    let context = Context::new(&main_loop)?;
    let core = context.connect(None)?;
    let done = Rc::new(Cell::new(false));

    let pending = core.sync(0)?;
    let _listener = core
        .add_listener_local()
        .done({
            let done = done.clone();
            let main_loop = main_loop.clone();
            move |id, seq| {
                log::debug!("Event {id}: seq {}", seq.seq());
                if id == pipewire::core::PW_ID_CORE && seq == pending {
                    done.set(true);
                    main_loop.quit();
                }
            }
        })
        .register();
    let result = run(context.clone(), core);

    while !done.get() {
        main_loop.run();
    }

    Ok(result)
}
