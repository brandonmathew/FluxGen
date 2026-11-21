# Changelog

All notable changes to FluxGen are documented in this file.

The format is based on Keep a Changelog, and this project adheres to
Semantic Versioning.

## [Unreleased]

## [0.1.5] - 2026-11-21

### Added

- Segmented downloads: files of 32 MiB and larger download
  through up to 8 parallel range connections into one
  pre-sized partial file, with per-segment resume state
  (Issue #16).

### Changed

- The portable download is the bare `FluxGen.exe` again
  instead of a zip archive.

## [0.1.4] - 2026-11-20

### Added

- Signed in-app updates: the application checks GitHub
  Releases, and an approved update downloads, verifies its
  signature, installs in place, and restarts the application
  (Issue #3).
- The pinned engine is downloaded on first run from the
  engine release — resumable, and verified by size and
  SHA-256 — and the Models page shows the engine
  installation state with download and update actions
  (Issue #7).
- Image-to-image source image is chosen with a file picker
  instead of a typed path (Issue #13).

### Removed

- The engine bundle from the installer and portable
  downloads; both now contain the application only.

## [0.1.3] - 2026-11-20

### Fixed

- Engine start: the variant's diffusion model is passed to
  sd-server with `--diffusion-model`. With `--model` the real
  server (stable-diffusion.cpp master-1106-532d2c1) loads the
  files, then exits with `get sd version from file failed`, so
  no variant could start.
- Variant profiles: shared-file sources corrected for all three
  variants (several entries pointed at repositories that do not
  contain the files; the Klein text encoder's original
  repository is gated and was replaced with a byte-identical
  mirror), and every recorded file size is now the exact byte
  count from the repository listing instead of a rounded
  estimate the download verifier would reject.

### Added

- Engine integration tests against a real sd-server binary
  (environment-gated): missing-model error surfacing, model
  load, an HTTP generation request, and clean shutdown,
  verified with FLUX.2 Klein 4B on CPU.
- Bundled engine: the Windows package now includes the
  stable-diffusion.cpp master-1106-532d2c1 CUDA build
  (sd-server and its libraries, 21 files, ~1.3 GB) in an
  `engine` folder next to the application, in both the NSIS
  installer and the new portable zip. The Models page shows
  the bundled engine version. The portable download is now a
  zip containing the application and the engine folder; the
  previous bare executable download is replaced by it.
- Model downloads on the Models page: each variant card has
  a quantization selector (auto-pick preselected, override
  selectable), a Download button with live per-file and
  overall progress, and cancel support. Downloads resume
  from partial files, skip files already present at the
  exact size, and surface failures with their reason. A
  variant counts as downloaded only when its complete file
  set is present at the exact listed sizes.

## [0.1.2] - 2026-11-20

### Fixed

- Error banner visibility: an explicit `[hidden]` CSS rule now
  guarantees hidden elements stay hidden. The banner's
  `display: flex` rule previously overrode the `hidden` attribute
  in Chromium/WebView2, so the banner frame could render with no
  current error, including after dismissal.
- Error messages are self-identifying: the banner and the new
  Session errors list on the Settings page record which command
  produced each error, with the time. Empty error values are
  replaced with an explicit unknown-error message instead of
  rendering a blank banner.
- Settings page shows the application version in the System panel.

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
