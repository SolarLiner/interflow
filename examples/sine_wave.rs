use std::f32::consts::TAU;

use anyhow::Result;

use interflow::prelude::*;

fn main() -> Result<()> {
    let device = default_output_device();
    let config = StreamConfig {
        samplerate: 48000.,
        channels: 0b11,
        buffer_size_range: (None, None),
    };
    assert!(device.is_config_supported(&config));
    println!("Using device {}", device.name());
    let stream = device.create_output_stream(
        config,
        SineWave {
            frequency: 440.,
            phase: 0.,
        },
    ).unwrap();
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject().unwrap();
    Ok(())
}

struct SineWave {
    frequency: f32,
    phase: f32,
}

impl AudioOutputCallback for SineWave {
    fn on_output_data(&mut self, context: AudioCallbackContext, mut output: AudioOutput<f32>) {
        eprintln!("Callback called, timestamp: {:2.3} s", context.timestamp.as_seconds());
        let sr = context.timestamp.samplerate as f32;
        for i in 0..output.buffer.num_samples() {
            output.buffer.set_mono(i, self.next_sample(sr));
        }
        // Reduce amplitude to not blow up speakers and ears
        output.buffer.change_amplitude(0.125);
    }
}

impl SineWave {
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
