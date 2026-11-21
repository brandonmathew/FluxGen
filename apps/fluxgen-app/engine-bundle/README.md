# Engine bundle staging

The Windows release build stages the bundled stable-diffusion.cpp
engine binaries (sd-server and its libraries) into this folder
before packaging; `tauri.conf.json` maps the folder to the
`engine` folder next to the installed application.

The binaries are not committed to the repository. This README
keeps the folder present in a fresh checkout because the Tauri
build requires every configured resource path to exist. See the
"Stage the bundled engine" step in
`.github/workflows/build-windows.yml` for the pinned engine
release, the archive hashes, and the staged file list.
