//! CoreAudio stream implementation.
use crate::device::Device;
use crate::Error;
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::render_callback::{data, Args};
use coreaudio::audio_unit::{AudioUnit, Element, SampleFormat, Scope, StreamFormat};
use coreaudio_sys::{kAudioDevicePropertyBufferFrameSize, kAudioUnitProperty_StreamFormat};
use interflow_core::buffer::AudioBuffer;
use interflow_core::device::{Device as _, ResolvedStreamConfig, StreamConfig};
use interflow_core::stream;
use interflow_core::stream::{AudioInput, AudioOutput, CallbackContext, StreamProxy};
use interflow_core::timing::Timestamp;
use interflow_core::traits::{ExtensionProvider, Selector};
use std::convert::Infallible;
use std::num::NonZeroUsize;

/// Stream type created by opening up a stream on a [`Device`].
pub struct Handle<Callback> {
    audio_unit: AudioUnit,
    callback_retrieve: oneshot::Sender<oneshot::Sender<Callback>>,
}

impl<Callback> stream::StreamHandle<Callback> for Handle<Callback> {
    type Error = Infallible;

    fn eject(mut self) -> Result<Callback, Self::Error> {
        let (tx, rx) = oneshot::channel();
        self.callback_retrieve
            .send(tx)
            .expect("Callback receiver cannot have been dropped yet");
        let callback = rx.recv().expect("Oneshot receiver must be used");
        self.audio_unit.free_input_callback();
        self.audio_unit.free_render_callback();
        Ok(callback)
    }
}

fn input_stream_format(sample_rate: f64, channel_count: usize) -> StreamFormat {
    StreamFormat {
        sample_rate,
        sample_format: SampleFormat::I16,
        flags: LinearPcmFlags::IS_SIGNED_INTEGER,
        channels: channel_count as _,
    }
}

fn output_stream_format(sample_rate: f64, channel_count: usize) -> StreamFormat {
    StreamFormat {
        sample_rate,
        sample_format: SampleFormat::F32,
        flags: LinearPcmFlags::IS_NON_INTERLEAVED | LinearPcmFlags::IS_FLOAT,
        channels: channel_count as _,
    }
}

struct DummyStreamProxy;

impl ExtensionProvider for DummyStreamProxy {
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a> {
        selector
    }
}

impl StreamProxy for DummyStreamProxy {}

static STREAM_PROXY: DummyStreamProxy = DummyStreamProxy;

