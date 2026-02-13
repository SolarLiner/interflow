use windows::Win32::Media::Audio::IAudioClient;
use crate::util::CoTask;

struct Handle {
    audio_client: CoTask<IAudioClient>,
}