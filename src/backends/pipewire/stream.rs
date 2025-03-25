use crate::audio_buffer::{AudioBuffer, AudioMut};
use crate::backends::pipewire::error::PipewireError;
use crate::channel_map::Bitset;
use crate::timestamp::Timestamp;
use crate::{
    AudioCallbackContext, AudioInput, AudioOutput, AudioOutputCallback, AudioStreamHandle,
    StreamConfig,
};
use libspa::param::audio::{AudioFormat, AudioInfoRaw};
use libspa::pod::Pod;
use libspa_sys::{SPA_PARAM_EnumFormat, SPA_TYPE_OBJECT_Format};
use pipewire::context::Context;
use pipewire::main_loop::{MainLoop, WeakMainLoop};
use pipewire::properties::properties;
use pipewire::stream::{Stream, StreamFlags, StreamListener};
use pipewire::{keys, Error};
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
    fn process(&mut self, channels: usize, frames: usize) -> usize {
        let buffer = AudioMut::from_noninterleaved_mut(
            &mut self.scratch_buffer[..channels * frames],
            channels,
        )
        .unwrap();
        if let Some(callback) = self.callback.as_mut() {
            let context = AudioCallbackContext {
                stream_config: self.config,
                timestamp: self.timestamp,
            };
            let num_frames = buffer.num_samples();
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

const MAX_FRAMES: usize = 8192;

impl<Callback: 'static + Send + AudioOutputCallback> StreamHandle<Callback> {
    /// Create a Pipewire stream
    pub fn new(
        name: impl ToString,
        mut config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, PipewireError> {
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        let name = name.to_string();
        let handle = std::thread::spawn(move || {
            let main_loop = MainLoop::new(None)?;
            let context = Context::new(&main_loop)?;
            let core = context.connect(None)?;

            let channels = config.channels.count();
            let channels_str = channels.to_string();
            let stream = Stream::new(
                &core,
                &name,
                properties! {
                    *keys::MEDIA_TYPE => "Audio",
                    *keys::MEDIA_ROLE => "Music",
                    *keys::MEDIA_CATEGORY => "Playback",
                    *keys::AUDIO_CHANNELS => channels_str,
                },
            )?;
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
                        let Some(min_frames) = datas
                            .iter_mut()
                            .filter_map(|d| d.data().map(|d| d.len() / size_of::<f32>()))
                            .min()
                        else {
                            log::warn!("No datas available");
                            return;
                        };
                        let frames = min_frames.min(MAX_FRAMES);
                        let frames = inner.process(channels, frames);

                        for (i, data) in datas.iter_mut().enumerate() {
                            let processed_slice = &mut inner.scratch_buffer[i * frames..][..frames];
                            if let Some(data) = data.data() {
                                let slice: &mut [f32] = zerocopy::FromBytes::mut_from_bytes(data)
                                    .inspect_err(|e| log::error!("Cannot cast to f32 slice: {e}"))
                                    .unwrap();
                                slice[..frames].copy_from_slice(processed_slice);
                            }
                            let chunk = data.chunk_mut();
                            *chunk.offset_mut() = 0;
                            *chunk.stride_mut() = size_of::<f32>() as _;
                            *chunk.size_mut() = (size_of::<f32>() * frames) as _;
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
                pipewire::spa::utils::Direction::Output,
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
