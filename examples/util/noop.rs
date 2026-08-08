use interflow::prelude::*;
use interflow_core::stream::CallbackContext;

pub struct Noop;

impl stream::Callback for Noop {
    fn prepare(&mut self, context: CallbackContext) {}

    fn process_audio(
        &mut self,
        context: CallbackContext,
        input: &AudioInput<f32>,
        output: &mut AudioOutput<f32>,
    ) {
    }
}
