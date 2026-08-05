use crate::util::MMDevice;
use crate::Error;
use interflow_core::device::StreamConfig;
use interflow_core::traits::{ExtensionProvider, Selector};
use interflow_core::{device, stream, DeviceType};
use std::borrow::Cow;
use std::ptr::NonNull;
use windows::Win32::Media::Audio;
use windows::Win32::Media::Audio::{IAudioClient, IAudioClient3};
use windows::Win32::Media::Multimedia;

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
    type Error = Error;
    type StreamHandle<Callback: 'static + stream::Callback> = crate::stream::Handle<Callback>;

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
        self.check_format(config).unwrap_or(false)
    }

    fn create_stream<Callback: 'static + Send + stream::Callback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        crate::stream::Handle::new(self, stream_config, callback)
    }
}

impl Device {
    pub(crate) fn activate_audio_client(&self) -> Result<IAudioClient, Error> {
        self.handle.activate::<IAudioClient>()
    }

    fn get_mix_format(&self) -> Result<StreamConfig, Error> {
        let client = self.activate_audio_client()?;
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

    fn get_mix_format_iac3(&self) -> Result<StreamConfig, Error> {
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

    fn check_format(&self, config: &StreamConfig) -> Result<bool, Error> {
        unsafe {
            let audio_client = self.activate_audio_client()?;
            let sharemode = if config.exclusive {
                Audio::AUDCLNT_SHAREMODE_EXCLUSIVE
            } else {
                Audio::AUDCLNT_SHAREMODE_SHARED
            };
            let format = Self::build_format(config);
            if config.exclusive {
                audio_client
                    .IsFormatSupported(sharemode, &format, None)
                    .ok()?;
                return Ok(true);
            }
            let mut closest_ptr: *mut Audio::WAVEFORMATEX = std::ptr::null_mut();
            let result = audio_client.IsFormatSupported(sharemode, &format, Some(&mut closest_ptr));
            let hr = result.0;
            if hr == 0 {
                return Ok(true);
            }
            if hr > 0 && !closest_ptr.is_null() {
                let closest = crate::util::CoTask::new(NonNull::new_unchecked(closest_ptr));
                let closest_format = closest.as_ptr().read_unaligned();
                return Ok(closest_format.nSamplesPerSec == config.sample_rate as u32
                    && closest_format.nChannels as usize
                        == config.output_channels.max(config.input_channels));
            }
            Ok(false)
        }
    }

    pub(crate) fn build_format(config: &StreamConfig) -> Audio::WAVEFORMATEX {
        let channels = (config.input_channels.max(config.output_channels)) as u16;
        let sample_rate = config.sample_rate as u32;
        let sample_bytes = size_of::<f32>() as u16;
        let avg_bytes_per_sec = channels as u32 * sample_rate * sample_bytes as u32;
        let block_align = channels * sample_bytes;
        Audio::WAVEFORMATEX {
            wFormatTag: Multimedia::WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: avg_bytes_per_sec,
            nBlockAlign: block_align,
            wBitsPerSample: 8 * sample_bytes,
            cbSize: 0,
        }
    }
}

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
            let device = self.collection.Item(self.next_item).ok()?;
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
