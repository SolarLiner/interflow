use crate::audio_buffer::{AudioMut, AudioRef};
use crate::backends::alsa::device::AlsaDevice;
use crate::backends::alsa::{triggerfd, AlsaError};
use crate::timestamp::Timestamp;
use crate::{
    AudioCallback, AudioCallbackContext, AudioInput, AudioOutput, AudioStreamHandle,
    ResolvedStreamConfig, StreamConfig,
};
use alsa::pcm;
use alsa::PollDescriptors;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::thread::JoinHandle;

struct AlsaStreamData<'io> {
    pcm: Weak<pcm::PCM>,
    io: Option<pcm::IO<'io, f32>>,
    buffer: Box<[f32]>,
    timestamp: Timestamp,
    sample_rate: f64,
    channels: usize,
    max_frame_count: usize,
    latency: f64,
}

impl<'io> AlsaStreamData<'io> {
    fn new(device: &'io AlsaDevice, config: StreamConfig) -> Result<Self, AlsaError> {
        let (hwp, _, io) = device.apply_config(&config)?;
        let (_, period_size) = device.pcm.get_params()?;
        let period_size = period_size as usize;
        log::info!("[{}] Period size : {period_size}", &device.name);
        let channels = hwp.get_channels()? as usize;
        log::info!("[{}] channels: {channels}", &device.name);
        let sample_rate = hwp.get_rate()? as f64;
        log::info!("[{}] Sample rate : {sample_rate}", &device.name);

        let buffer = std::iter::repeat_n(0.0, period_size * channels).collect();
        let timestamp = Timestamp::new(sample_rate);
        let latency = period_size as f64 / sample_rate;

        Ok(Self {
            pcm: Rc::downgrade(&device.pcm),
            io: Some(io),
            buffer,
            timestamp,
            sample_rate,
            channels,
            max_frame_count: period_size,
            latency,
        })
    }

    fn new_empty() -> Self {
        Self {
            pcm: Weak::new(),
            io: None,
            buffer: Box::new([]),
            timestamp: Timestamp::new(0.0),
            sample_rate: 0.0,
            channels: 0,
            max_frame_count: 0,
            latency: 0.0,
        }
    }

    fn descriptor_count(&self) -> usize {
        let Some(pcm) = self.pcm.upgrade() else {
            return 0;
        };
        return pcm.count();
    }

    fn available(&self) -> Result<usize, AlsaError> {
        let Some(pcm) = self.pcm.upgrade() else {
            return Ok(0);
        };
        Ok(pcm.avail_update()? as usize)
    }

    fn read(&mut self, num_frames: usize) -> Result<AudioRef<f32>, AlsaError> {
        let Some(io) = self.io.as_mut() else {
            return Ok(self.get_buffer(0));
        };
        let len = num_frames * self.channels;
        let buffer = &mut self.buffer[..len];
        if let Err(err) = io.readi(buffer) {
            self.pcm.upgrade().unwrap().try_recover(err, true)?;
        }
        Ok(AudioRef::from_interleaved(buffer, self.channels).unwrap())
    }

    fn read_input(&mut self, num_frames: usize) -> Result<AudioInput<f32>, AlsaError> {
        Ok(AudioInput {
            timestamp: self.timestamp,
            buffer: self.read(num_frames)?,
        })
    }

    fn get_buffer(&self, num_frames: usize) -> AudioRef<f32> {
        let len = num_frames * self.channels;
        AudioRef::from_interleaved(&self.buffer[..len], self.channels).unwrap()
    }

    fn get_buffer_mut(&mut self, num_frames: usize) -> AudioMut<f32> {
        let len = num_frames * self.channels;
        AudioMut::from_interleaved_mut(&mut self.buffer[..len], self.channels).unwrap()
    }

