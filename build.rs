use cfg_aliases::cfg_aliases;

fn main() {
    // Setup cfg aliases
    cfg_aliases! {
        os_alsa: { any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd",
            target_os = "netbsd") }
    }
}