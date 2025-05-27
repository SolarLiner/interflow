use crate::audio_buffer::AudioRef;
use crate::backends::alsa::stream::AlsaStream;
use crate::backends::alsa::AlsaError;
use crate::prelude::alsa::device::AlsaDevice;
use crate::prelude::{AudioMut, Timestamp};
use crate::{AudioCallback, AudioCallbackContext, AudioInput, AudioOutput, StreamConfig};

impl<Callback: 'static + Send + AudioCallback> AlsaStream<Callback> {
    pub(super) fn new_input(
        name: String,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, AlsaError> {
        Self::new_generic(
            stream_config,
            move || AlsaDevice::new(&name, alsa::Direction::Capture),
            callback,
            move |ctx, recover| {
                if let Err(err) = ctx.io.readi(&mut ctx.buffer[..]) {
                    recover(err)?;
                }
                let buffer = AudioRef::from_interleaved(ctx.buffer, ctx.num_channels).unwrap();
                let context = AudioCallbackContext {
                    stream_config: *ctx.config,
                    timestamp: *ctx.timestamp,
                };
                let input = AudioInput {
                    buffer,
                    timestamp: *ctx.timestamp,
                };
                let dummy_output = AudioOutput {
                    timestamp: Timestamp::new(ctx.config.sample_rate),
                    buffer: AudioMut::empty(),
                };
                ctx.callback.process_audio(context, input, dummy_output);
                *ctx.timestamp += ctx.num_frames as u64;
                Ok(())
            },
        )
    }
}
