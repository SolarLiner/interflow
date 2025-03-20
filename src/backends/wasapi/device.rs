use super::{error, stream};
use crate::backends::wasapi::stream::WasapiStream;
use crate::channel_map::Bitset;
use crate::device::Channel;
use crate::device::{AudioDevice, AudioInputDevice, AudioOutputDevice, DeviceType};
use crate::prelude::wasapi::util::WasapiMMDevice;
use crate::stream::{AudioInputCallback, AudioOutputCallback, StreamConfig};
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

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        match self.device_type {
            DeviceType::Output => stream::is_output_config_supported(self.device.clone(), config),
            _ => false,
        }
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }
}

impl AudioInputDevice for WasapiDevice {
    type StreamHandle<Callback: AudioInputCallback> = WasapiStream<Callback>;

    fn input_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_client = self.device.activate::<Audio::IAudioClient>()?;
        let format = unsafe { audio_client.GetMixFormat()?.read_unaligned() };
        let frame_size = unsafe { audio_client.GetBufferSize() }
            .map(|i| i as usize)
            .ok();
        Ok(StreamConfig {
            channels: 0u32.with_indices(0..format.nChannels as _),
            exclusive: false,
            samplerate: format.nSamplesPerSec as _,
            buffer_size_range: (frame_size, frame_size),
        })
    }

    fn create_input_stream<Callback: 'static + Send + AudioInputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        Ok(WasapiStream::new_input(
            self.device.clone(),
            stream_config,
            callback,
        ))
    }
}

impl AudioOutputDevice for WasapiDevice {
    type StreamHandle<Callback: AudioOutputCallback> = WasapiStream<Callback>;

    fn output_channel_map(&self) -> impl Iterator<Item = Channel> {
        [].into_iter()
    }

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_client = self.device.activate::<Audio::IAudioClient>()?;
        let format = unsafe { audio_client.GetMixFormat()?.read_unaligned() };
        let frame_size = unsafe { audio_client.GetBufferSize() }
            .map(|i| i as usize)
            .ok();
        Ok(StreamConfig {
            channels: 0u32.with_indices(0..format.nChannels as _),
            exclusive: false,
            samplerate: format.nSamplesPerSec as _,
            buffer_size_range: (frame_size, frame_size),
        })
    }

    fn create_output_stream<Callback: 'static + Send + AudioOutputCallback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        Ok(WasapiStream::new_output(
            self.device.clone(),
            stream_config,
            callback,
        ))
    }
}

/// An iterable collection WASAPI devices.
pub struct WasapiDeviceList {
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
