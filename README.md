# Interflow

Interflow is a Rust library that abstracts away platform-specific audio APIs
and provides a unified, opinionated interface for audio applications. It aims
to simplify the development of audio applications by offering seamless support
for duplex audio with separate input and output devices, as well as sample rate
and format conversion.

## Features

- [x] Unified interface for platform-specific audio APIs.
- [ ] Support for duplex audio (simultaneous input and output).
- [ ] Separate input and output devices.
- [ ] Sample rate conversion.
- [ ] Format conversion.

## Supported drivers

- [ ] WASAPI
- [ ] ASIO
- [ ] ALSA
- [ ] PulseAudio
- [ ] PipeWire
- [ ] JACK
- [ ] CoreAudio

## Getting Started

### Prerequisites

Ensure you have the following installed on your system:

- [Rust](https://www.rust-lang.org/tools/install)
- Platform-specific audio development libraries:
- **Windows**: Ensure you have the Windows SDK installed, and optionally the
  ASIO SDK if the `asio` feature is enabled.
- **macOS**: Xcode and its command line tools should be installed.
- **Linux**: Development libraries for ALSA (Advanced Linux Sound
  Architecture), PulseAudio, PipeWire, or JACK are only required if their
  relevant features are enabled (by default, only `alsa` is).

### Building

`Interflow` uses `cargo` for dependency management and building.
