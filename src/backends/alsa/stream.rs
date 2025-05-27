use crate::backends::alsa::device::AlsaDevice;
use crate::backends::alsa::{triggerfd, AlsaError};
use crate::channel_map::{Bitset, ChannelMap32};
use crate::timestamp::Timestamp;
use crate::{
    AudioCallback, AudioCallbackContext, AudioStreamHandle, ResolvedStreamConfig, StreamConfig,
};
use alsa::PollDescriptors;
use alsa::{pcm, Direction};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Type of ALSA streams.
///
/// The audio stream implementation relies on the synchronous API for now, as the [`alsa`] crate
/// does not seem to wrap the asynchronous API as of now. A separate I/O thread is spawned when
/// creating a stream, and is stopped when caling [`AudioInputDevice::eject`] /
/// [`AudioOutputDevice::eject`].
pub struct AlsaStream<Callback> {
    pub(super) eject_trigger: Arc<triggerfd::Sender>,
    pub(super) join_handle: JoinHandle<Result<Callback, AlsaError>>,
}

impl<Callback> AudioStreamHandle<Callback> for AlsaStream<Callback> {
    type Error = AlsaError;

    fn eject(self) -> Result<Callback, Self::Error> {
        self.eject_trigger.trigger()?;
        self.join_handle.join().unwrap()
    }
}

impl<Callback: 'static + AudioCallback> AlsaStream<Callback> {
    pub(super) fn new_generic(
        stream_config: StreamConfig,
        device: impl 'static + Send + FnOnce() -> Result<AlsaDevice, alsa::Error>,
        mut callback: Callback,
        loop_callback: impl 'static
            + Send
            + Fn(
                StreamContext<Callback>,
                &dyn Fn(alsa::Error) -> Result<(), alsa::Error>,
            ) -> Result<(), alsa::Error>,
    ) -> Result<Self, AlsaError> {
        let (tx, rx) = triggerfd::trigger()?;
        let join_handle = std::thread::spawn({
            move || {
                let device = device()?;
                let recover = |err| device.pcm.try_recover(err, true);
                let mut poll_descriptors = {
                    let mut buf = vec![rx.as_pollfd()];
                    let num_descriptors = device.pcm.count();
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
                let (hwp, _, io) = device.apply_config(&stream_config)?;
                let (_, period_size) = device.pcm.get_params()?;
                let period_size = period_size as usize;
                log::info!("Period size : {period_size}");
                let num_channels = hwp.get_channels()? as usize;
                let (input_channels, output_channels) = match device.direction {
                    Direction::Playback => (0, num_channels),
                    Direction::Capture => (num_channels, 0),
                };
                log::info!("Num channels: {num_channels}");
                let sample_rate = hwp.get_rate()? as f64;
                log::info!("Sample rate : {sample_rate}");
                let stream_config = ResolvedStreamConfig {
                    sample_rate,
                    input_channels,
                    output_channels,
                    max_frame_count: period_size,
                };
                let mut timestamp = Timestamp::new(sample_rate);
                let mut buffer = vec![0f32; period_size * num_channels];
                let latency = period_size as f64 / sample_rate;
                device.pcm.prepare()?;
                if device.pcm.state() != pcm::State::Running {
                    log::info!("Device not already started, starting now");
                    device.pcm.start()?;
                }
                callback.prepare(AudioCallbackContext {
                    stream_config,
                    timestamp: Timestamp::new(sample_rate),
                });
                let _try = || loop {
                    let frames = device.pcm.avail_update()? as usize;
                    if frames == 0 {
                        let latency = latency.round() as i32;
                        if alsa::poll::poll(&mut poll_descriptors, latency)? > 0 {
                            log::debug!("Eject requested, returning ownership of callback");
                            break Ok(callback);
                        }
                        continue;
                    }

                    log::debug!("Frames available: {frames}");
                    let frames = std::cmp::min(frames, period_size);
                    let len = frames * num_channels;

                    loop_callback(
                        StreamContext {
                            config: &stream_config,
                            timestamp: &mut timestamp,
                            io: &io,
                            num_channels,
                            num_frames: frames,
                            buffer: &mut buffer[..len],
                            callback: &mut callback,
                        },
                        &recover,
                    )?;

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
        Ok(Self {
            eject_trigger: Arc::new(tx),
            join_handle,
        })
    }
}

pub(super) struct StreamContext<'a, Callback: 'a> {
    pub(super) config: &'a ResolvedStreamConfig,
    pub(super) timestamp: &'a mut Timestamp,
    pub(super) io: &'a pcm::IO<'a, f32>,
    pub(super) num_channels: usize,
    pub(super) num_frames: usize,
    pub(super) buffer: &'a mut [f32],
    pub(super) callback: &'a mut Callback,
}
