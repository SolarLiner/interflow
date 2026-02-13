//! PipeWire streams.

use crate::audio_buffer::{AudioMut, AudioRef};
use crate::backends::pipewire::error::PipewireError;
use crate::channel_map::Bitset;
use crate::prelude::pipewire::utils::{BlackHole, CallbackHolder};
use crate::timestamp::Timestamp;
use crate::{
    audio_buffer::{AudioMut, AudioRef},
    ResolvedStreamConfig,
};
use crate::{
    AudioCallback, AudioCallbackContext, AudioInput, AudioOutput, AudioStreamHandle, StreamConfig,
};
use libspa::buffer::Data;
use libspa::param::audio::{AudioFormat, AudioInfoRaw};
use libspa::pod::Pod;
use libspa::utils::Direction;
use libspa_sys::{SPA_PARAM_EnumFormat, SPA_TYPE_OBJECT_Format};
use pipewire::keys;
use pipewire::main_loop::MainLoop;
use pipewire::properties::Properties;
use pipewire::stream::{Stream, StreamFlags};
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Formatter;
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;

enum StreamCommands<Callback> {
    Eject(oneshot::Sender<Callback>),
}

impl<Callback> fmt::Debug for StreamCommands<Callback> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eject(_) => write!(f, "Eject"),
        }
    }
}

struct StreamInner<Callback> {
    scratch_buffer: Box<[f32]>,
    callback: Weak<CallbackHolder<Callback>>,
    config: ResolvedStreamConfig,
    timestamp: Timestamp,
}

impl<Callback: AudioCallback> StreamInner<Callback> {
    fn process_output(&mut self, channels: usize, frames: usize) -> usize {
        let buffer = AudioMut::from_noninterleaved_mut(
            &mut self.scratch_buffer[..channels * frames],
            channels,
        )
        .unwrap();
        let Some(mut callback) = self.callback.upgrade() else {
            return 0;
        };
        let context = AudioCallbackContext {
            stream_config: self.config,
            timestamp: self.timestamp,
        };
        let num_frames = buffer.num_frames();
        let dummy_input = AudioInput {
            timestamp: Timestamp::new(self.config.sample_rate),
            buffer: AudioRef::empty(),
        };
        let output = AudioOutput {
            buffer,
            timestamp: self.timestamp,
        };
        // SAFETY: there is max one other owner of the callback Arc, and it never dereferences
        // it thanks to `BlackHole`, fulfilling safety requirements of `arc_get_mut_unchecked()`.
        let callback = unsafe { arc_get_mut_unchecked(&mut callback) };
        callback.process_audio(context, dummy_input, output);
        self.timestamp += num_frames as u64;
        num_frames
    }
}

impl<Callback: AudioCallback> StreamInner<Callback> {
    fn process_input(&mut self, channels: usize, frames: usize) -> usize {
        let buffer =
            AudioRef::from_interleaved(&self.scratch_buffer[..channels * frames], channels)
                .unwrap();
        if let Some(mut callback) = self.callback.upgrade() {
            let context = AudioCallbackContext {
                stream_config: self.config,
                timestamp: self.timestamp,
            };
            let num_frames = buffer.num_frames();
            let input = AudioInput {
                buffer,
                timestamp: self.timestamp,
            };

            // SAFETY: there is max one other owner of the callback Arc, and it never dereferences
            // it thanks to `BlackHole`, fulfilling safety requirements of `arc_get_mut_unchecked()`.
            let callback = unsafe { arc_get_mut_unchecked(&mut callback) };
            callback.on_input_data(context, input);

            self.timestamp += num_frames as u64;
            num_frames
        } else {
            0
        }
    }
}

/// PipeWire stream handle.
pub struct StreamHandle<Callback> {
    commands: pipewire::channel::Sender<StreamCommands<Callback>>,
    handle: JoinHandle<Result<(), PipewireError>>,
}

impl<Callback> AudioStreamHandle<Callback> for StreamHandle<Callback> {
    type Error = PipewireError;

    fn eject(self) -> Result<Callback, Self::Error> {
        log::info!("Ejecting stream");
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(StreamCommands::Eject(tx))
            .expect("Should be able to send a message through PipeWire channel");
        self.handle.join().unwrap()?;
        Ok(rx.recv().unwrap())
    }
}

