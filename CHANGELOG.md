# Changelog

All notable changes to FluxGen are documented in this file.

The format is based on Keep a Changelog, and this project adheres to
Semantic Versioning.

## [Unreleased]

## [0.1.1] - 2026-11-20

### Fixed

- System specification detection on Windows: total RAM is read with
  GlobalMemoryStatusEx and GPU name and dedicated VRAM are read with
  DXGI adapter enumeration. Detection previously returned a
  not-implemented error on Windows, which also blocked the
  quantization recommendation.

## [0.1.0] - 2026-11-20

### Added

- Repository structure: Rust workspace with `fluxgen-core`,
  `fluxgen-models`, `fluxgen-engine`, and `fluxgen-store` crates.
- Continuous integration workflow covering formatting, clippy, and
  tests on Windows and Ubuntu.
- Domain types for variants, quantization levels, generation
  settings, LoRA entries, presets, and jobs, with validation and
  unit tests.
- Variant profiles for Chroma1-HD, FLUX.1 [dev], and FLUX.2 Klein 4B
  with GGUF file sets, sizes, and quantization tables.
- System specification detection and quantization auto-selection
  with unit tests.
- Resumable download manager with `.part` files and SHA-256
  verification.
- stable-diffusion.cpp server supervisor with health check, bounded
  output capture, exit detection, and clean shutdown.
- SQLite store for gallery records, presets, and the LoRA library,
  plus PNG settings metadata embed and read.
- Tauri v2 application shell with a no-build-step HTML/CSS/JS
  front-end: sidebar pages for Generate, Gallery, Models, LoRAs,
  Presets, and Settings; dark theme by default with a light theme
  toggle; error banner with copy action; crash screen on startup
  when a crash report exists.
- Portable data-root resolution next to the executable, with a
  `FLUXGEN_DATA_DIR` environment override for development.
- Engine spike results recorded in `docs/SPIKE.md`.
