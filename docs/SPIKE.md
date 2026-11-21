# Spike results — stage 1

Facts recorded from the stage 1 build. Items are marked PROVEN
(observed in a run) or NOT PROVEN (no successful run yet).

## Library crates — PROVEN on Linux

Commands run in the repository root:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Results:

- Format check: clean.
- Clippy with warnings denied: clean (exit code 0).
- Tests: 54 passed, 0 failed.
  - fluxgen-core: 20 (settings validation, quant ordering,
    portable paths, presets, LoRA entries, jobs, variants).
  - fluxgen-models: 19 (variant profiles, system parsing,
    quantization selection, download verification and resume).
  - fluxgen-engine: 5 unit + 2 integration (see below).
  - fluxgen-store: 8 (SQLite round trips, PNG metadata).

## Quantization auto-selection — PROVEN by unit test

On a synthetic machine with 12 GB VRAM, 16 GB RAM, and 152 GB
free disk at 1024×1024 output:

- Chroma1-HD picks Q6_K (7.65 GB). Q8_0 (9.74 GB) exceeds the
  budget of 12 − 1.5 (sampling reserve) − 1.0 (VAE allowance)
  = 9.5 GB.
- FLUX.1 [dev] picks Q5_K_S (8.3 GB). Q6_K (9.9 GB) exceeds the
  same budget.
- FLUX.2 Klein 4B picks its top published level, Q8_0.

## Download manager — PROVEN against a localhost server

A unit test serves a 50,000-byte payload from a localhost HTTP
responder with Range support, seeds a `.part` file with the first
10,000 bytes, and completes the download through the resume path.
The result matches the payload byte for byte, the SHA-256 check
passes, and the `.part` file is renamed to the destination.

No real model file has been downloaded yet.

## Engine supervisor — PROVEN against a fake server binary

The supervisor (`fluxgen-engine`) was tested against a fake
`sd-server`: a Python HTTP responder that accepts sd-server style
arguments, listens on the requested port, and answers JSON.

Integration test results:

- Launch, HTTP readiness wait, output capture (the fake server's
  banner line is present in the captured output), and clean
  shutdown: pass.
- Crash path: a stand-in binary that prints
  `fake sd-server fatal: model file is corrupt` and exits with
  code 3 produces `AppError::EngineExited` with code 3 and the
  printed message inside the error's captured output: pass.
- Missing binary path produces `AppError::EngineUnavailable`
  naming the path: pass.

The `engine_spike` example was also run against the fake server:

```text
cargo run -p fluxgen-engine --example engine_spike -- <fake-sd-server> 18099
SPIKE RESULT: server answered HTTP at http://127.0.0.1:18099
```

## Real sd-server — PROVEN on Linux (CPU), 2026-11-20

Verified against stable-diffusion.cpp **master-1106-532d2c1**
(built from the pinned release source on the build machine;
the binary reports that exact version string). Integration
tests live in `crates/fluxgen-engine/tests/real_server.rs` and
run when `FLUXGEN_SD_SERVER_BIN` and `FLUXGEN_KLEIN_DIR` point
at a real binary and the FLUX.2 Klein 4B test files.

Results (both tests pass, 151 s total on a 4-core CPU machine):

- Missing model: the supervisor surfaces
  `AppError::EngineExited` carrying the real server's captured
  output.
- Full path: the supervisor launches the real server with
  FluxGen's argument set, the server loads the Klein files
  (5,020 MB of parameters), readiness succeeds, an HTTP
  generation request (256×256, 1 step) returns an image, and
  shutdown is clean.

Findings from this verification:

- The diffusion model must be passed with `--diffusion-model`.
  With `--model` the real server loads the files, then exits
  with `get sd version from file failed` (Issue #12; fixed).
- The FLUX.2 VAE in Diffusers format
  (`vae/diffusion_pytorch_model.safetensors`, BF16) loads and
  is used by the server without conversion.
- Several variant profile entries pointed at repositories that
  do not contain the files, and all recorded sizes were rounded
  estimates; sources and exact byte sizes were corrected from
  the repository listings (Issue #11).

NOT PROVEN by this run: CUDA execution, the Chroma and FLUX.1
variants against the real server, and generation through the
application on the target Windows machine (Issue #2).

## Generation client — PROVEN on Linux (CPU), 2026-11-21

- The engine crate's generation client (the code path the
  application queue runs) generated a PNG from the same real
  server and Klein files: the `generation_client_generates_a_png`
  integration test passed in 192.77 s, and the returned bytes
  carry the PNG signature.
- The request shape (fields, the `lora` array of
  `{path, multiplier}` resolved inside the server's
  `--lora-model-dir`, `init_images` plus `denoising_strength`
  for image-to-image) was taken from the pinned server's own
  route source before the client was written.

NOT PROVEN by this run: generation with a LoRA applied, and
image-to-image through the client.

## Tauri application — NOT PROVEN on Windows

- `cargo check` for `apps/fluxgen-app` on the Linux build machine
  stops at the system library probe, before the crate's own code
  is compiled:

  ```text
  The system library `gdk-3.0` required by crate `gdk-sys` was not found.
  Package 'gdk-3.0', required by 'virtual:world', not found
  ```

  WebKitGTK/GDK development libraries are not installed on that
  machine. This is a missing-system-library failure, not a code
  error report; the crate's Rust code has not been compiler-checked
  anywhere yet.

- Checks that did run for the application crate: `cargo fmt`
  clean, JavaScript syntax check clean (`node --check`), and both
  JSON configuration files parse.
- The CI workflow compiles the application crate on
  windows-latest. A Windows build, an NSIS bundle, application
  launch, and a real generation are all NOT PROVEN by this stage.
