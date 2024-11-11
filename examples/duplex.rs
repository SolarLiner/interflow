use crate::util::sine::SineWave;
use anyhow::Result;
use interflow::duplex::AudioDuplexCallback;
use interflow::prelude::*;

mod util;

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
        let sr = context.stream_config.samplerate as f32;
        for i in 0..output.buffer.num_samples() {
            let inp = input.buffer.get_frame(i)[0];
            let c = self.carrier.next_sample(sr);
            output.buffer.set_mono(i, inp * c);
        }
    }
}
