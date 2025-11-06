//! This example demonstrates how to request a specific buffer size from the CoreAudio backend.
//! Probably only works on macOS.

mod util;

#[cfg(os_coreaudio)]
fn main() -> anyhow::Result<()> {
    use interflow::backends::coreaudio::CoreAudioDriver;
    use interflow::channel_map::CreateBitset;
    use interflow::prelude::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use util::sine::SineWave;

    struct MyCallback {
        first_callback: Arc<AtomicBool>,
        sine_wave: SineWave,
    }

    impl AudioCallback for MyCallback {
        fn prepare(&mut self, context: AudioCallbackContext) {
            self.sine_wave.prepare(context);
        }

        fn process_audio(&mut self, _: AudioCallbackContext, _: AudioInput<f32>, mut output: AudioOutput<f32>) {
            if self.first_callback.swap(false, Ordering::SeqCst) {
                println!(
                    "Actual buffer size granted by OS: {}",
                    output.buffer.num_frames()
                );
            }

            for mut frame in output.buffer.as_interleaved_mut().rows_mut() {
                let sample = self
                    .sine_wave
                    .next_sample();
                for channel_sample in &mut frame {
                    *channel_sample = sample;
                }
            }
        }
    }

    env_logger::init();

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
        sample_rate: 48000.0,
        input_channels: 0,
        output_channels: 2,
        buffer_size_range: (Some(requested_buffer_size), Some(requested_buffer_size)),
        exclusive: false,
    };

    let callback = MyCallback {
        first_callback: Arc::new(AtomicBool::new(true)),
        sine_wave: SineWave::new(440.0),
    };

    let stream = device.create_stream(stream_config, callback)?;

    println!("Playing sine wave... Press enter to stop.");
    std::io::stdin().read_line(&mut String::new())?;

    stream.eject()?;
    Ok(())
}

#[cfg(not(os_coreaudio))]
fn main() {
    println!("This example is only available on platforms that support CoreAudio (e.g. macOS).");
}
