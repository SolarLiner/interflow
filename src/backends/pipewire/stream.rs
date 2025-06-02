use crate::backends::pipewire::error::PipewireError;
use crate::channel_map::Bitset;
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
use libspa_sys::{SPA_PARAM_EnumFormat, SPA_TYPE_OBJECT_Format};
use pipewire::keys;
use pipewire::main_loop::{MainLoop, WeakMainLoop};
use pipewire::properties::properties;
use pipewire::stream::{Stream, StreamFlags};
use pipewire::{context::Context, node::NodeListener};
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
    config: ResolvedStreamConfig,
    timestamp: Timestamp,
    loop_ref: WeakMainLoop,
}

impl<Callback: AudioCallback> StreamInner<Callback> {
    fn handle_command(&mut self, command: StreamCommands<Callback>) {
        log::debug!("Handling command: {command:?}");
        match command {
            StreamCommands::ReceiveCallback(mut callback) => {
                debug_assert!(self.callback.is_none());
                log::debug!("StreamCommands::ReceiveCallback prepare {:#?}", self.config);
                callback.prepare(AudioCallbackContext {
                    stream_config: self.config,
                    timestamp: self.timestamp,
                });
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

impl<Callback: AudioCallback> StreamInner<Callback> {
    fn process_output(&mut self, channels: usize, frames: usize) -> usize {
        let buffer = AudioMut::from_noninterleaved_mut(
            &mut self.scratch_buffer[..channels * frames],
            channels,
        )
        .unwrap();
        let Some(callback) = self.callback.as_mut() else {
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
        callback.process_audio(context, dummy_input, output);
        self.timestamp += num_frames as u64;
        num_frames
    }
}

impl<Callback: AudioCallback> StreamInner<Callback> {
    fn process_input(&mut self, channels: usize, frames: usize) -> usize {
        let buffer =
            AudioRef::from_noninterleaved(&self.scratch_buffer[..channels * frames], channels)
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
            let dummy_output = AudioOutput {
                timestamp: Timestamp::new(self.config.sample_rate),
                buffer: AudioMut::empty(),
            };
            callback.process_audio(context, input, dummy_output);
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

impl<Callback: 'static + AudioCallback> StreamHandle<Callback> {
    fn create_stream(name: String, config: StreamConfig, callback: Callback) -> Result<Self, PipewireError> {
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
        name: String,
        config: StreamConfig,
        callback: Callback,
        direction: pipewire::spa::utils::Direction,
        process_frames: impl Fn(&mut [Data], &mut StreamInner<Callback>, usize, std::ops::Range<usize>) -> usize
            + Send
            + 'static,
    ) -> Result<Self, PipewireError> {
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
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
            let max_frame_count = MAX_FRAMES;
            let frame_count = config
                .buffer_size_range
                .0
                .or(config.buffer_size_range.1)
                .unwrap_or(max_frame_count);
            let sample_rate = config.sample_rate;

            let node_rate = format!("1/{}", sample_rate);
            let node_latency = format!("{}/{}", frame_count, sample_rate);
            let node_max_latency = format!("{}/{}", max_frame_count, sample_rate);

            log::debug!("Create stream node_rate={node_rate} node_latency={node_latency} node_max_latency={node_max_latency}");
            let stream = Stream::new(
                &core,
                &name,
                properties! {
                    *keys::MEDIA_TYPE => "Audio",
                    *keys::MEDIA_ROLE => "Music",
                    *keys::MEDIA_CATEGORY => get_category(direction),
                    *keys::AUDIO_CHANNELS => channels_str,
                    *keys::AUDIO_RATE => node_rate,
                    *keys::NODE_LATENCY => node_latency,
                    *keys::NODE_MAX_LATENCY => node_max_latency,
                },
            )?;

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
            let _listener = stream
                .add_local_listener_with_user_data(StreamInner {
                    callback: None,
                    commands: rx,
                    scratch_buffer: {
                        log::debug!(
                            "StreamInner: allocating {} frames",
                            max_frame_count * channels
                        );
                        vec![0.0; max_frame_count * channels].into_boxed_slice()
                    },
                    loop_ref: main_loop.downgrade(),
                    config,
                    timestamp: Timestamp::new(config.sample_rate),
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
                        let Some(min_frames) = datas
                            .iter_mut()
                            .filter_map(|d| d.data().map(|d| d.len() / size_of::<f32>()))
                            .min()
                        else {
                            log::warn!("No datas available");
                            return;
                        };
                        let frames = min_frames.min(max_frame_count);

                        let mut processed = 0;
                        while processed < min_frames {
                            let frames = frames.min(min_frames - processed);
                            processed += process_frames(
                                datas,
                                inner,
                                channels,
                                processed..processed + frames,
                            );
                        }

                        for data in datas.iter_mut() {
                            let chunk = data.chunk_mut();
                            *chunk.offset_mut() = 0;
                            *chunk.stride_mut() = size_of::<f32>() as _;
                            *chunk.size_mut() = (size_of::<f32>() * processed) as _;
                        }
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
                        info.set_format(AudioFormat::F32P);
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
        name: impl ToString,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        Self::create_stream_old(
            name.to_string(),
            config,
            callback,
            pipewire::spa::utils::Direction::Input,
            |datas, inner, channels, frame_range| {
                log::debug!("input inner process: frames={frame_range:?}");
                let frames = frame_range.len();
                for (i, data) in datas.iter_mut().enumerate() {
                    let Some(data) = data.data() else {
                        continue;
                    };
                    let slice: &[f32] = zerocopy::FromBytes::ref_from_bytes(data)
                        .inspect_err(|e| log::error!("Cannot cast to f32 slice: {e}"))
                        .unwrap();
                    let target = &mut inner.scratch_buffer[i * frames..(i + 1) * frames];
                    target.copy_from_slice(&slice[frame_range.clone()]);
                }
                inner.process_input(channels, frames)
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
        name: impl ToString,
        config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        Self::create_stream_old(
            name.to_string(),
            config,
            callback,
            pipewire::spa::utils::Direction::Output,
            |datas, inner, channels, frame_range| {
                log::debug!("output inner process: frames={frame_range:?}");
                let frames = frame_range.len();
                let frames = inner.process_output(channels, frames);
                for (i, data) in datas.iter_mut().enumerate() {
                    let processed_slice = &inner.scratch_buffer[i * frames..(i + 1) * frames];
                    let Some(data) = data.data() else {
                        continue;
                    };
                    let slice: &mut [f32] = zerocopy::FromBytes::mut_from_bytes(data)
                        .inspect_err(|e| log::error!("Cannot cast to f32 slice: {e}"))
                        .unwrap();
                    slice[frame_range.clone()].copy_from_slice(processed_slice);
                }
                frames
            },
        )
    }
}
