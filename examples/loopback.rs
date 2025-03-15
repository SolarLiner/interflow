use crate::util::meter::PeakMeter;
use crate::util::AtomicF32;
use anyhow::Result;
use interflow::duplex::AudioDuplexCallback;
use interflow::prelude::*;
use std::sync::Arc;
mod util;

#[cfg(os_alsa)]
fn main() -> Result<()> {
    env_logger::init();

    let device = default_duplex_device();
    let mut config = device.default_duplex_config().unwrap();
    config.buffer_size_range = (Some(128), Some(512));
    let value = Arc::new(AtomicF32::new(0.0));
    let stream = device
        .create_duplex_stream(config, Loopback::new(44100., value.clone()))
        .unwrap();
    util::display_peakmeter(value)?;
    stream.eject().unwrap();
    Ok(())
}

#[cfg(not(os_alsa))]
fn main() -> Result<()> {
    env_logger::init();

    let input = default_input_device();
    let output = default_output_device();
    let mut input_config = input.default_input_config().unwrap();
    input_config.buffer_size_range = (Some(128), Some(512));
    let mut output_config = output.default_output_config().unwrap();
    output_config.buffer_size_range = (Some(128), Some(512));
    input_config.channels = 0b01;
    output_config.channels = 0b11;
    let value = Arc::new(AtomicF32::new(0.));
    let stream = duplex::create_duplex_stream(
        input,
        input_config,
        output,
        output_config,
        Loopback::new(44100., value.clone()),
    )
    .unwrap();
    util::display_peakmeter(value)?;
    stream.eject().unwrap();
    Ok(())
}

struct Loopback {
    meter: PeakMeter,
    value: Arc<AtomicF32>,
}

impl Loopback {
    fn new(samplerate: f32, value: Arc<AtomicF32>) -> Self {
        Self {
            meter: PeakMeter::new(samplerate, 15.0),
            value,
        }
    }
}

impl AudioDuplexCallback for Loopback {
    fn on_audio_data(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        mut output: AudioOutput<f32>,
    ) {
        self.meter
            .set_samplerate(context.stream_config.samplerate as f32);
        let rms = self.meter.process_buffer(input.buffer.as_ref());
        self.value.store(rms, std::sync::atomic::Ordering::Relaxed);
        output.buffer.as_interleaved_mut().fill(0.0);
        output.buffer.mix(input.buffer.as_ref(), 1.0);
    }
}
