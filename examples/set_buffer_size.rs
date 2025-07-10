//! This example demonstrates how to request a specific buffer size from the CoreAudio backend.
//! Probably only works on macOS.

use anyhow::Result;
use interflow::backends::coreaudio::CoreAudioDriver;
use interflow::channel_map::{ChannelMap32, CreateBitset};
use interflow::prelude::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use util::sine::SineWave;

mod util;

struct MyCallback {
    first_callback: Arc<AtomicBool>,
    sine_wave: SineWave,
}

impl AudioOutputCallback for MyCallback {
    fn on_output_data(&mut self, context: AudioCallbackContext, mut output: AudioOutput<f32>) {
        if self.first_callback.swap(false, Ordering::SeqCst) {
            println!(
                "Actual buffer size granted by OS: {}",
                output.buffer.num_samples()
            );
        }

        for mut frame in output.buffer.as_interleaved_mut().rows_mut() {
            let sample = self
                .sine_wave
                .next_sample(context.stream_config.samplerate as f32);
            for channel_sample in &mut frame {
                *channel_sample = sample;
            }
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();

    // We can only use the CoreAudio backend on macOS.
    if !cfg!(target_os = "macos") {
        println!("This example only runs on macOS.");
        return Ok(());
    }

    let driver = CoreAudioDriver;
    let device = driver
        .default_device(DeviceType::OUTPUT)
        .expect("Failed to query for default output device")
        .expect("No default output device found on this system");

    println!("Using device: {}", device.name());

    if let Ok((min, max)) = device.buffer_size_range() {
        println!(
            "Supported buffer size range: min={}, max={}",
            min.map_or_else(|| "N/A".to_string(), |v| v.to_string()),
            max.map_or_else(|| "N/A".to_string(), |v| v.to_string())
        );
    }

    let requested_buffer_size = 256;
    println!("Requesting buffer size: {}", requested_buffer_size);

    let stream_config = StreamConfig {
        samplerate: 48000.0,
        channels: ChannelMap32::from_indices([0, 1]),
        buffer_size_range: (Some(requested_buffer_size), Some(requested_buffer_size)),
        exclusive: false,
    };

    let callback = MyCallback {
        first_callback: Arc::new(AtomicBool::new(true)),
        sine_wave: SineWave::new(440.0),
    };

    let stream = device.create_output_stream(stream_config, callback)?;

    println!("Playing sine wave... Press enter to stop.");
    std::io::stdin().read_line(&mut String::new())?;

    stream.eject()?;
    Ok(())
}
