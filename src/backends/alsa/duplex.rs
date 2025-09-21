use crate::channel_map::{Bitset, ChannelMap32};
use crate::duplex::AudioDuplexCallback;
use crate::prelude::alsa::device::AlsaDevice;
use crate::prelude::alsa::stream::AlsaStream;
use crate::timestamp::Timestamp;
use crate::{
    audio_buffer::{AudioMut, AudioRef},
    backends::alsa::AlsaError,
    stream::{AudioCallbackContext, AudioInput, AudioOutput, StreamConfig},
};
use alsa::{pcm, PollDescriptors};
use std::sync::Arc;
use std::time::Duration;

impl<Callback: AudioDuplexCallback> AlsaStream<Callback> {
    pub fn new_duplex(
        stream_config: StreamConfig,
        input_name: String,
        output_name: String,
        mut callback: Callback,
    ) -> Result<Self, AlsaError> {
        {
            let (tx, rx) = super::triggerfd::trigger()?;
            let join_handle = std::thread::spawn({
                move || {
                    let output_device = AlsaDevice::new(&output_name, alsa::Direction::Playback)?;
                    let (output_hwp, _, output_io) = output_device.apply_config(&stream_config)?;
                    let (_, period_size) = output_device.pcm.get_params()?;
                    let out_periods = period_size as usize;
                    log::info!("[Output] Period size : {out_periods}");
                    let out_channels = output_hwp.get_channels()? as usize;
                    log::info!("[Output] Num channels: {out_channels}");
                    let out_samplerate = output_hwp.get_rate()? as f64;
                    log::info!("[Output] Sample rate : {out_samplerate}");
                    let output_config = StreamConfig {
                        samplerate: out_samplerate,
                        channels: ChannelMap32::default()
                            .with_indices(std::iter::repeat(1).take(out_channels)),
                        buffer_size_range: (Some(out_periods), Some(out_periods)),
                        exclusive: false,
                    };
                    let mut out_timestamp = Timestamp::new(out_samplerate);
                    let mut out_buffer = vec![0f32; out_periods * out_channels];
                    let out_latency = out_periods as f64 / out_samplerate;
                    output_device.pcm.prepare()?;
                    if output_device.pcm.state() != pcm::State::Running {
                        output_device.pcm.start()?;
                    }

                    let input_device = AlsaDevice::new(&input_name, alsa::Direction::Capture)?;
                    let (input_hwp, _, input_io) = input_device.apply_config(&output_config)?;
                    let (_, period_size) = input_device.pcm.get_params()?;
                    let in_periods = period_size as usize;
                    log::info!("[Input]  Period size : {in_periods}");
                    let in_channels = input_hwp.get_channels()? as usize;
                    log::info!("[Input]  Num channels: {in_channels}");
                    let in_samplerate = input_hwp.get_rate()? as f64;
                    log::info!("[Input]  Sample rate : {in_samplerate}");
                    let mut in_timestamp = Timestamp::new(in_samplerate);
                    let mut in_buffer = vec![0f32; in_periods * in_channels];
                    let in_latency = in_periods as f64 / in_samplerate;
                    input_device.pcm.prepare()?;
                    if input_device.pcm.state() != pcm::State::Running {
                        input_device.pcm.start()?;
                    }
                    let mut poll_descriptors = {
                        let mut buf = vec![rx.as_pollfd()];
                        let num_descriptors = input_device.pcm.count() + output_device.pcm.count();
                        buf.extend(
                            std::iter::repeat(libc::pollfd {
                                fd: 0,
                                events: 0,
                                revents: 0,
                            })
                            .take(num_descriptors),
                        );
                        buf
                    };

                    let _try = || loop {
                        let out_frames = output_device.pcm.avail_update()? as usize;
                        let in_frames = input_device.pcm.avail_update()? as usize;
                        if out_frames == 0 && in_frames == 0 {
                            let latency = in_latency.min(out_latency).round() as i32;
                            if alsa::poll::poll(&mut poll_descriptors, latency)? > 0 {
                                log::debug!("Eject requested, returning ownership of callback");
                                break Ok(callback);
                            }
                            continue;
                        }

                        log::debug!("[Output] Frames available: {out_frames}");
                        let out_frames = std::cmp::min(out_frames, out_periods);
                        let out_len = out_frames * out_channels;
                        let in_frames = std::cmp::min(in_frames, in_periods);
                        let in_len = in_frames * in_channels;

                        if let Err(err) = input_io.readi(&mut in_buffer[..in_len]) {
                            input_device.pcm.try_recover(err, true)?;
                        }

                        let context = AudioCallbackContext {
                            timestamp: out_timestamp,
                            stream_config: output_config,
                        };
                        let input = AudioInput {
                            timestamp: in_timestamp,
                            buffer: AudioRef::from_interleaved(&in_buffer[..in_len], in_channels)
                                .unwrap(),
                        };
                        let output = AudioOutput {
                            timestamp: out_timestamp,
                            buffer: AudioMut::from_interleaved_mut(
                                &mut out_buffer[..out_len],
                                out_channels,
                            )
                            .unwrap(),
                        };
                        callback.on_audio_data(context, input, output);

                        if let Err(err) = output_io.writei(&out_buffer[..out_len]) {
                            output_device.pcm.try_recover(err, true)?;
                        }

                        in_timestamp += in_frames as u64;
                        out_timestamp += out_frames as u64;

                        if input_device.ensure_state(&input_hwp)?
                            || output_device.ensure_state(&output_hwp)?
                        {
                            std::thread::sleep(Duration::from_secs(1));
                        }
                    };
                    _try().inspect_err(|err| log::error!("Error in duplex thread: {:?}", err))
                }
            });
            Ok(Self {
                eject_trigger: Arc::new(tx),
                join_handle,
            })
        }
    }
}
