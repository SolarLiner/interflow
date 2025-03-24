use crate::backends::pipewire::error::PipewireError;
use crate::{AudioDevice, Channel, DeviceType, StreamConfig};
use pipewire::core::Core;
use std::borrow::Cow;

pub struct PipewireDevice {
    pub(super) target_node: Option<u32>,
    pub device_type: DeviceType,
}

impl AudioDevice for PipewireDevice {
    type Error = PipewireError;

    fn name(&self) -> Cow<str> {
        // TODO: Return device name
        Cow::Borrowed("unknown")
    }

    fn device_type(&self) -> DeviceType {
        todo!()
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        []
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        todo!()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        Some([])
    }
}
