use crate::util::MMDevice;
use interflow_core::device::{ResolvedStreamConfig, StreamConfig};
use interflow_core::stream;
use interflow_core::traits::{ExtensionProvider, Selector};
use interflow_core::{device, DeviceType};
use std::borrow::Cow;
use windows::Win32::Media::Audio;
use windows::Win32::Media::Audio::{IAudioClient, IAudioClient3};

#[derive(Debug, Clone)]
pub struct Device {
    pub(crate) handle: MMDevice,
    pub(crate) device_type: DeviceType,
}

impl ExtensionProvider for Device {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl device::Device for Device {
    type Error = crate::Error;
    type StreamHandle<Callback: stream::Callback> = ();

    fn name(&self) -> Cow<'_, str> {
        Cow::Owned(self.handle.name())
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        self.get_mix_format_iac3()
            .or_else(|_| self.get_mix_format())
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        todo!()
    }

    fn create_stream<Callback: 'static + Send + stream::Callback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        todo!()
    }
}

impl Device {
    fn get_mix_format(&self) -> Result<StreamConfig, crate::Error> {
        let client = self.handle.activate::<IAudioClient>()?;
        let mix_format = unsafe { client.GetMixFormat() }?;
        let format = unsafe { mix_format.read_unaligned() };
        let channels = format.nChannels as usize;
        let input_channels = if self.device_type.is_input() {
            channels
        } else {
            0
        };
        let output_channels = if self.device_type.is_output() {
            channels
        } else {
            0
        };
        Ok(StreamConfig {
            sample_rate: format.nSamplesPerSec as _,
            input_channels,
            output_channels,
            buffer_size_range: (None, None),
            exclusive: false,
        })
    }

    fn get_mix_format_iac3(&self) -> Result<StreamConfig, crate::Error> {
        let client = self.handle.activate::<IAudioClient3>()?;
        let mut period_default = 0u32;
        let mut period_min = 0u32;
        let mut period_max = 0u32;
        let format = unsafe { client.GetMixFormat() }?;
        unsafe {
            let mut _fundamental_period = 0u32;
            client.GetSharedModeEnginePeriod(
                format.cast_const(),
                &mut period_default,
                &mut _fundamental_period,
                &mut period_min,
                &mut period_max,
            )?;
        }
        let format = unsafe { format.read_unaligned() };
        let channels = format.nChannels as usize;
        let input_channels = if self.device_type.is_input() {
            channels
        } else {
            0
        };
        let output_channels = if self.device_type.is_output() {
            channels
        } else {
            0
        };
        Ok(StreamConfig {
            sample_rate: format.nSamplesPerSec as _,
            input_channels,
            output_channels,
            buffer_size_range: (Some(period_min as usize), Some(period_max as usize)),
            exclusive: false,
        })
    }
}

/// An iterable collection WASAPI devices.
pub(crate) struct DeviceList {
    pub(crate) collection: Audio::IMMDeviceCollection,
    pub(crate) total_count: u32,
    pub(crate) next_item: u32,
    pub(crate) device_type: DeviceType,
}

unsafe impl Send for DeviceList {}

unsafe impl Sync for DeviceList {}

impl Iterator for DeviceList {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
            self.next_item += 1;
            Some(Device {
                handle: MMDevice::new(device),
                device_type: self.device_type,
            })
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rest = (self.total_count - self.next_item) as usize;
        (rest, Some(rest))
    }
}

impl ExactSizeIterator for DeviceList {}
