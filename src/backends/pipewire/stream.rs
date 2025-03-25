use crate::audio_buffer::AudioMut;
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
use pipewire::main_loop::MainLoop;
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
    ejected: bool,
    config: StreamConfig,
    timestamp: Timestamp,
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
                    self.ejected = true;
                }
            }
        }
    }

    fn handle_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            self.handle_command(command);
        }
    }
}

impl<Callback: AudioOutputCallback> StreamInner<Callback> {
    fn process(&mut self, buffer: AudioMut<f32>) {
        if let Some(callback) = self.callback.as_mut() {
            let context = AudioCallbackContext {
                stream_config: self.config,
                timestamp: self.timestamp,
            };
            let num_frames = buffer.num_samples() as u64;
            let output = AudioOutput {
                buffer,
                timestamp: self.timestamp,
            };
            callback.on_output_data(context, output);
            self.timestamp += num_frames;
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

impl<Callback: 'static + Send + AudioOutputCallback> StreamHandle<Callback> {
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
                    scratch_buffer: vec![0.0; channels * 8192].into_boxed_slice(),
                    ejected: false,
                    config,
                    timestamp: Timestamp::new(config.samplerate),
                })
                .process(move |stream, inner| {
                    log::debug!("Processing stream");
                    inner.handle_commands();
                    if inner.ejected {
                        if let Err(err) = stream.disconnect() {
                            log::error!("Failed to disconnect stream: {}", err);
                        }
                        return;
                    }
                    if let Some(mut buffer) = stream.dequeue_buffer() {
                        let datas = buffer.datas_mut();
                        let buf = &mut datas[0];
                        if let Some(data) = buf.data() {
                            let data: &mut [f32] = zerocopy::FromBytes::mut_from_bytes(data)
                                .expect("Pipewire buffer cannot be cast to f32 slice");
                            let buffer = AudioMut::from_noninterleaved_mut(data, channels)
                                .expect("Pipewire buffer is incomplete");
                            inner.process(buffer);
                        }
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
            return Ok::<_, PipewireError>(());
        });
        log::debug!("Sending callback to stream");
        tx.push(StreamCommands::ReceiveCallback(callback)).unwrap();
        Ok(Self {
            commands: tx,
            handle,
        })
    }
}
