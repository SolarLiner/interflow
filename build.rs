use cfg_aliases::cfg_aliases;

fn main() {
    // Setup cfg aliases
    cfg_aliases! {
        wasm: { any(target_os = "wasm32") },
        os_alsa: { any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd",
            target_os = "netbsd") },
        os_coreaudio: { any (target_os = "macos", target_os = "ios") },
        os_pipewire: { any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd") },
        os_wasapi: { target_os = "windows" },
        unsupported: { not(any(os_alsa, os_coreaudio, os_wasapi)) }
    }
}
