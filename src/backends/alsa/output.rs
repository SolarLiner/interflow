use crate::audio_buffer::AudioMut;
use crate::backends::alsa::stream::AlsaStream;
use crate::backends::alsa::AlsaError;
use crate::prelude::alsa::device::AlsaDevice;
use crate::stream::AudioOutput;
use crate::stream::{AudioCallbackContext, AudioOutputCallback, StreamConfig};

impl<Callback: 'static + Send + AudioOutputCallback> AlsaStream<Callback> {
    pub(super) fn new_output(
        name: String,
        stream_config: StreamConfig,
        callback: Callback,
    ) -> Result<Self, AlsaError> {
        Self::new_generic(
            stream_config,
            move || AlsaDevice::new(&name, alsa::Direction::Playback),
            callback,
            move |ctx, recover| {
                let context = AudioCallbackContext {
                    stream_config,
                    timestamp: *ctx.timestamp,
                };
                let input = AudioOutput {
                    buffer: AudioMut::from_interleaved_mut(&mut ctx.buffer[..], ctx.num_channels)
                        .unwrap(),
                    timestamp: *ctx.timestamp,
                };
                ctx.callback.on_output_data(context, input);
                *ctx.timestamp += ctx.num_frames as u64;
                if let Err(err) = ctx.io.writei(&ctx.buffer[..]) {
                    recover(err)?;
                }
                Ok(())
            },
        )
    }
}
