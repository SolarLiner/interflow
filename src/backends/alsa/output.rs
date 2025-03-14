use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use alsa::pcm;
use std::time::Duration;
use crate::{AudioCallbackContext, AudioOutput, AudioOutputCallback, StreamConfig};
use crate::audio_buffer::AudioMut;
use crate::backends::alsa::stream::AlsaStream;
use crate::backends::alsa::device::AlsaDevice;
use crate::channel_map::{Bitset, ChannelMap32};
use crate::prelude::Timestamp;

impl<Callback: 'static + Send + AudioOutputCallback> AlsaStream<Callback> {
    pub(super) fn new_output(name: String, stream_config: StreamConfig, mut callback: Callback) -> Self {
        let eject_signal = Arc::new(AtomicBool::new(false));
        let join_handle = std::thread::spawn({
            let eject_signal = eject_signal.clone();
            move || {
                let device = AlsaDevice::new(&name, alsa::Direction::Playback)?;
                let (hwp, _, io) = device.apply_config(&stream_config)?;
                let (_, period_size) = device.pcm.get_params()?;
                let period_size = period_size as usize;
                log::info!("Period size : {period_size}");
                let num_channels = hwp.get_channels()? as usize;
                log::info!("Num channels: {num_channels}");
                let samplerate = hwp.get_rate()? as f64;
                log::info!("Sample rate : {samplerate}");
                let stream_config = StreamConfig {
                    samplerate,
                    channels: ChannelMap32::default()
                        .with_indices(std::iter::repeat(1).take(num_channels)),
                    buffer_size_range: (Some(period_size), Some(period_size)),
                    exclusive: false,
                };
                let frames = device.pcm.avail_update()? as usize;
                log::info!("[avail_update] frames: {frames}");
                let mut timestamp = Timestamp::new(samplerate);
                let mut buffer = vec![0f32; frames * num_channels];
                device.pcm.prepare()?;
                if device.pcm.state() != pcm::State::Running {
                    device.pcm.start()?;
                }
                let _try = || loop {
                    if eject_signal.load(Ordering::Relaxed) {
                        break Ok(callback);
                    }
                    
                    let frames = device.pcm.avail_update()? as usize;
                    if frames == 0 {
                        // TODO: Polling for proper wakeup
                        std::thread::yield_now();
                        continue;
                    }

                    log::debug!("Frames available: {frames}");
                    let len = frames * num_channels;
                    let context = AudioCallbackContext {
                        stream_config,
                        timestamp,
                    };
                    let input = AudioOutput {
                        buffer: AudioMut::from_interleaved_mut(&mut buffer[..len], num_channels)
                            .unwrap(),
                        timestamp,
                    };
                    callback.on_output_data(context, input);
                    timestamp += frames as u64;
                    if let Err(err) = io.writei(&buffer[..len]) {
                        device.pcm.try_recover(err, true)?
                    }
                    match device.pcm.state() {
                        pcm::State::Suspended => {
                            if hwp.can_resume() {
                                log::debug!("Stream suspended, resuming");
                                device.pcm.resume()?;
                            } else {
                                log::debug!(
                                    "Stream suspended but cannot resume, re-prepare instead"
                                );
                                device.pcm.prepare()?;
                            }
                        }
                        pcm::State::Paused => std::thread::sleep(Duration::from_secs(1)),
                        _ => {}
                    }
                };
                _try().inspect_err(|err| log::error!("Audio thread error: {err}"))
            }
        });
        Self {
            eject_signal,
            join_handle,
        }
    }
}