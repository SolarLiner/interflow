use std::sync::Arc;

use asio_sys as asio;

use crate::stream::AudioStreamHandle;

use super::error::AsioError;

pub struct AsioStream<Callback> {
    pub driver: Arc<asio::Driver>,
    pub callback_id: asio::CallbackId,
    pub callback_retrieve: oneshot::Sender<oneshot::Sender<Callback>>,
}

impl<Callback> AudioStreamHandle<Callback> for AsioStream<Callback> {
    type Error = AsioError;

    fn eject(self) -> Result<Callback, Self::Error> {
        let (tx, rx) = oneshot::channel();
        self.callback_retrieve.send(tx).unwrap();
        let callback = rx.recv().unwrap();
        self.driver.stop()?;
        self.driver.remove_callback(self.callback_id);
        Ok(callback)
    }
}
