use crate::audio_buffer::{AudioMut, AudioRef};
use crate::backends::pipewire::error::PipewireError;
use crate::channel_map::Bitset;
use crate::timestamp::Timestamp;
use crate::{
    AudioCallback, AudioCallbackContext, AudioInput, AudioOutput, AudioOutputCallback,
    AudioStreamHandle, StreamConfig,
};
use libspa::buffer::Data;
use libspa::param::audio::{AudioFormat, AudioInfoRaw};
use libspa::pod::Pod;
use libspa::utils::Direction;
use libspa_sys::{SPA_PARAM_EnumFormat, SPA_TYPE_OBJECT_Format};
use pipewire::context::Context;
use pipewire::keys;
use pipewire::main_loop::{MainLoop, WeakMainLoop};
use pipewire::properties::Properties;
use pipewire::stream::{Stream, StreamFlags};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Formatter;
use std::thread::JoinHandle;

enum StreamCommands<Callback> {
    ReceiveCallback(Callback),
    Eject(oneshot::Sender<Callback>),
}

impl<Callback> fmt::Debug for StreamCommands<Callback> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReceiveCallback(_) => write!(f, "ReceiveCallback"),
            Self::Eject(_) => write!(f, "Eject"),
        }
    }
}

struct StreamInner<Callback> {
    commands: rtrb::Consumer<StreamCommands<Callback>>,
    scratch_buffer: Box<[f32]>,
    callback: Option<Callback>,
    config: StreamConfig,
    timestamp: Timestamp,
    loop_ref: WeakMainLoop,
}

impl<Callback> StreamInner<Callback> {
    fn handle_command(&mut self, command: StreamCommands<Callback>) {
        log::debug!("Handling command: {command:?}");
        match command {
            StreamCommands::ReceiveCallback(callback) => {
                debug_assert!(self.callback.is_none());
                self.callback = Some(callback);
            }
            StreamCommands::Eject(reply) => {
                if let Some(callback) = self.callback.take() {
                    reply.send(callback).unwrap();
                    if let Some(loop_ref) = self.loop_ref.upgrade() {
                        loop_ref.quit();
                    }
                }
            }
        }
    }

    fn handle_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            self.handle_command(command);
        }
    }

    fn ejected(&self) -> bool {
        self.callback.is_none()
    }
}

impl<Callback: AudioOutputCallback> StreamInner<Callback> {
    fn process_output(&mut self, channels: usize, buffer_size: usize) -> usize {
        let buffer = AudioMut::from_interleaved_mut(
            &mut self.scratch_buffer[..channels * buffer_size],
            channels,
        )
        .unwrap();
        if let Some(callback) = self.callback.as_mut() {
            let context = AudioCallbackContext {
                stream_config: self.config,
                timestamp: self.timestamp,
            };
            let num_frames = buffer.num_frames();
            let output = AudioOutput {
                buffer,
                timestamp: self.timestamp,
            };
            callback.on_output_data(context, output);
            self.timestamp += num_frames as u64;
            num_frames
        } else {
            0
        }
    }
}

impl<Callback: AudioCallback> StreamInner<Callback> {
    fn process_input(&mut self, channels: usize, frames: usize) -> usize {
        let buffer =
            AudioRef::from_interleaved(&self.scratch_buffer[..channels * frames], channels)
                .unwrap();
        if let Some(callback) = self.callback.as_mut() {
            let context = AudioCallbackContext {
                stream_config: self.config,
                timestamp: self.timestamp,
            };
            let num_frames = buffer.num_frames();
            let input = AudioInput {
                buffer,
                timestamp: self.timestamp,
            };
            callback.process_audio(context, input);
            self.timestamp += num_frames as u64;
            num_frames
        } else {
            0
        }
    }
}

pub struct StreamHandle<Callback> {
    commands: rtrb::Producer<StreamCommands<Callback>>,
    handle: JoinHandle<Result<(), PipewireError>>,
}

impl<Callback> AudioStreamHandle<Callback> for StreamHandle<Callback> {
    type Error = PipewireError;

    fn eject(mut self) -> Result<Callback, Self::Error> {
        log::info!("Ejecting stream");
        let (tx, rx) = oneshot::channel();
        self.commands
            .push(StreamCommands::Eject(tx))
            .expect("Command buffer overflow");
        self.handle.join().unwrap()?;
        Ok(rx.recv().unwrap())
    }
}

impl<Callback: 'static + Send> StreamHandle<Callback> {
    fn create_stream(
        device_object_serial: Option<String>,
        name: String,
        mut config: StreamConfig,
        user_properties: HashMap<Vec<u8>, Vec<u8>>,
        callback: Callback,
        direction: pipewire::spa::utils::Direction,
        process_frames: impl Fn(&mut [Data], &mut StreamInner<Callback>, usize) -> usize
            + Send
            + 'static,
    ) -> Result<Self, PipewireError> {
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        let handle = std::thread::spawn(move || {
            let main_loop = MainLoop::new(None)?;
            let context = Context::new(&main_loop)?;
            let core = context.connect(None)?;

            let channels = config.output_channels.count();
            let channels_str = channels.to_string();
            let buffer_size = stream_buffer_size(config.buffer_size_range);

            let mut properties = Properties::new();
            for (key, value) in user_properties {
                properties.insert(key, value);
            }

            properties.insert(*keys::MEDIA_TYPE, "Audio");
            properties.insert(*keys::MEDIA_ROLE, "Music");
            properties.insert(*keys::MEDIA_CATEGORY, get_category(direction));
            properties.insert(*keys::AUDIO_CHANNELS, channels_str);
            properties.insert(*keys::NODE_FORCE_QUANTUM, buffer_size.to_string());

            if let Some(device_object_serial) = device_object_serial {
                properties.insert(*keys::TARGET_OBJECT, device_object_serial);
            }

            let stream = Stream::new(&core, &name, properties)?;
            config.samplerate = config.samplerate.round();
            let _listener = stream
                .add_local_listener_with_user_data(StreamInner {
                    callback: None,
                    commands: rx,
                    scratch_buffer: vec![0.0; MAX_FRAMES * channels].into_boxed_slice(),
                    loop_ref: main_loop.downgrade(),
                    config,
                    timestamp: Timestamp::new(config.samplerate),
                })
                .process(move |stream, inner| {
                    log::debug!("Processing stream");
                    inner.handle_commands();
                    if inner.ejected() {
                        return;
                    }
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
                        info.set_rate(config.samplerate as u32);
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
            log::debug!("Starting Pipewire main loop");
            main_loop.run();
            Ok::<_, PipewireError>(())
        });
        log::debug!("Sending callback to stream");
        tx.push(StreamCommands::ReceiveCallback(callback)).unwrap();
        Ok(Self {
            commands: tx,
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
        Self::create_stream(
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

impl<Callback: 'static + Send + AudioOutputCallback> StreamHandle<Callback> {
    /// Create an output Pipewire stream
    pub fn new_output(
        device_object_serial: Option<String>,
        name: impl ToString,
        config: StreamConfig,
        properties: HashMap<Vec<u8>, Vec<u8>>,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        Self::create_stream(
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
