use crate::util::sine::SineWave;
use anyhow::Result;
use interflow::prelude::*;

mod util;

//noinspection RsUnwrap
fn main() -> Result<()> {
    env_logger::init();
    let input = default_input_device();
    let output = default_output_device();
    log::info!("Opening input: {}", input.name());
    log::info!("Opening output: {}", output.name());
    let config = StreamConfig {
        buffer_size_range: (Some(128), Some(512)),
        input_channels: 1,
        ..output.default_config().unwrap()
    };
    let duplex_config = DuplexStreamConfig::new(config);
    let stream = create_duplex_stream(input, output, RingMod::new(), duplex_config).unwrap();
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

impl AudioCallback for RingMod {
    fn prepare(&mut self, context: AudioCallbackContext) {
        self.carrier.prepare(context);
    }

    fn process_audio(
        &mut self,
        context: AudioCallbackContext,
        input: AudioInput<f32>,
        mut output: AudioOutput<f32>,
    ) {
        let sr = context.stream_config.sample_rate as f32;
        for i in 0..output.buffer.num_frames() {
            let inp = input.buffer.get_frame(i)[0];
            let c = self.carrier.next_sample();
            output.buffer.set_mono(i, inp * c);
        }
    }
}
