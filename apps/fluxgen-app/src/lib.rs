//! FluxGen Tauri command layer.
//!
//! Wires the library crates to the front-end:
//! - `fluxgen_core` for types and portable paths,
//! - `fluxgen_models` for variant profiles, system detection, and
//!   quantization selection,
//! - `fluxgen_engine` for the supervised sd-server process,
//! - `fluxgen_store` for the gallery, presets, and LoRA library.
//!
//! Every command returns `Result<T, String>` where the error string
//! is the full typed-error message, so the front-end error banner
//! shows the real reason. Nothing fails silently.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use fluxgen_core::{AppError, DataPaths, Quant, Variant};
use fluxgen_engine::{EngineConfig, EngineProcess};
use fluxgen_models::{detect_system, pick_quant, profile_for};
use fluxgen_store::Store;

/// Fixed loopback port for the engine server.
const ENGINE_PORT: u16 = 8189;

/// Application state shared by commands.
pub struct AppState {
    /// Portable data paths, resolved at startup.
    pub paths: DataPaths,
    /// SQLite store, opened at startup.
    pub store: Mutex<Store>,
    /// Running engine process, when started.
    pub engine: tokio::sync::Mutex<Option<EngineProcess>>,
}

fn err_string(e: AppError) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// System information shown on the Settings and Models pages.
#[derive(Serialize)]
pub struct SystemInfoDto {
    /// GPU display name, when detected.
    pub gpu_name: Option<String>,
    /// GPU VRAM in bytes, when detected.
    pub vram_bytes: Option<u64>,
    /// Total system RAM in bytes.
    pub ram_bytes: u64,
    /// Free disk bytes on the data volume.
    pub free_disk_bytes: u64,
    /// Portable data root path.
    pub data_root: String,
}

/// One quantization row for a variant.
#[derive(Serialize)]
pub struct QuantDto {
    /// Publisher label.
    pub label: String,
    /// Model file size in bytes.
    pub size_bytes: u64,
    /// Total download size in bytes at this quantization.
    pub total_size_bytes: u64,
}

/// One variant row for the Models page and Generate page.
#[derive(Serialize)]
pub struct VariantDto {
    /// Stable variant id.
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Whether the variant model directory exists and is non-empty.
    pub installed: bool,
    /// Quantization options, smallest first.
    pub quants: Vec<QuantDto>,
}

/// Quantization recommendation for the Models page.
#[derive(Serialize)]
pub struct QuantChoiceDto {
    /// Publisher label of the pick.
    pub label: String,
    /// Model size in bytes.
    pub model_size_bytes: u64,
    /// Factual reasoning lines.
    pub reasons: Vec<String>,
}

/// Engine state for the Generate page.
#[derive(Serialize)]
pub struct EngineStatusDto {
    /// Whether an engine process is held by the app.
    pub running: bool,
    /// Whether the held process is still alive.
    pub alive: bool,
    /// Port the engine listens on.
    pub port: u16,
}

/// One gallery row for the Gallery page.
#[derive(Serialize)]
pub struct GalleryDto {
    /// Database row id.
    pub id: Option<i64>,
    /// Output image path.
    pub path: String,
    /// Unix timestamp of creation.
    pub created_at: i64,
    /// Variant display name.
    pub variant: String,
    /// Quantization label.
    pub quant: String,
    /// Settings JSON document.
    pub settings_json: String,
}

/// Bundled engine identity and presence.
#[derive(Serialize)]
pub struct EngineInfoDto {
    /// Bundled engine release string.
    pub version: String,
    /// Whether the engine binary is present in the engine folder.
    pub bundled: bool,
    /// Full path the engine binary is expected at.
    pub binary_path: String,
}

