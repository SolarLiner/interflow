use interflow::audio_buffer::AudioRef;

#[derive(Debug, Copy, Clone)]
pub struct PeakMeter {
    last_out: f32,
    decay: f32,
    dt: f32,
}

impl PeakMeter {
    pub fn new(samplerate: f32, decay: f32) -> Self {
        Self {
            last_out: 0.,
            decay,
            dt: 1. / samplerate,
        }
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

    pub fn value(&self) -> f32 {
        self.last_out
    }
    pub fn process(&mut self, sample: f32) -> f32 {
        let k = f32::exp(-self.decay * self.dt);
        self.last_out = (k * sample).max(self.last_out);
        self.last_out
    }

    pub fn process_buffer(&mut self, buffer: AudioRef<f32>) -> f32 {
        let buffer_duration = buffer.num_samples() as f32 * self.dt;
        let peak_lin = buffer
            .channels()
            .flat_map(|ch| ch.iter().copied().max_by(f32::total_cmp))
            .max_by(f32::total_cmp)
            .unwrap_or(0.);
        self.last_out = peak_lin.max(self.last_out * f32::exp(-self.decay * buffer_duration));
        self.last_out
    }
}
