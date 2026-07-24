//! Null backend. Accepts any stream configuration, does nothing.

use crate::prelude::*;
use interflow_core::collect;
use interflow_core::device::{ResolvedStreamConfig, StreamConfig};
use interflow_core::stream::CallbackContext;
use std::borrow::Cow;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

/// Platform type for the null backend.
pub struct Platform;

impl ExtensionProvider for Platform {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl platform::Platform for Platform {
    type Error = Infallible;
    type Device = Device;
    const NAME: &'static str = "Null";

    fn default_device(&self, device_type: DeviceType) -> Result<Self::Device, Self::Error> {
        Ok(Device(device_type))
    }

    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error> {
        Ok([Device(DeviceType::OUTPUT), Device(DeviceType::INPUT)])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Device(pub DeviceType);

impl ExtensionProvider for Device {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl device::Device for Device {
    type Error = Infallible;
    type StreamHandle<Callback: 'static + stream::Callback> = Stream<Callback>;

    fn name(&self) -> Cow<'_, str> {
        Cow::Borrowed("Null device")
    }

    fn device_type(&self) -> DeviceType {
        self.0
    }

    fn default_config(&self) -> Result<StreamConfig, Self::Error> {
        Ok(StreamConfig {
            exclusive: true,
            sample_rate: 48000.0,
            input_channels: 2,
            output_channels: 2,
            buffer_size_range: (None, None),
        })
    }

    fn is_config_supported(&self, _: &StreamConfig) -> bool {
        true
    }

    fn create_stream<Callback: 'static + Send + stream::Callback>(
        &self,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self::StreamHandle<Callback>, Self::Error> {
        Ok(Stream::new(stream_config, callback))
    }
}

pub struct Stream<Callback: stream::Callback> {
    thread_handle: JoinHandle<Callback>,
    abort_signal: Arc<AtomicBool>,
}

impl<Callback: 'static + stream::Callback> ExtensionProvider for Stream<Callback> {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl<Callback: 'static + stream::Callback> StreamHandle<Callback> for Stream<Callback> {
    type Error = Infallible;

    fn eject(self) -> Result<Callback, Self::Error> {
        self.abort_signal.store(true, Ordering::Relaxed);
        Ok(self.thread_handle.join().unwrap())
    }
}

impl<Callback: 'static + stream::Callback> Stream<Callback> {
    fn new(stream_config: StreamConfig, mut callback: Callback) -> Self {
        let abort_signal = Arc::new(AtomicBool::new(false));
        let thread_handle = std::thread::spawn({
            let abort_signal = abort_signal.clone();
            let frame_count = NonZeroUsize::new(
                stream_config
                    .buffer_size_range
                    .1
                    .or(stream_config.buffer_size_range.0)
                    .unwrap_or(512)
                    .max(1),
            )
            .unwrap();
            let input_buffers = AudioBuffer::zeroed(frame_count, stream_config.input_channels);
            let mut output_buffers =
                AudioBuffer::zeroed(frame_count, stream_config.output_channels);
            move || {
                let stream_config = ResolvedStreamConfig {
                    sample_rate: stream_config.sample_rate,
                    input_channels: stream_config.input_channels,
                    output_channels: stream_config.output_channels,
                    max_frame_count: frame_count.get(),
                };
                let mut context = CallbackContext {
                    stream_config: &stream_config,
                    timestamp: Timestamp::new(stream_config.sample_rate),
                    stream_proxy: &NullStreamProxy,
                };
                callback.prepare(context);
                let start = Instant::now();
                loop {
                    if abort_signal.load(Ordering::Relaxed) {
                        break callback;
                    }

                    let audio_input = AudioInput {
                        timestamp: context.timestamp,
                        buffer: input_buffers.as_ref(),
                        channel_flags: &[],
                    };
                    let mut audio_output = AudioOutput {
                        timestamp: context.timestamp,
                        buffer: output_buffers.as_mut(),
                        channel_flags: &[],
                    };
                    callback.process_audio(context, &audio_input, &mut audio_output);
                    context.timestamp += stream_config.max_frame_count as u64;
                    while context.timestamp.as_duration() > start.elapsed() {
                        std::thread::yield_now();
                    }
                }
            }
        });
        Self {
            thread_handle,
            abort_signal,
        }
    }
}

struct NullStreamProxy;

impl ExtensionProvider for NullStreamProxy {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl StreamProxy for NullStreamProxy {}

#[scattered_collect::scatter(collect::REGISTRAR)]
static NULL_PLATFORM_REGISTRATION: collect::Registration = collect::Registration {
    constructor: || {
        log::error!("No platforms available, using null backend (you will not hear any sound)");
        Some(Rc::new(Platform))
    },
    priority: i32::MIN + 1,
};
