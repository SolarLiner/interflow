use crate::util::meter::PeakMeter;
use crate::util::AtomicF32;
use anyhow::Result;
use interflow::prelude::*;
use std::sync::Arc;

mod util;

fn main() -> Result<()> {
    env_logger::init();

    let device = default_input_device();
    let value = Arc::new(AtomicF32::new(0.));
    let stream = device
        .default_input_stream(RmsMeter::new(value.clone()))
        .unwrap();
    util::display_peakmeter(value)?;
    stream.eject().unwrap();
    Ok(())
}

struct RmsMeter {
    value: Arc<AtomicF32>,
    meter: Option<PeakMeter>,
}

impl RmsMeter {
    fn new(value: Arc<AtomicF32>) -> Self {
        Self { meter: None, value }
    }
}

impl AudioInputCallback for RmsMeter {
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>) {
        let meter = self
            .meter
            .get_or_insert_with(|| PeakMeter::new(context.stream_config.samplerate as f32, 15.0));
        meter.set_samplerate(context.stream_config.samplerate as f32);
        meter.process_buffer(input.buffer.as_ref());
        self.value
            .store(meter.value(), std::sync::atomic::Ordering::Relaxed);
    }
}
