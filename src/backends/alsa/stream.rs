use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use crate::{AudioInputDevice, AudioOutputDevice, AudioStreamHandle};
use crate::backends::alsa::AlsaError;

/// Type of ALSA streams.
///
/// The audio stream implementation relies on the synchronous API for now, as the [`alsa`] crate
/// does not seem to wrap the asynchronous API as of now. A separate I/O thread is spawned when
/// creating a stream, and is stopped when caling [`AudioInputDevice::eject`] /
/// [`AudioOutputDevice::eject`].
pub struct AlsaStream<Callback> {
    pub(super) eject_signal: Arc<AtomicBool>,
    pub(super) join_handle: JoinHandle<Result<Callback, AlsaError>>,
}

impl<Callback> AudioStreamHandle<Callback> for AlsaStream<Callback> {
    type Error = AlsaError;

    fn eject(self) -> Result<Callback, Self::Error> {
        self.eject_signal.store(true, Ordering::Relaxed);
        self.join_handle.join().unwrap()
    }
}