impl<Callback: 'static + AudioCallback> StreamHandle<Callback> {
    fn create_stream(
        name: String,
        serial: Option<String>,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        let handle = std::thread::Builder::new()
            .name(format!("{name} audio thread"))
            .spawn(move || {
                let main_loop = MainLoop::new(None)?;
                let context = Context::new(&main_loop)?;
                let core = context.connect(None)?;
                Ok(todo!())
            });
    }

    fn create_stream_old(
        device_object_serial: Option<String>,
        name: String,
        config: StreamConfig,
        callback: Callback,
        direction: pipewire::spa::utils::Direction,
        process_frames: impl Fn(&mut [Data], &mut StreamInner<Callback>, usize) -> usize
            + Send
            + 'static,
    ) -> Result<Self, PipewireError> {
        // Create a channel for sending command into PipeWire main loop.
        let (pipewire_sender, pipewire_receiver) =
            pipewire::channel::channel::<StreamCommands<Callback>>();

        let handle = std::thread::spawn(move || {
            let main_loop = MainLoop::new(None)?;
            let context = Context::new(&main_loop)?;
            let core = context.connect(None)?;

            let channels = if direction == pipewire::spa::utils::Direction::Input {
                config.input_channels
            } else {
                config.output_channels
            };
            let channels_str = channels.to_string();
            let buffer_size = stream_buffer_size(config.buffer_size_range);

            let mut properties = Properties::new();
            for (key, value) in user_properties {
                properties.insert(key, value);
            }

            let input_channels = if direction == pipewire::spa::utils::Direction::Input {
                channels
            } else {
                0
            };
            let output_channels = if direction == pipewire::spa::utils::Direction::Output {
                channels
            } else {
                0
            };

            let config = ResolvedStreamConfig {
                sample_rate: config.sample_rate.round(),
                input_channels,
                output_channels,
                max_frame_count,
            };

            properties.insert(*keys::MEDIA_TYPE, "Audio");
            properties.insert(*keys::MEDIA_ROLE, "Music");
            properties.insert(*keys::MEDIA_CATEGORY, get_category(direction));
            properties.insert(*keys::AUDIO_CHANNELS, channels_str);
            properties.insert(*keys::NODE_FORCE_QUANTUM, buffer_size.to_string());

            if let Some(device_object_serial) = device_object_serial {
                properties.insert(*keys::TARGET_OBJECT, device_object_serial);
            }

            let (callback_holder, callback_rx) = CallbackHolder::new(callback);
            let callback_holder = Arc::new(callback_holder);

            let stream_inner = StreamInner {
                callback: Arc::downgrade(&callback_holder),
                scratch_buffer: {
                    log::debug!(
                        "StreamInner: allocating {} frames",
                        max_frame_count * channels
                    );
                    vec![0.0; max_frame_count * channels].into_boxed_slice()
                },
                config,
                timestamp: Timestamp::new(config.sample_rate),
            };

            // SAFETY of StreamInner::process_input(), StreamInner::process_output() depends on us
            // never _dereferencing_ `callback_holder` outside of `StreamInner`. Achieve that at
            // type level by wrapping it in a black hole.
            let callback_holder = BlackHole::new(callback_holder);

            let stream = Stream::new(&core, &name, properties)?;
            config.samplerate = config.samplerate.round();
            let _listener = stream
                .add_local_listener_with_user_data(stream_inner)
                .process(move |stream, inner| {
                    log::debug!("Processing stream");
                    if let Some(mut buffer) = stream.dequeue_buffer() {
                        let datas = buffer.datas_mut();
                        log::debug!("Datas: len={}", datas.len());

                        if datas.is_empty() {
                            log::warn!("No datas available");
                            return;
                        };

                        process_frames(datas, inner, channels);
                    } else {
                        log::warn!("No buffer available");
                    }
                })
                .register()?;
            let values = pipewire::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &pipewire::spa::pod::Value::Object(pipewire::spa::pod::Object {
                    type_: SPA_TYPE_OBJECT_Format,
                    id: SPA_PARAM_EnumFormat,
                    properties: {
                        let mut info = AudioInfoRaw::new();
                        info.set_format(AudioFormat::F32LE);
                        info.set_rate(config.sample_rate as u32);
                        info.set_channels(channels as u32);
                        info.into()
                    },
                }),
            )?
            .0
            .into_inner();
            let mut params = [Pod::from_bytes(&values).unwrap()];
            stream.connect(
                direction,
                None,
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
                &mut params,
            )?;

            // Handle commands (stream ejection). Runs in the PipeWire main loop.
            let loop_ref = main_loop.downgrade();
            // pipewire::channel::receiver::attach() only accepts `Fn()` (instead of expected
            // `FnMut()`), so we need interior mutability. Cell is sufficient.
            let callback_holder = Cell::new(Some(callback_holder));
            let _attached_receiver = pipewire_receiver.attach(main_loop.loop_(), move |command| {
                log::debug!("Handling command: {command:?}");
                match command {
                    StreamCommands::Eject(reply) => {
                        // Take the callback holder our of its `Cell`, leaving `None` in place.
                        let callback_holder = callback_holder.take();

                        if callback_holder.is_none() {
                            // We've already ejected the callback, nothing to do.
                            return;
                        }

                        // Drop our reference to the Arc, which is its only persistent strong
                        // reference. The `CallbackHolder` will go out of scope (usually right away,
                        // but if the callback is running right now in the rt thread, then after it
                        // releases it), and its Drop impl will send it through `callback_tx`.
                        drop(callback_holder);

                        let callback = callback_rx.recv().expect(
                            "channel from StreamInner to receiver in pipewire main thread should \
                             not be closed",
                        );
                        reply.send(callback).unwrap();
                        if let Some(loop_ref) = loop_ref.upgrade() {
                            loop_ref.quit();
                        }
                    }
                }
            });

            log::debug!("Starting Pipewire main loop");
            main_loop.run();
            Ok::<_, PipewireError>(())
        });
        Ok(Self {
            commands: pipewire_sender,
            handle,
        })
    }
}

