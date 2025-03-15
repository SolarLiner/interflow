use crate::util::sine::SineWave;
use anyhow::Result;
use interflow::duplex::AudioDuplexCallback;
use interflow::prelude::*;

mod util;

#[cfg(os_alsa)]
fn main() -> Result<()> {
    env_logger::init();

    let device = default_duplex_device();
    let mut config = device.default_duplex_config().unwrap();
    config.buffer_size_range = (Some(128), Some(512));
    let stream = device.create_duplex_stream(config, RingMod::new()).unwrap();
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject().unwrap();
    Ok(())
}

#[cfg(not(os_alsa))]
fn main() -> Result<()> {
    let input = default_input_device();
    let output = default_output_device();
    let mut input_config = input.default_input_config().unwrap();
    input_config.buffer_size_range = (Some(128), Some(512));
    let mut output_config = output.default_output_config().unwrap();
    output_config.buffer_size_range = (Some(128), Some(512));
    let stream =
        duplex::create_duplex_stream(input, input_config, output, output_config, RingMod::new())
            .unwrap();
    println!("Press Enter to stop");
    std::io::stdin().read_line(&mut String::new())?;
    stream.eject().unwrap();
    Ok(())
}

struct RingMod {
    carrier: SineWave,
}

impl RingMod {
    fn new() -> Self {
        Self {
            carrier: SineWave::new(440.0),
        }
    }
}

impl AudioDuplexCallback for RingMod {
    fn on_audio_data(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        mut output: AudioOutput<f32>,
    ) {
        if input.buffer.num_samples() < output.buffer.num_samples() {
            log::error!("Input underrun");
        }
        let sr = context.stream_config.samplerate as f32;
        let num_samples = output.buffer.num_samples().min(input.buffer.num_samples());
        for i in 0..num_samples {
            let inp = input.buffer.get_frame(i)[0];
            let c = self.carrier.next_sample(sr);
            output.buffer.set_mono(i, inp * c);
        }
    }
}