/// The newest crash report, when one exists.
#[derive(Serialize)]
pub struct CrashReportDto {
    /// Report file name inside `crashes/`.
    pub file_name: String,
    /// Full report text.
    pub contents: String,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Detected system specifications and the data root.
#[tauri::command]
fn get_system_info(state: State<'_, AppState>) -> Result<SystemInfoDto, String> {
    let specs = detect_system(&state.paths.root).map_err(err_string)?;
    Ok(SystemInfoDto {
        gpu_name: specs.gpu_name,
        vram_bytes: specs.vram_bytes,
        ram_bytes: specs.ram_bytes,
        free_disk_bytes: specs.free_disk_bytes,
        data_root: state.paths.root.to_string_lossy().into_owned(),
    })
}

/// Bundled engine version and whether the binary is in place.
#[tauri::command]
fn get_engine_info(state: State<'_, AppState>) -> Result<EngineInfoDto, String> {
    let binary_name = if cfg!(windows) {
        "sd-server.exe"
    } else {
        "sd-server"
    };
    let binary_path = state.paths.engine.join(binary_name);
    Ok(EngineInfoDto {
        version: fluxgen_engine::ENGINE_VERSION.to_string(),
        bundled: binary_path.is_file(),
        binary_path: binary_path.to_string_lossy().into_owned(),
    })
}

/// All variants with install state and quantization tables.
#[tauri::command]
fn list_variants(state: State<'_, AppState>) -> Result<Vec<VariantDto>, String> {
    let mut out = Vec::new();
    for variant in Variant::ALL {
        let profile = profile_for(variant);
        let dir = state.paths.variant_models_dir(variant);
        let installed = std::fs::read_dir(&dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        let quants = profile
            .quant_options
            .iter()
            .map(|o| QuantDto {
                label: o.label.to_string(),
                size_bytes: o.size_bytes,
                total_size_bytes: profile.total_size_bytes(o.quant).unwrap_or(o.size_bytes),
            })
            .collect();
        out.push(VariantDto {
            id: variant.id().to_string(),
            display_name: variant.display_name().to_string(),
            installed,
            quants,
        });
    }
    Ok(out)
}

/// Auto-picked quantization for a variant at an output size, with
/// the reasoning.
#[tauri::command]
fn get_quant_recommendation(
    state: State<'_, AppState>,
    variant_id: String,
    width: u32,
    height: u32,
) -> Result<QuantChoiceDto, String> {
    let variant =
        Variant::from_id(&variant_id).ok_or_else(|| format!("unknown variant id: {variant_id}"))?;
    let specs = detect_system(&state.paths.root).map_err(err_string)?;
    let choice = pick_quant(variant, &specs, width, height).map_err(err_string)?;
    Ok(QuantChoiceDto {
        label: choice.label,
        model_size_bytes: choice.model_size_bytes,
        reasons: choice.reasons,
    })
}

/// Resolve the engine launch configuration for a variant at a
/// quantization from the portable data layout. Missing files are
/// reported as typed errors naming the missing file.
fn engine_config_for(
    paths: &DataPaths,
    variant: Variant,
    quant: Quant,
) -> Result<EngineConfig, AppError> {
    let profile = profile_for(variant);
    let option = profile.quant_option(quant).ok_or_else(|| {
        AppError::validation(
            "quant",
            format!(
                "{} is not published for {}",
                quant.label(),
                variant.display_name()
            ),
        )
    })?;
    let dir = paths.variant_models_dir(variant);
    let model_path = dir.join(option.file_name);
    if !model_path.is_file() {
        return Err(AppError::FileNotFound(model_path));
    }
    let mut encoders = Vec::new();
    let mut vae_path = None;
    for file in profile.shared_files {
        let path = dir.join(file.file_name);
        match file.role {
            fluxgen_models::profiles::FileRole::TextEncoder => {
                if !path.is_file() {
                    return Err(AppError::FileNotFound(path));
                }
                encoders.push(path);
            }
            fluxgen_models::profiles::FileRole::Vae => {
                if !path.is_file() {
                    return Err(AppError::FileNotFound(path));
                }
                vae_path = Some(path);
            }
            _ => {}
        }
    }
    let vae_path = vae_path.ok_or_else(|| {
        AppError::Storage(format!(
            "profile for {} has no VAE file",
            variant.display_name()
        ))
    })?;
    let binary_name = if cfg!(windows) {
        "sd-server.exe"
    } else {
        "sd-server"
    };
    let binary_path = paths.engine.join(binary_name);
    Ok(EngineConfig {
        binary_path,
        port: ENGINE_PORT,
        model_path,
        text_encoder_paths: encoders,
        vae_path,
        startup_timeout_secs: 300,
    })
}

/// Start the persistent engine for a variant at a quantization.
/// Fails with the full reason when the engine binary or any model
/// file is missing, or when the engine exits during startup.
#[tauri::command]
async fn engine_start(
    state: State<'_, AppState>,
    variant_id: String,
    quant_label: String,
) -> Result<EngineStatusDto, String> {
    let variant =
        Variant::from_id(&variant_id).ok_or_else(|| format!("unknown variant id: {variant_id}"))?;
    let quant = [
        Quant::Q2,
        Quant::Q3,
        Quant::Q4,
        Quant::Q5,
        Quant::Q6,
        Quant::Q8,
        Quant::Bf16,
    ]
    .into_iter()
    .find(|q| profile_for(variant).quant_option(*q).map(|o| o.label) == Some(quant_label.as_str()))
    .ok_or_else(|| {
        format!(
            "unknown quant label for {}: {quant_label}",
            variant.display_name()
        )
    })?;

    let mut guard = state.engine.lock().await;
    if let Some(existing) = guard.as_mut() {
        if existing.try_exit_status().map_err(err_string)?.is_none() {
            return Err(
                "engine is already running; stop it before starting another variant".to_string(),
            );
        }
        *guard = None;
    }

    let config = engine_config_for(&state.paths, variant, quant).map_err(err_string)?;
    let mut process = EngineProcess::launch(config).map_err(err_string)?;
    process.wait_until_ready().await.map_err(err_string)?;
    *guard = Some(process);
    Ok(EngineStatusDto {
        running: true,
        alive: true,
        port: ENGINE_PORT,
    })
}

/// Stop the running engine, if any.
#[tauri::command]
async fn engine_stop(state: State<'_, AppState>) -> Result<EngineStatusDto, String> {
    let mut guard = state.engine.lock().await;
    if let Some(mut process) = guard.take() {
        process.shutdown().await.map_err(err_string)?;
    }
    Ok(EngineStatusDto {
        running: false,
        alive: false,
        port: ENGINE_PORT,
    })
}

/// Current engine state. A held process that has exited is
/// reported as not alive — the exit is surfaced, not hidden.
#[tauri::command]
async fn engine_status(state: State<'_, AppState>) -> Result<EngineStatusDto, String> {
    let mut guard = state.engine.lock().await;
    match guard.as_mut() {
        Some(process) => {
            let exited = process.try_exit_status().map_err(err_string)?.is_some();
            Ok(EngineStatusDto {
                running: true,
                alive: !exited,
                port: ENGINE_PORT,
            })
        }
        None => Ok(EngineStatusDto {
            running: false,
            alive: false,
            port: ENGINE_PORT,
        }),
    }
}

/// Gallery records, newest first.
#[tauri::command]
fn list_gallery(state: State<'_, AppState>) -> Result<Vec<GalleryDto>, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let records = store.list_gallery().map_err(err_string)?;
    Ok(records
        .into_iter()
        .map(|r| GalleryDto {
            id: r.id,
            path: r.path.to_string_lossy().into_owned(),
            created_at: r.created_at,
            variant: r.variant.display_name().to_string(),
            quant: r.quant.label().to_string(),
            settings_json: r.settings_json,
        })
        .collect())
}

/// Saved presets.
#[tauri::command]
fn list_presets(state: State<'_, AppState>) -> Result<Vec<fluxgen_core::Preset>, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    store.list_presets().map_err(err_string)
}

/// LoRA library entries for a variant.
#[tauri::command]
fn list_loras(
    state: State<'_, AppState>,
    variant_id: String,
) -> Result<Vec<fluxgen_core::LoraEntry>, String> {
    let variant =
        Variant::from_id(&variant_id).ok_or_else(|| format!("unknown variant id: {variant_id}"))?;
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    store.list_loras(variant).map_err(err_string)
}

/// The newest crash report in the portable `crashes/` folder, when
/// one exists. Checked by the front-end at startup to decide
/// whether to show the crash screen.
#[tauri::command]
fn check_crash_report(state: State<'_, AppState>) -> Result<Option<CrashReportDto>, String> {
    let dir = &state.paths.crashes;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "txt").unwrap_or(false) {
                if let Ok(meta) = entry.metadata() {
                    let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                        newest = Some((modified, path));
                    }
                }
            }
        }
    }
    match newest {
        Some((_, path)) => {
            let contents = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            Ok(Some(CrashReportDto {
                file_name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                contents,
            }))
        }
        None => Ok(None),
    }
}

