use crate::util::AtomicF32;
use interflow::core::buffer::AudioRef;
use interflow_core::prelude::{AtomicTimestamp, AudioInput, AudioOutput};
use interflow_core::stream;
use interflow_core::stream::CallbackContext;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Clone)]
pub struct PeakMeter {
    output: Arc<AtomicF32>,
    last_out: f32,
    decay: f32,
    dt: f32,
}

impl PeakMeter {
    pub fn new(decay: f32) -> Self {
        Self {
            output: Arc::new(AtomicF32::new(0.)),
            last_out: 0.,
            decay,
            dt: 0.0,
        }
    }

    pub fn output(&self) -> Arc<AtomicF32> {
        self.output.clone()
    }

    pub fn samplerate(&self) -> f32 {
        1. / self.dt
    }

    pub fn set_samplerate(&mut self, samplerate: f32) {
        self.dt = 1. / samplerate;
    }

    pub fn decay(&self) -> f32 {
        self.decay
    }

    pub fn set_decay(&mut self, decay: f32) {
        self.decay = decay;
    }

    pub fn process(&mut self, sample: f32) -> f32 {
        let k = f32::exp(-self.decay * self.dt);
        self.last_out = (k * sample).max(self.last_out);
        self.output.store(self.last_out, Ordering::Relaxed);
        self.last_out
    }

    pub fn process_buffer(&mut self, buffer: AudioRef<f32>) -> f32 {
        let buffer_duration = buffer.frames() as f32 * self.dt;
        let peak_lin = (0..buffer.channels())
            .flat_map(|ch| buffer[ch].iter().copied().max_by(f32::total_cmp))
            .max_by(f32::total_cmp)
            .unwrap_or(0.);
        self.last_out = peak_lin.max(self.last_out * f32::exp(-self.decay * buffer_duration));
        self.output.store(self.last_out, Ordering::Relaxed);
        self.last_out
    }
}

#[derive(Clone)]
pub struct Metered<C: stream::Callback> {
    pub inner: C,
    input: PeakMeter,
    output: PeakMeter,
    timestamp: Arc<AtomicTimestamp>,
}

impl<C: stream::Callback> Metered<C> {
    pub fn new(inner: C, meter_decay: f32) -> Self {
        Self {
            inner,
            input: PeakMeter::new(meter_decay),
            output: PeakMeter::new(meter_decay),
            timestamp: Arc::new(AtomicTimestamp::zeroed()),
        }
    }

    pub fn shared(&self) -> MeteredShared {
        MeteredShared {
            input: self.input.output.clone(),
            output: self.output.output.clone(),
            timestamp: self.timestamp.clone(),
        }
    }
}

impl<C: stream::Callback> stream::Callback for Metered<C> {
    fn prepare(&mut self, context: CallbackContext) {
        self.input
            .set_samplerate(context.stream_config.sample_rate as _);
        self.output
            .set_samplerate(context.stream_config.sample_rate as _);
        self.inner.prepare(context);
    }

    fn process_audio(
        &mut self,
        context: CallbackContext,
        input: &AudioInput<f32>,
        output: &mut AudioOutput<f32>,
    ) {
        self.timestamp.update(context.timestamp);
        self.input.process_buffer(input.buffer);
        self.inner.process_audio(context, input, output);
        self.output.process_buffer(output.buffer.as_ref());
    }
}

pub struct MeteredShared {
    pub input: Arc<AtomicF32>,
    pub output: Arc<AtomicF32>,
    pub timestamp: Arc<AtomicTimestamp>,
}
