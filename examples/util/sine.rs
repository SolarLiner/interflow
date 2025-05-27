use interflow::{AudioCallback, AudioCallbackContext, AudioInput, AudioOutput};
use std::f32::consts::TAU;

pub struct SineWave {
    pub frequency: f32,
    pub phase: f32,
}

impl AudioCallback for SineWave {
    fn prepare(&mut self, _context: AudioCallbackContext) {}
    fn process_audio(
        &mut self,
        context: AudioCallbackContext,
        _input: AudioInput<f32>,
        mut output: AudioOutput<f32>,
    ) {
        eprintln!(
            "Callback called, timestamp: {:2.3} s",
            context.timestamp.as_seconds()
        );
        let sr = context.timestamp.samplerate as f32;
        for i in 0..output.buffer.num_frames() {
            output.buffer.set_mono(i, self.next_sample(sr));
        }
        // Reduce amplitude to not blow up speakers and ears
        output.buffer.change_amplitude(0.125);
    }
}

impl SineWave {
    pub fn new(frequency: f32) -> Self {
        Self {
            frequency,
            phase: 0.0,
        }
    }

    pub fn next_sample(&mut self, samplerate: f32) -> f32 {
        let step = samplerate.recip() * self.frequency;
        let y = (TAU * self.phase).sin();
        self.phase += step;
        if self.phase > 1. {
            self.phase -= 1.;
        }
        y
    }
}