impl<Callback: 'static + Send + AudioCallback> StreamHandle<Callback> {
    /// Create an input Pipewire stream
    pub fn new_input(
        device_object_serial: Option<String>,
        name: impl ToString,
        config: StreamConfig,
        properties: HashMap<Vec<u8>, Vec<u8>>,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        Self::create_stream_old(
            device_object_serial,
            name.to_string(),
            config,
            properties,
            callback,
            Direction::Input,
            |datas, inner, channels| {
                // TODO: also take chunk offset into account to index into the data?
                let mut frames_total = 0;

                for (chunk, data) in datas.iter_mut().enumerate() {
                    let samples = data.chunk().size() as usize / size_of::<f32>();
                    if let Some(bytes) = data.data() {
                        let frames = samples / channels;
                        frames_total += frames;

                        let slice: &[f32] = zerocopy::FromBytes::ref_from_bytes(bytes)
                            .inspect_err(|e| log::error!("Cannot cast to f32 slice: {e}"))
                            .unwrap();
                        let target = &mut inner.scratch_buffer[chunk * samples..][..samples];
                        target.copy_from_slice(&slice[..samples]);
                    }
                }

                inner.process_input(channels, frames_total)
            },
        )
    }
}

const MAX_FRAMES: usize = 8192;

fn get_category(direction: pipewire::spa::utils::Direction) -> &'static str {
    match direction {
        pipewire::spa::utils::Direction::Input => "Capture",
        pipewire::spa::utils::Direction::Output => "Playback",
        x => unreachable!("Unexpected direction: 0x{:X}", x.as_raw()),
    }
}

impl<Callback: 'static + Send + AudioCallback> StreamHandle<Callback> {
    /// Create an output Pipewire stream
    pub fn new_output(
        device_object_serial: Option<String>,
        name: impl ToString,
        config: StreamConfig,
        properties: HashMap<Vec<u8>, Vec<u8>>,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        Self::create_stream_old(
            device_object_serial,
            name.to_string(),
            config,
            properties,
            callback,
            pipewire::spa::utils::Direction::Output,
            move |datas, inner, channels| {
                let buffer_size = stream_buffer_size(config.buffer_size_range);
                let provided_buffer_size = inner.process_output(channels, buffer_size);
                // TODO handle provided_buffer_size not being a multiple of datas.len()
                let buffer_size_per_chunk = provided_buffer_size / datas.len();
                let samples_per_chunk = buffer_size_per_chunk * channels;

                for (i, data) in datas.iter_mut().enumerate() {
                    let processed_slice =
                        &inner.scratch_buffer[i * samples_per_chunk..][..samples_per_chunk];
                    if let Some(bytes) = data.data() {
                        let slice: &mut [f32] = zerocopy::FromBytes::mut_from_bytes(bytes)
                            .inspect_err(|e| log::error!("Cannot cast to f32 slice: {e}"))
                            .unwrap();
                        slice[..samples_per_chunk].copy_from_slice(processed_slice);
                        let chunk = data.chunk_mut();
                        *chunk.offset_mut() = 0;
                        *chunk.stride_mut() = size_of::<f32>() as _;
                        *chunk.size_mut() = (size_of::<f32>() * samples_per_chunk) as _;
                    }
                }

                provided_buffer_size
            },
        )
    }
}

const DEFAULT_EXPECTED_FRAMES: usize = 512;

fn stream_buffer_size(range: (Option<usize>, Option<usize>)) -> usize {
    range.0.or(range.1).unwrap_or(DEFAULT_EXPECTED_FRAMES)
}

/// Returns a mutable reference into the given `Arc`, without any check.
///
/// This does the same thing as unstable [`Arc::get_mut_unchecked()`], but on stable Rust.
/// The documentation including Safety prerequisites are copied from Rust stdlib.
/// This helper can be removed once `get_mut_unchecked()` is stabilized and hits our MSRV.
///
/// Unsafe variant of [`Arc::get_mut()`], which is safe and does appropriate checks.
///
/// # Safety
///
/// If any other `Arc` or [`Weak`] pointers to the same allocation exist, then
/// they must not be dereferenced or have active borrows for the duration
/// of the returned borrow, and their inner type must be exactly the same as the
/// inner type of this Rc (including lifetimes). This is trivially the case if no
/// such pointers exist, for example immediately after `Arc::new`.
unsafe fn arc_get_mut_unchecked<T>(arc: &mut Arc<T>) -> &mut T {
    let raw_pointer = Arc::as_ptr(arc) as *mut T;
    unsafe { &mut *raw_pointer }
}
