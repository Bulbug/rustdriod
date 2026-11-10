# Android Emulator Manager (Rust)

A tiny Windows control panel for Google's official Android Emulator.
It downloads the SDK tools at runtime, manages system images and virtual devices,
and launches them with lean settings. Nothing from Google's SDK is bundled in this repo.

## Why it's light
- One small native `.exe` (no Java, no .NET, no browser engine)
- Uses almost no RAM and ~0% CPU while idle
- Presets include a **Lite** profile for low-end PCs

## Requirements
- Windows 10/11 with CPU virtualization enabled (BIOS) + Windows Hypervisor Platform
- To build: Rust (https://rustup.rs) and, on Windows, the "Build Tools for Visual Studio" (C++ workload)

## Build and run
```
cargo run --release
```
The exe ends up in `target/release/android-emulator-manager.exe`.

## Roadmap
- [x] Phase 1: environment check + SDK setup (also fetches a small Java runtime if needed)
- [x] Phase 2: system image manager (recommended list)
- [x] Phase 3: virtual device manager (presets, incl. Lite)
- [x] Phase 4: launcher (start/stop, live log, lean launch flags)
- [ ] Phase 5: ADB tools (install APK, screenshot, shell)
- [ ] Phase 6: keymapping, extra performance profiles
