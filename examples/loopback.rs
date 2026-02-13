use crate::util::meter::PeakMeter;
use crate::util::AtomicF32;
use anyhow::Result;
use interflow::prelude::*;
use std::sync::Arc;
mod util;

fn main() -> Result<()> {
    env_logger::init();

    let input = default_input_device();
    let output = default_output_device();
    log::info!("Opening input : {}", input.name());
    log::info!("Opening output: {}", output.name());
    let config = StreamConfig {
        buffer_size_range: (Some(128), Some(512)),
        input_channels: 1,
        output_channels: 1,
        ..output.default_config().unwrap()
    };
    let value = Arc::new(AtomicF32::new(0.));
    let config = DuplexStreamConfig::new(config);
    let stream =
        create_duplex_stream(input, output, Loopback::new(44100., value.clone()), config).unwrap();
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

impl AudioCallback for Loopback {
    fn prepare(&mut self, context: AudioCallbackContext) {}
    fn process_audio(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        mut output: AudioOutput<f32>,
    ) {
        self.meter
            .set_samplerate(context.stream_config.sample_rate as f32);
        let rms = self.meter.process_buffer(input.buffer.as_ref());
        self.value.store(rms, std::sync::atomic::Ordering::Relaxed);
        output.buffer.as_interleaved_mut().fill(0.0);
        output.buffer.mix(input.buffer.as_ref(), 1.0);
    }
}
