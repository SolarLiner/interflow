# Interflow

[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](CODE_OF_CONDUCT.md)
![GitHub branch check runs](https://img.shields.io/github/check-runs/SolarLiner/interflow/main)
![GitHub issue custom search in repo](https://img.shields.io/github/issues-search/SolarLiner/interflow?query=is%3Aissue%20state%3Aopen&label=issues)
![Discord](https://img.shields.io/discord/590254806208217089?label=RustAudio%20on%20Discord)

Interflow is a Rust library that abstracts away platform-specific audio APIs
and provides a unified, opinionated interface for audio applications. It aims
to simplify the development of audio applications by offering seamless support
for duplex audio with separate input and output devices, as well as sample rate
and format conversion.

## Features

- [x] Unified interface for platform-specific audio APIs.
- [x] Support for duplex audio (simultaneous input and output).
- [x] Separate input and output devices.
- [ ] Sample rate conversion.
- [x] Format conversion.

## Supported drivers

- [x] WASAPI
- [ ] ASIO
- [ ] ALSA
- [ ] PulseAudio
- [ ] PipeWire
- [ ] JACK
- [x] CoreAudio

## Getting started

Add `interflow` to the the list of dependencies in your `Cargo.toml` file:

```toml
[dependencies]
interflow = { git = "https://github.com/SolarLiner/interflow.git", version = "0.1.0" }
```

Then, in your main function, import `interflow::prelude::*` and use the `default_stream` function:

```rust
use interflow::prelude::*;

fn main() {
  let handle = default_stream(DeviceType::OUTPUT, |context, _input, output| {
    let time = context.timestamp.as_seconds(); // stream timestamp
    let time = output.timestamp.as_seconds(); // output stream provided timestamp (generally more accurate, also 
    // available for input)
    for i in 0..output.buffer.frames() {
      output.buffer.set_mono(0.0); // example: output silence
    }
  });
  std::thread::sleep(std::time::Duration::from_secs(10));
  let callback = handle.eject().unwrap(); // You can "eject" your callback and retrieve it, so you can reuse it in 
                                          // another stream. If you don't eject it, the callback will be dropped 
                                          // with the handle itself
}
```

It is important to import backends that you want to use, which is done automatically for default backends with `use 
interflow::prelude::*;`. Additional third-party backends that participate in automatic registration must be imported 
separately.

The mechanism used is link-time registration through the
[`scattered-collections`](https://docs.rs/scattered-collect/latest/scattered_collect/) crate. It is possible to use 
backends directly:

```rust
use interflow::prelude::*;

fn main() {
  let device = wasapi::platform::Platform.default_device(DeviceType::OUTPUT).unwrap();
  let handle = device.create_default_stream(...);
}
```

Take a look at the [examples](./examples) for an overview of the available API, as well 
as [the docs](https://solarliner.dev/interflow) for the generated reference documentation.

## Contributing

### Prerequisites

Ensure you have the following installed on your system:

- [Rust](https://www.rust-lang.org/tools/install) (1.85 and up supported)
- Platform-specific audio development libraries:
- **Windows**: Ensure you have the Windows SDK installed, and optionally the
  ASIO SDK if the `asio` feature is enabled.
- **macOS**: Xcode and its command line tools should be installed.
- **Linux**: Development libraries for ALSA (Advanced Linux Sound
  Architecture), PulseAudio, PipeWire, or JACK are only required if their
  relevant features are enabled (by default, only `alsa` is).

### Building

`Interflow` uses `cargo` for dependency management and building.