impl<Callback: 'static + Send + stream::Callback> Handle<Callback> {
    pub(crate) fn new(
        device: &Device,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, Error> {
        let requested_type = stream_config.requested_device_type();
        if requested_type.is_duplex() {
            return Err(Error::DuplexUnavailable);
        }

        let unsupported = device.device_type() & !requested_type;
        if !unsupported.is_empty() {
            log::warn!(
                "Cannot request {unsupported:?} for {device}, ignoring",
                device = device.name()
            );
        }
        let unit = device.request.to_audio_unit()?;
        if requested_type.is_input() {
            Self::new_input(unit, stream_config, callback)
        } else {
            Self::new_output(unit, stream_config, callback)
        }
    }

    fn new_input(
        mut audio_unit: AudioUnit,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Result<Self, Error> {
        let asbd =
            input_stream_format(stream_config.sample_rate, stream_config.input_channels).to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Output,
            Element::Input,
            Some(&asbd),
        )?;
        let frame_count: u32 = audio_unit.get_property(
            kAudioDevicePropertyBufferFrameSize,
            Scope::Input,
            Element::Input,
        )?;
        let resolved_config = ResolvedStreamConfig {
            sample_rate: asbd.mSampleRate,
            input_channels: asbd.mChannelsPerFrame as _,
            output_channels: 0,
            max_frame_count: frame_count as _,
        };
        let mut buffer = AudioBuffer::zeroed(
            NonZeroUsize::new(frame_count as _).unwrap(),
            NonZeroUsize::new(asbd.mChannelsPerFrame as _).unwrap(),
        );

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        callback.prepare(CallbackContext {
            stream_config: &resolved_config,
            timestamp: Timestamp::new(asbd.mSampleRate),
            stream_proxy: &STREAM_PROXY,
        });
        let mut callback = Some(callback);

        audio_unit.set_input_callback(move |args: Args<data::Interleaved<i16>>| {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return Err(());
            }
            let num_frames = args.num_frames;
            let channels = asbd.mChannelsPerFrame as usize;
            let input_data = args.data.buffer;

            for frame_idx in 0..num_frames {
                for ch in 0..channels {
                    let sample = input_data[frame_idx * channels + ch];
                    buffer
                        .frame_mut(frame_idx)
                        .set(ch, (sample as f32) / (i16::MAX as f32));
                }
            }

            let timestamp = Timestamp::from_count(
                resolved_config.sample_rate,
                args.time_stamp.mSampleTime as _,
            );

            let input = AudioInput {
                buffer: buffer.as_ref(),
                timestamp,
                channel_flags: &[],
            };

            let mut dummy_buf = AudioBuffer::zeroed(
                NonZeroUsize::new(1).unwrap(),
                NonZeroUsize::new(channels as _).unwrap(),
            );
            let dummy_output = AudioOutput {
                buffer: dummy_buf.as_mut(),
                timestamp: Timestamp::new(asbd.mSampleRate),
                channel_flags: &[],
            };

            if let Some(callback) = &mut callback {
                callback.process_audio(
                    CallbackContext {
                        stream_config: &resolved_config,
                        timestamp,
                        stream_proxy: &STREAM_PROXY,
                    },
                    input,
                    dummy_output,
                );
            }
            Ok(())
        })?;
        audio_unit.start()?;
        Ok(Self {
            audio_unit,
            callback_retrieve: tx,
        })
    }

    fn new_output(
        mut audio_unit: AudioUnit,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Result<Self, Error> {
        let asbd = output_stream_format(stream_config.sample_rate, stream_config.output_channels)
            .to_asbd();
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd),
        )?;
        let frame_size: u32 = audio_unit.get_property(
            kAudioDevicePropertyBufferFrameSize,
            Scope::Output,
            Element::Output,
        )?;
        let resolved_config = ResolvedStreamConfig {
            sample_rate: asbd.mSampleRate,
            input_channels: 0,
            output_channels: asbd.mChannelsPerFrame as _,
            max_frame_count: frame_size as _,
        };
        let mut buffer = AudioBuffer::zeroed(
            NonZeroUsize::new(frame_size as _).unwrap(),
            NonZeroUsize::new(asbd.mChannelsPerFrame as _).unwrap(),
        );

        let (tx, rx) = oneshot::channel::<oneshot::Sender<Callback>>();
        callback.prepare(CallbackContext {
            stream_config: &resolved_config,
            timestamp: Timestamp::new(resolved_config.sample_rate),
            stream_proxy: &STREAM_PROXY,
        });
        let mut callback = Some(callback);

        audio_unit.set_render_callback(move |mut args: Args<data::NonInterleaved<f32>>| {
            if let Ok(sender) = rx.try_recv() {
                sender.send(callback.take().unwrap()).unwrap();
                return Err(());
            }
            let timestamp = Timestamp::from_count(
                resolved_config.sample_rate,
                args.time_stamp.mSampleTime as _,
            );

            let dummy_buf = AudioBuffer::zeroed(
                NonZeroUsize::new(1).unwrap(),
                NonZeroUsize::new(asbd.mChannelsPerFrame as _).unwrap(),
            );
            let dummy_input = AudioInput {
                buffer: dummy_buf.as_ref(),
                timestamp: Timestamp::new(resolved_config.sample_rate),
                channel_flags: &[],
            };

            let output = AudioOutput {
                buffer: buffer.as_mut(),
                timestamp,
                channel_flags: &[],
            };

            if let Some(callback) = &mut callback {
                callback.process_audio(
                    CallbackContext {
                        stream_config: &resolved_config,
                        timestamp,
                        stream_proxy: &STREAM_PROXY,
                    },
                    dummy_input,
                    output,
                );
                for (out_ch, in_ch) in args.data.channels_mut().zip(buffer.iter_channels()) {
                    out_ch.copy_from_slice(in_ch);
                }
            }
            Ok(())
        })?;
        audio_unit.start()?;
        Ok(Self {
            audio_unit,
            callback_retrieve: tx,
        })
    }
}
