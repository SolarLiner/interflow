#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use std::rc::Rc;

use core::{stream, DeviceType};
pub use interflow_core as core;
use interflow_core::proxies::CreateStreamExt;

pub mod backends;

/// Prelude module. Import all with `interflow::prelude::*`.
pub mod prelude {
    pub use super::{default_device, default_platform, default_stream};
    pub use interflow_core::prelude::*;
    pub use interflow_coreaudio::prelude::*;
}

/// Return the default platform.
/// The platform is selected automatically based on your available and enabled backends.
#[allow(unreachable_code)]
pub fn default_platform() -> core::proxies::DynPlatform {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return Rc::new(interflow_coreaudio::platform::Platform);
    todo!("null backend")
}

/// Return the default device, using the default platform as returned by [`default_platform`].
pub fn default_device(device_type: DeviceType) -> anyhow::Result<core::proxies::DynDevice> {
    default_platform().default_device(device_type)
}

/// Create a stream using the default device as returned by [`default_device`].
pub fn default_stream<Callback: 'static + stream::Callback>(
    device_type: DeviceType,
    callback: Callback,
) -> anyhow::Result<core::proxies::StreamHandle<Callback>> {
    let device = default_device(device_type)?;
    Ok(device.default_stream(device_type, callback)?)
}
