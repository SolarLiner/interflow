use std::f32::consts::TAU;

use interflow_core::{device::Device, platform::Platform as _, stream::Callback, DeviceType};
use interflow_coreaudio::Platform;

fn main() {
    Platform
        .default_device(DeviceType::PHYSICAL | DeviceType::OUTPUT)
        .unwrap()
        .default_stream(DeviceType::OUTPUT, Sine::new(440.0))
        .unwrap();
}

struct Sine {
    phase: f32,
    frequency: f32,
    step: f32,
}

impl Sine {
    fn new(frequency: f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            step: 0.0,
        }
    }
}

impl Callback for Sine {
    fn prepare(&mut self, context: interflow_core::stream::CallbackContext) {
        self.step = self.frequency * context.stream_config.sample_rate as f32;
        self.phase = 0.0;
    }

    fn process_audio(
        &mut self,
        context: interflow_core::stream::CallbackContext,
        input: interflow_core::stream::AudioInput<f32>,
        mut output: interflow_core::stream::AudioOutput<f32>,
    ) {
        let num_frames = output.buffer.channel(0).len();
        for i in 0..num_frames {
            let v = (self.phase * TAU).sin() * 0.125;
            self.phase += self.step;
            while self.phase > 1.0 {
                self.phase -= 1.0;
            }
            output.buffer.frame_mut(i).set_mono(v);
        }
    }
}
