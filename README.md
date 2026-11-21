# FluxGen

FluxGen is a desktop image generation application for Windows. It runs
text-to-image and image-to-image generation locally through a bundled
stable-diffusion.cpp server process, with model files stored on local disk
and streamed to the GPU.

## Features

- Three model variants: Chroma1-HD, FLUX.1 [dev], and FLUX.2 Klein 4B,
  distributed as GGUF files.
- Model downloads managed by the application: resumable transfers,
  size checks, and SHA-256 verification before a file is accepted.
- Quantization level selected automatically from detected system
  specifications (GPU VRAM, system RAM, free disk), with the reasoning
  shown and a manual override in the Models page. A real engine load
  test verifies the selected quantization; a failed load steps down
  one quantization level.
- LoRA support: multiple LoRAs per generation, per-LoRA weight,
  per-LoRA enable/disable. User LoRAs are imported through a file
  picker into the application LoRA library, tagged by variant.
- Generation controls: prompt, negative prompt, seed, steps,
  guidance, sampler, width, height, batch queue, cancel.
- Saveable presets: variant, generation settings, and the full LoRA
  stack in one named bundle.
- Gallery: every output image carries its full generation settings
  embedded in the PNG file, and the same record is stored in a local
  SQLite gallery database for search and recall.
- Portable layout: the executable, engine, models, LoRAs, presets,
  outputs, and crash reports all live in one folder. No data is
  written to AppData or the registry.
- No content filtering in the application. Prompts pass to the engine
  exactly as entered.
- No telemetry. Network access is used only for model downloads and
  update checks.
- Updates: the application checks GitHub Releases for new versions
  and installs an update only after user approval. Update artifacts
  are signed and verified.
- Error handling: failures surface in the application with the full
  reason. A crash writes a text report to the `crashes/` folder and
  the next launch opens on a crash screen showing that report.

## Target hardware

- Windows 11, NVIDIA GPU with 12 GB VRAM, 16 GB system RAM.
- Model files require approximately 30 GB of disk space for all three
  variants; each variant can be downloaded separately.

## Repository structure

- `crates/fluxgen-core` — domain types, settings validation, portable
  path resolution, error types.
- `crates/fluxgen-models` — variant profiles, system specification
  detection, quantization selection, download manager.
- `crates/fluxgen-engine` — stable-diffusion.cpp server process
  supervision: launch, health check, output capture, exit detection,
  shutdown.
- `crates/fluxgen-store` — SQLite gallery, presets, and LoRA library;
  PNG settings metadata embed and read.
- `apps/fluxgen-app` — Tauri v2 application: Rust command layer and a
  vanilla HTML/CSS/JS front-end with no build step.

## Build

Requirements: Rust (stable), CMake, and the Tauri v2 prerequisites
for the target platform (on Windows: Microsoft C++ Build Tools and
WebView2).

```text
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

The Tauri application is a separate workspace under
`apps/fluxgen-app` and is built with the Tauri CLI:

```text
cd apps/fluxgen-app
cargo tauri build
```

## Versioning

Semantic Versioning. Each release is tagged `vX.Y.Z`, matching the
application version, and published as a GitHub Release of the same
name. Changes are recorded in `CHANGELOG.md`.
