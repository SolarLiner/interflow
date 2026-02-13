use std::ptr::NonNull;

use pipewire::properties::Properties;
use pipewire::{core::Core, sys::pw_filter};

pub struct Filter {
    data: NonNull<pw_filter>,
}

impl Filter {
    pub fn new(core: &Core, name: impl AsRef<[u8]>, properties: Properties) -> Self {
        let core = core.as_raw();
        let name = {
            let s = CString::new(name.as_ref());
        };
        let filter = unsafe { pw_filter_new(core) };
    }
}
