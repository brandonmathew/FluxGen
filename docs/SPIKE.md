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

## Real sd-server — NOT PROVEN

A real stable-diffusion.cpp build was not completed in this
stage:

- A shallow clone of the upstream repository was started, then
  stopped before any build step: the cloned content failed an
  execution-risk check, and building or running that content was
  not carried out. The partial clone was deleted.
- No real `sd-server` binary has been launched by the supervisor.
- The fallback fake-server integration test above was used
  instead, as specified for this case.
- sd-server argument names in `fluxgen-engine` (`--model`,
  `--vae`, `--clip_l`, `--t5xxl`, `--llm`, `--params-backend`)
  follow the upstream documentation but have not been exercised
  against a real binary. Flag verification against a real build
  remains open.

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
