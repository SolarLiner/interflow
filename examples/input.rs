use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};

use interflow::prelude::*;
use interflow::timestamp::Timestamp;

fn main() -> Result<()> {
    env_logger::init();
    
    let device = default_input_device();
    let stream = device
        .default_input_stream(RmsMeter::default())
        .unwrap();
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new()).unwrap();
    let meter = stream.eject().unwrap();
    meter.progress.finish_and_clear();
    Ok(())
}

struct RmsMeter {
    progress: ProgressBar,
    last_out: f32,
    last_show: f64,
}

impl Default for RmsMeter {
    fn default() -> Self {
        let progress = ProgressBar::new(100);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{bar:40.green} {msg}")
                .unwrap(),
        );
        Self {
            progress,
            last_out: 0.,
            last_show: f64::NEG_INFINITY,
        }
    }
}

impl AudioInputCallback for RmsMeter {
    fn on_input_data(&mut self, context: AudioCallbackContext, input: AudioInput<f32>) {
        let buffer_duration =
            Timestamp::from_count(input.timestamp.samplerate, input.buffer.num_samples() as _);
        let peak_lin = input
            .buffer
            .channels()
            .flat_map(|ch| ch.iter().copied().max_by(f32::total_cmp))
            .max_by(f32::total_cmp)
            .unwrap_or(0.);
        let rms_lin =
            peak_lin.max(self.last_out * f32::exp(-15. * buffer_duration.as_seconds() as f32));
        self.last_out = rms_lin;

        let time = context.timestamp.as_seconds();
        if time > self.last_show + 50e-3 {
            let peak_db = 20. * rms_lin.log10();
            let pc = normalize(-60., 6., peak_db);
            let pos = if let Some(len) = self.progress.length() {
                pc * len as f32
            } else {
                self.progress.set_length(100);
                100. * pc
            };
            self.progress.set_position(pos as _);
            self.progress
                .set_message(format!("Peak: {peak_db:2.1} dB | Runtime: {time:2.3} s"));
            self.last_show = time;
        }
    }
}

fn normalize(min: f32, max: f32, value: f32) -> f32 {
    let range = max - min;
    (value - min) / range
}
