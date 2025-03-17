use std::{marker::PhantomData, sync::{atomic::AtomicBool, Arc}};

use asio_sys::{self as asio, CallbackId};

use crate::stream::AudioStreamHandle;

use super::error::AsioError;

pub struct AsioStream<Callback> {
    playing: Arc<AtomicBool>,
    // driver: Arc<asio::Driver>,
    // streams: Arc<asio::AsioStreams>,
    callback_id: asio::CallbackId,
    callback: PhantomData<Callback>,
}

impl<Callback> AudioStreamHandle<Callback> for AsioStream<Callback> {
    
    type Error = AsioError;
    
    fn eject(self) -> Result<Callback, Self::Error> {
        todo!()
    }
}

impl<Callback> AsioStream<Callback> {
    pub fn new(playing: Arc<AtomicBool>, callback_id: CallbackId) -> Self {
        AsioStream { playing, callback_id, callback: PhantomData }
    }

    
}