/// Delete a crash report by file name (name only; the file must be
/// inside the crashes folder).
#[tauri::command]
fn delete_crash_report(state: State<'_, AppState>, file_name: String) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("invalid crash report file name".to_string());
    }
    let path = state.paths.crashes.join(&file_name);
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

/// Install a panic hook that writes a crash report into the
/// portable `crashes/` folder before the default hook runs. Keeps
/// at most 10 reports.
fn install_crash_hook(paths: &DataPaths) {
    let crashes_dir = paths.crashes.clone();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let report = format!(
            "FluxGen crash report\nunix_time: {timestamp}\n\n{info}\n\nbacktrace note: set RUST_BACKTRACE=1 for a full backtrace\n"
        );
        let _ = std::fs::create_dir_all(&crashes_dir);
        let path = crashes_dir.join(format!("crash-{timestamp}.txt"));
        let _ = std::fs::write(&path, &report);
        prune_crash_reports(&crashes_dir, 10);
        default_hook(info);
    }));
}

/// Delete older crash reports beyond the newest `keep`.
fn prune_crash_reports(dir: &std::path::Path, keep: usize) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().map(|x| x == "txt").unwrap_or(false))
            .filter_map(|e| {
                e.metadata().ok().map(|m| {
                    (
                        m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                        e.path(),
                    )
                })
            })
            .collect();
        files.sort_by_key(|(t, _)| *t);
        while files.len() > keep {
            let (_, path) = files.remove(0);
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Build and run the Tauri application.
pub fn run() {
    let paths = DataPaths::resolve().unwrap_or_else(|_| {
        DataPaths::from_root(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    });
    if let Err(e) = paths.ensure_created() {
        eprintln!(
            "failed to create data directories under {}: {e}",
            paths.root.display()
        );
    }
    install_crash_hook(&paths);

    let store = match Store::open(&paths.database) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("failed to open database {}: {e}", paths.database.display());
            std::process::exit(1);
        }
    };

    let state = AppState {
        paths,
        store: Mutex::new(store),
        engine: tokio::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_system_info,
            get_engine_info,
            list_variants,
            get_quant_recommendation,
            engine_start,
            engine_stop,
            engine_status,
            list_gallery,
            list_presets,
            list_loras,
            check_crash_report,
            delete_crash_report
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("application failed to start: {e}");
            std::process::exit(1);
        });
}
