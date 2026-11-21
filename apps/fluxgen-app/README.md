# FluxGen application

Tauri v2 application crate. This crate is a separate Cargo
workspace because it requires platform GUI libraries (WebView2 on
Windows, WebKitGTK on Linux).

## Layout

- `src/lib.rs` — Tauri command layer wiring the FluxGen library
  crates (core, models, engine, store) to the front-end.
- `src/main.rs` — binary entry point.
- `ui/` — vanilla HTML/CSS/JS front-end. No build step: the files
  are served as-is (`build.frontendDist` points at `ui/`).
- `tauri.conf.json` — application and bundle configuration.

## Updater state

The updater endpoint points at GitHub Releases. The `pubkey` field
in `tauri.conf.json` is intentionally empty and
`bundle.createUpdaterArtifacts` is `false` until an updater signing
key is generated and stored outside the repository; updater
artifacts must not be enabled before that key exists.

## Build

```text
cargo tauri build
```

Produces an NSIS installer for Windows under
`target/release/bundle/nsis/`.
