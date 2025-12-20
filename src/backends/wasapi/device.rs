use super::{error, stream};
use crate::backends::wasapi::stream::WasapiStream;
use crate::prelude::wasapi::stream::{FindSupportedConfig, StreamDirection};
use crate::prelude::wasapi::util::WasapiMMDevice;
use crate::{AudioCallback, AudioDevice, Channel, DeviceType, StreamConfig};
use std::borrow::Cow;
use windows::Win32::Media::Audio;

/// Type of devices available from the WASAPI driver.
#[derive(Debug, Clone)]
pub struct WasapiDevice {
    device: WasapiMMDevice,
    device_type: DeviceType,
}

impl WasapiDevice {
    pub(crate) fn new(device: Audio::IMMDevice, device_type: DeviceType) -> Self {
        WasapiDevice {
            device: WasapiMMDevice::new(device),
            device_type,
        }
    }
}

impl AudioDevice for WasapiDevice {
    type Error = error::WasapiError;
    type StreamHandle<Callback: AudioCallback> = WasapiStream<Callback>;

    fn name(&self) -> Cow<str> {
        match self.device.name() {
            Some(std) => Cow::Owned(std),
            None => {
                eprintln!("Cannot get audio device name");
                Cow::Borrowed("<unknown>")
            }
        }
    }

    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        []
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        if self.device_type.is_duplex() || !self.device_type.is_physical() {
            return false;
        }
        FindSupportedConfig {
            config,
            device: &self.device,
            is_output: self.device_type.is_output(),
        }
        .supported_config()
        .is_some()
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_client = self.device.activate::<Audio::IAudioClient>()?;
        let format = unsafe { audio_client.GetMixFormat()?.read_unaligned() };
        let frame_size = unsafe { audio_client.GetBufferSize() }
            .map(|i| i as usize)
            .ok();
        let (input_channels, output_channels) = if self.device_type.is_input() {
            (format.nChannels as _, 0)
        } else {
            (0, format.nChannels as _)
        };
        Ok(StreamConfig {
            sample_rate: format.nSamplesPerSec as _,
            input_channels,
            output_channels,
            buffer_size_range: (frame_size, frame_size),
            exclusive: false,
        })
    }

    fn create_stream<Callback: 'static + Send + AudioCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        Ok(WasapiStream::new(
            self.device.clone(),
            StreamDirection::try_from(self.device_type)?,
            stream_config,
            callback,
        ))
    }
}

/// An iterable collection WASAPI devices.
pub(crate) struct WasapiDeviceList {
    pub(crate) collection: Audio::IMMDeviceCollection,
    pub(crate) total_count: u32,
    pub(crate) next_item: u32,
    pub(crate) device_type: DeviceType,
}

unsafe impl Send for WasapiDeviceList {}

unsafe impl Sync for WasapiDeviceList {}

impl Iterator for WasapiDeviceList {
    type Item = WasapiDevice;

    fn next(&mut self) -> Option<WasapiDevice> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
            self.next_item += 1;
            Some(WasapiDevice::new(device, self.device_type))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rest = (self.total_count - self.next_item) as usize;
        (rest, Some(rest))
    }
}

impl ExactSizeIterator for WasapiDeviceList {}
