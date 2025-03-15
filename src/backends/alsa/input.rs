use crate::audio_buffer::AudioRef;
use crate::backends::alsa::device::AlsaDevice;
use crate::backends::alsa::stream::AlsaStream;
use crate::channel_map::{Bitset, ChannelMap32};
use crate::prelude::Timestamp;
use crate::{AudioCallbackContext, AudioInput, AudioInputCallback, StreamConfig};
use alsa::pcm;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

impl<Callback: 'static + Send + AudioInputCallback> AlsaStream<Callback> {
    pub(super) fn new_input(
        name: String,
        stream_config: StreamConfig,
        mut callback: Callback,
    ) -> Self {
        let eject_signal = Arc::new(AtomicBool::new(false));
        let join_handle = std::thread::spawn({
            let eject_signal = eject_signal.clone();
            move || {
                let device = AlsaDevice::new(&name, alsa::Direction::Capture)?;
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
                let mut timestamp = Timestamp::new(samplerate);
                let mut buffer = vec![0f32; period_size * num_channels];
                device.pcm.prepare()?;
                if device.pcm.state() != pcm::State::Running {
                    log::info!("Device not already started, starting now");
                    device.pcm.start()?;
                }
                let _try = || loop {
                    if eject_signal.load(Ordering::Relaxed) {
                        log::debug!("Eject requested, returning ownership of callback");
                        break Ok(callback);
                    }

                    let frames = device.pcm.avail_update()? as usize;
                    if frames == 0 {
                        // TODO: Polling for proper wakeup
                        std::thread::yield_now();
                        continue;
                    }

                    log::debug!("Frames available: {frames}");
                    let frames = std::cmp::min(frames, period_size);
                    let len = frames * num_channels;
                    if let Err(err) = io.readi(&mut buffer[..len]) {
                        log::warn!("ALSA PCM error, trying to recover ...");
                        log::debug!("Error: {err}");
                        device.pcm.try_recover(err, true)?;
                    }
                    let buffer = AudioRef::from_interleaved(&buffer[..len], num_channels).unwrap();
                    let context = AudioCallbackContext {
                        stream_config,
                        timestamp,
                    };
                    let input = AudioInput { buffer, timestamp };
                    callback.on_input_data(context, input);
                    timestamp += frames as u64;

                    match device.pcm.state() {
                        pcm::State::Suspended => {
                            if hwp.can_resume() {
                                device.pcm.resume()?;
                            } else {
                                device.pcm.prepare()?;
                            }
                        }
                        pcm::State::Paused => std::thread::sleep(Duration::from_secs(1)),
                        _ => {}
                    }
                };
                _try()
            }
        });
        Self {
            eject_signal,
            join_handle,
        }
    }
}
