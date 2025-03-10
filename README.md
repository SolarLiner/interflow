# Interflow

[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](code_of_conduct.md)
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
- [ ] Support for duplex audio (simultaneous input and output).
- [ ] Separate input and output devices.
- [ ] Sample rate conversion.
- [ ] Format conversion.

## Supported drivers

- [x] WASAPI
- [ ] ASIO
- [x] ALSA
- [ ] PulseAudio
- [ ] PipeWire
- [ ] JACK
- [x] CoreAudio

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