    fn write(&mut self, num_frames: usize) -> Result<(), AlsaError> {
        let Some(io) = self.io.as_mut() else {
            return Ok(());
        };
        let len = num_frames * self.channels;
        let scratch = &self.buffer[..len];
        if let Err(err) = io.writei(scratch) {
            self.pcm.upgrade().unwrap().try_recover(err, true)?;
        }
        Ok(())
    }

    fn provide_output(&mut self, num_frames: usize) -> AudioOutput<f32> {
        AudioOutput {
            timestamp: self.timestamp,
            buffer: self.get_buffer_mut(num_frames),
        }
    }

    fn tick_timestamp(&mut self, samples: u64) {
        self.timestamp += samples;
    }
}

struct AlsaThread<Callback> {
    callback: Callback,
    eject_trigger: triggerfd::Receiver,
    stream_config: StreamConfig,
    input_device: Option<AlsaDevice>,
    output_device: Option<AlsaDevice>,
}

impl<Callback: AudioCallback> AlsaThread<Callback> {
    fn new(
        callback: Callback,
        eject_trigger: triggerfd::Receiver,
        stream_config: StreamConfig,
        input_device: Option<AlsaDevice>,
        output_device: Option<AlsaDevice>,
    ) -> Self {
        Self {
            callback,
            eject_trigger,
            stream_config,
            input_device,
            output_device,
        }
    }

    fn thread_loop(mut self) -> Result<Callback, AlsaError> {
        let mut input_stream = self
            .input_device
            .as_ref()
            .map(|d| AlsaStreamData::new(d, self.stream_config))
            .transpose()?
            .unwrap_or_else(AlsaStreamData::new_empty);
        let mut output_stream = self
            .output_device
            .as_ref()
            .map(|d| AlsaStreamData::new(d, self.stream_config))
            .transpose()?
            .unwrap_or_else(AlsaStreamData::new_empty);

        let stream_config = ResolvedStreamConfig {
            sample_rate: output_stream.sample_rate,
            input_channels: input_stream.channels,
            output_channels: output_stream.channels,
            max_frame_count: output_stream
                .max_frame_count
                .max(input_stream.max_frame_count),
        };
        let mut poll_descriptors = {
            let mut buf = vec![self.eject_trigger.as_pollfd()];
            let num_descriptors =
                input_stream.descriptor_count() + output_stream.descriptor_count();
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
        self.callback.prepare(AudioCallbackContext {
            stream_config,
            timestamp: Timestamp::new(stream_config.sample_rate),
        });
        loop {
            let out_frames = output_stream.available()?;
            let in_frames = input_stream.available()?;

            if out_frames == 0 && in_frames == 0 {
                let latency = input_stream.latency.round() as i32;
                if alsa::poll::poll(&mut poll_descriptors, latency)? > 0 {
                    log::debug!("Eject requested, returning ownership of callback");
                    break Ok(self.callback);
                }
                continue;
            }

            log::debug!("Frames available:  out {out_frames}, in {in_frames}");
            let context = AudioCallbackContext {
                timestamp: output_stream.timestamp,
                stream_config,
            };
            let input = input_stream.read_input(in_frames)?;
            let output = output_stream.provide_output(out_frames);
            self.callback.process_audio(context, input, output);
            input_stream.tick_timestamp(in_frames as u64);
            output_stream.tick_timestamp(out_frames as u64);
            output_stream.write(out_frames)?;
        }
    }
}

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
    pub(super) fn new(
        input_device: impl 'static + Send + FnOnce() -> Result<Option<AlsaDevice>, alsa::Error>,
        output_device: impl 'static + Send + FnOnce() -> Result<Option<AlsaDevice>, alsa::Error>,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, AlsaError> {
        let (tx, rx) = triggerfd::trigger()?;
        let join_handle = std::thread::spawn(move || {
            let worker = AlsaThread::new(
                callback,
                rx,
                stream_config,
                input_device()?,
                output_device()?,
            );
            worker.thread_loop()
        });
        Ok(Self {
            eject_trigger: Arc::new(tx),
            join_handle,
        })
    }
}
