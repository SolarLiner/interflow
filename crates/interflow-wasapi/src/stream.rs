use crate::util::CoTask;
use windows::Win32::Media::Audio::IAudioClient;

struct Handle {
    audio_client: CoTask<IAudioClient>,
}
