use super::{error, stream};
use crate::backends::wasapi::stream::WasapiStream;
use crate::channel_map::Bitset;
use crate::prelude::wasapi::util::WasapiMMDevice;
use crate::{
    AudioDevice, AudioInputCallback, AudioInputDevice, AudioOutputCallback, AudioOutputDevice,
    Channel, DeviceType, StreamConfig,
};
use std::borrow::Cow;
use std::ptr;
use windows::core::Interface;
use windows::Win32::Media::Audio;
use windows::Win32::System::Com::CoTaskMemFree;

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

    fn channel_map(&self) -> impl IntoIterator<Item = Channel> {
        []
    }

    fn is_config_supported(&self, config: &StreamConfig) -> bool {
        self.device_type.contains(DeviceType::OUTPUT)
            && stream::is_output_config_supported(self.device.clone(), config)
    }

    fn enumerate_configurations(&self) -> Option<impl IntoIterator<Item = StreamConfig>> {
        None::<[StreamConfig; 0]>
    }

    fn buffer_size_range(&self) -> Result<(Option<usize>, Option<usize>), Self::Error> {
        let audio_client = self.device.activate::<Audio::IAudioClient>()?;

        // A local RAII wrapper to ensure CoTaskMemFree is always called.
        struct ComWaveFormat(*mut Audio::WAVEFORMATEX);
        impl Drop for ComWaveFormat {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { CoTaskMemFree(Some(self.0 as *const _)) };
                }
            }
        }

        let format_to_use = (|| -> Result<Audio::WAVEFORMATEX, error::WasapiError> {
            unsafe {
                // Get the mix format, now managed by our RAII wrapper.
                let format_ptr = ComWaveFormat(audio_client.GetMixFormat()?);

                let mut closest_match_ptr: *mut Audio::WAVEFORMATEX = ptr::null_mut();
                let res = audio_client.IsFormatSupported(
                    Audio::AUDCLNT_SHAREMODE_SHARED,
                    &*format_ptr.0,
                    Some(&mut closest_match_ptr),
                );

                if res.is_ok() {
                    // The original format is supported.
                    return Ok(format_ptr.0.read_unaligned());
                }

                // Wrap the returned suggestion in our RAII struct as well.
                let closest_match = ComWaveFormat(closest_match_ptr);
                if !closest_match.0.is_null() {
                    return Ok(closest_match.0.read_unaligned());
                }

                res.ok()?;
                unreachable!();
            }
        })()?;

        let samplerate = format_to_use.nSamplesPerSec;

        // Attempt IAudioClient3/IAudioClient2 to get the buffer size range.
        if let Ok(client) = audio_client.cast::<Audio::IAudioClient3>() {
            let mut min_buffer_duration = 0;
            let mut max_buffer_duration = 0;
            // Based on the stream implementation, we assume event driven mode.
            let event_driven = true;
            unsafe {
                client.GetBufferSizeLimits(
                    &format_to_use,
                    event_driven.into(),
                    &mut min_buffer_duration,
                    &mut max_buffer_duration,
                )?;
            }
            // Convert from 100-nanosecond units to frames.
            let to_frames = |period| (period as u64 * samplerate as u64 / 10_000_000) as usize;
            return Ok((
                Some(to_frames(min_buffer_duration)),
                Some(to_frames(max_buffer_duration)),
            ));
        }
        if let Ok(client) = audio_client.cast::<Audio::IAudioClient2>() {
            let mut min_buffer_duration = 0;
            let mut max_buffer_duration = 0;
            let event_driven = true;
            unsafe {
                client.GetBufferSizeLimits(
                    &format_to_use,
                    event_driven.into(),
                    &mut min_buffer_duration,
                    &mut max_buffer_duration,
                )?;
            }
            let to_frames = |period| (period as u64 * samplerate as u64 / 10_000_000) as usize;
            return Ok((
                Some(to_frames(min_buffer_duration)),
                Some(to_frames(max_buffer_duration)),
            ));
        }

        // Fallback to GetBufferSize for older WASAPI versions.
        let frame_size = unsafe { audio_client.GetBufferSize() }.ok();
        Ok((
            frame_size.map(|v| v as usize),
            frame_size.map(|v| v as usize),
        ))
    }
}

impl AudioInputDevice for WasapiDevice {
    type StreamHandle<Callback: AudioInputCallback> = WasapiStream<Callback>;

    fn default_input_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_client = self.device.activate::<Audio::IAudioClient>()?;
        let format = unsafe { audio_client.GetMixFormat()?.read_unaligned() };
        Ok(StreamConfig {
            channels: 0u32.with_indices(0..format.nChannels as _),
            exclusive: false,
            samplerate: format.nSamplesPerSec as _,
            buffer_size_range: self.buffer_size_range()?,
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

    fn default_output_config(&self) -> Result<StreamConfig, Self::Error> {
        let audio_client = self.device.activate::<Audio::IAudioClient>()?;
        let format = unsafe { audio_client.GetMixFormat()?.read_unaligned() };
        Ok(StreamConfig {
            channels: 0u32.with_indices(0..format.nChannels as _),
            exclusive: false,
            samplerate: format.nSamplesPerSec as _,
            buffer_size_range: self.buffer_size_range()?,
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
