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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use fluxgen_core::{AppError, DataPaths, Quant, Variant};
use fluxgen_engine::{EngineConfig, EngineProcess};
use fluxgen_models::{detect_system, download_file, pick_quant, profile_for, DownloadTask};
use fluxgen_store::Store;

/// Fixed loopback port for the engine server.
const ENGINE_PORT: u16 = 8189;

/// One in-flight variant download.
pub struct DownloadSlot {
    /// Set by the cancel command; the runner checks it before
    /// reporting completion so a cancel is reported exactly once.
    cancelled: Arc<AtomicBool>,
    /// Handle of the spawned download task.
    handle: tokio::task::JoinHandle<()>,
}

/// Registry of in-flight downloads, keyed by variant id.
pub type DownloadRegistry = tokio::sync::Mutex<HashMap<String, DownloadSlot>>;

/// Application state shared by commands.
pub struct AppState {
    /// Portable data paths, resolved at startup.
    pub paths: DataPaths,
    /// SQLite store, opened at startup.
    pub store: Arc<Mutex<Store>>,
    /// Running engine process, when started.
    pub engine: Arc<tokio::sync::Mutex<Option<EngineProcess>>>,
    /// In-flight variant downloads.
    pub downloads: Arc<DownloadRegistry>,
    /// An application update that has been found and is ready
    /// to install.
    pub pending_update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    /// Generation queue, in enqueue order. Jobs are session
    /// state; finished outputs persist in the gallery.
    pub jobs: Arc<tokio::sync::Mutex<Vec<fluxgen_core::Job>>>,
    /// Next generation job id.
    pub next_job_id: Arc<std::sync::atomic::AtomicU64>,
    /// Wakes the generation runner when a job is enqueued.
    pub queue_notify: Arc<tokio::sync::Notify>,
    /// Variant and quantization the engine is loaded with,
    /// when the app started it.
    pub loaded_engine: Arc<tokio::sync::Mutex<Option<(Variant, Quant)>>>,
    /// Handle of the currently executing job, for cancel.
    pub running_job: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
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

/// Pinned engine identity and installation state.
#[derive(Serialize)]
pub struct EngineInfoDto {
    /// Pinned engine release string.
    pub version: String,
    /// Installation state: `current`, `outdated`, or `missing`.
    pub state: String,
    /// Full path the engine binary is expected at.
    pub binary_path: String,
}

/// Information about an available application update.
#[derive(Serialize, Clone)]
pub struct UpdateInfoDto {
    /// Version of the available update.
    pub version: String,
    /// Release notes, when the release provides them.
    pub notes: Option<String>,
}

/// Update download progress, emitted as an `update-progress`
/// event.
#[derive(Serialize, Clone)]
pub struct UpdateProgressDto {
    /// Bytes downloaded so far.
    pub downloaded_bytes: u64,
}

/// Progress of one variant download, emitted as a
/// `download-progress` event.
#[derive(Serialize, Clone)]
pub struct DownloadProgressDto {
    /// Variant being downloaded.
    pub variant_id: String,
    /// File currently transferring (leaf file name).
    pub file_name: String,
    /// 1-based index of the current file.
    pub file_index: usize,
    /// Total number of files in this download.
    pub files_total: usize,
    /// Bytes transferred for the current file.
    pub file_downloaded_bytes: u64,
    /// Expected size of the current file.
    pub file_total_bytes: u64,
    /// Bytes transferred across all files so far.
    pub overall_downloaded_bytes: u64,
    /// Expected size of the whole download.
    pub overall_total_bytes: u64,
}

/// Outcome of a variant download, emitted as a
/// `download-finished` event.
#[derive(Serialize, Clone)]
pub struct DownloadFinishedDto {
    /// Variant that was downloaded.
    pub variant_id: String,
    /// Whether every file downloaded and verified.
    pub ok: bool,
    /// The failure or cancellation reason, when not `ok`.
    pub error: Option<String>,
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

/// Path of the engine binary inside the engine folder.
fn engine_binary_path(paths: &DataPaths) -> PathBuf {
    let binary_name = if cfg!(windows) {
        "sd-server.exe"
    } else {
        "sd-server"
    };
    paths.engine.join(binary_name)
}

/// Installation state of the pinned engine: `current`,
/// `outdated` (a different release's marker is present), or
/// `missing` (no binary). An engine folder with a binary but
/// no marker predates markers (the v0.1.3 bundled build) and
/// is adopted as current.
fn engine_state(paths: &DataPaths) -> &'static str {
    if !engine_binary_path(paths).is_file() {
        return "missing";
    }
    let marker = paths.engine.join("engine-version.txt");
    match std::fs::read_to_string(&marker) {
        Ok(text) if text.trim() == fluxgen_engine::ENGINE_RELEASE_ID => "current",
        Ok(_) => "outdated",
        Err(_) => {
            let _ = std::fs::write(&marker, fluxgen_engine::ENGINE_RELEASE_ID);
            "current"
        }
    }
}

/// Pinned engine version and its installation state.
#[tauri::command]
fn get_engine_info(state: State<'_, AppState>) -> Result<EngineInfoDto, String> {
    Ok(EngineInfoDto {
        version: fluxgen_engine::ENGINE_VERSION.to_string(),
        state: engine_state(&state.paths).to_string(),
        binary_path: engine_binary_path(&state.paths)
            .to_string_lossy()
            .into_owned(),
    })
}

/// All variants with install state and quantization tables.
#[tauri::command]
fn list_variants(state: State<'_, AppState>) -> Result<Vec<VariantDto>, String> {
    let mut out = Vec::new();
    for variant in Variant::ALL {
        let profile = profile_for(variant);
        let dir = state.paths.variant_models_dir(variant);
        let installed = !profile.installed_quants(&dir).is_empty();
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

/// Run one variant download to completion, emitting progress
/// events as bytes land. Files already on disk at the exact
/// listed size are skipped by the caller.
async fn run_variant_download(
    app: AppHandle,
    variant_id: String,
    tasks: Vec<DownloadTask>,
) -> Result<(), String> {
    let files_total = tasks.len();
    let overall_total_bytes: u64 = tasks.iter().map(|t| t.expected_size_bytes).sum();
    let mut completed_bytes: u64 = 0;
    for (index, task) in tasks.iter().enumerate() {
        if let Some(parent) = task.destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file_name = task
            .destination
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| task.url.clone());
        let file_total_bytes = task.expected_size_bytes;
        let base_bytes = completed_bytes;
        let progress_app = app.clone();
        let progress_variant = variant_id.clone();
        let mut last_percent: u64 = u64::MAX;
        download_file(task, move |written, _total| {
            let overall = base_bytes + written;
            let percent = overall
                .checked_mul(100)
                .and_then(|scaled| scaled.checked_div(overall_total_bytes))
                .unwrap_or(100);
            if percent == last_percent {
                return;
            }
            last_percent = percent;
            let _ = progress_app.emit(
                "download-progress",
                DownloadProgressDto {
                    variant_id: progress_variant.clone(),
                    file_name: file_name.clone(),
                    file_index: index + 1,
                    files_total,
                    file_downloaded_bytes: written,
                    file_total_bytes,
                    overall_downloaded_bytes: overall,
                    overall_total_bytes,
                },
            );
        })
        .await
        .map_err(err_string)?;
        completed_bytes += file_total_bytes;
    }
    Ok(())
}

/// Start downloading a variant's files in the background.
/// `quant_label` selects the quantization by its publisher
/// label; when omitted, the quantization is auto-picked from
/// the detected system specifications. Progress arrives as
/// `download-progress` events and the outcome as a single
/// `download-finished` event.
#[tauri::command]
async fn start_variant_download(
    app: AppHandle,
    state: State<'_, AppState>,
    variant_id: String,
    quant_label: Option<String>,
) -> Result<(), String> {
    let variant =
        Variant::from_id(&variant_id).ok_or_else(|| format!("unknown variant id: {variant_id}"))?;
    let profile = profile_for(variant);
    let quant = match &quant_label {
        Some(label) => profile
            .quant_options
            .iter()
            .find(|o| o.label == label.as_str())
            .map(|o| o.quant)
            .ok_or_else(|| {
                format!(
                    "variant {} has no quantization labelled {label}",
                    variant.display_name()
                )
            })?,
        None => {
            let specs = detect_system(&state.paths.root).map_err(err_string)?;
            pick_quant(variant, &specs, 1024, 1024)
                .map_err(err_string)?
                .quant
        }
    };
    let dir = state.paths.variant_models_dir(variant);
    let tasks = profile
        .download_tasks(quant, &dir)
        .ok_or_else(|| "quantization is not published for this variant".to_string())?;
    let pending: Vec<DownloadTask> = tasks
        .into_iter()
        .filter(|t| fluxgen_models::download::verify_file(t, &t.destination).is_err())
        .collect();

    let job_variant = variant_id.clone();
    begin_download(&app, &state, variant_id, move |runner_app| async move {
        run_variant_download(runner_app, job_variant, pending).await
    })
    .await
}

/// Spawn a background download under `key`, registering it for
/// cancellation. The job emits its own progress events; this
/// wrapper emits exactly one `download-finished` event when
/// the job ends, unless the download was cancelled (the cancel
/// command emits it instead).
async fn begin_download<F, Fut>(
    app: &AppHandle,
    state: &AppState,
    key: String,
    make_job: F,
) -> Result<(), String>
where
    F: FnOnce(AppHandle) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    let registry = state.downloads.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let runner_cancelled = cancelled.clone();
    let runner_app = app.clone();
    let runner_key = key.clone();
    let handle = tokio::spawn(async move {
        let result = make_job(runner_app.clone()).await;
        // Unregister, but only our own slot: a cancel removes
        // the slot itself, and a newer download under the same
        // key must not be disturbed.
        let mut map = registry.lock().await;
        let ours = map
            .get(&runner_key)
            .map(|slot| Arc::ptr_eq(&slot.cancelled, &runner_cancelled))
            .unwrap_or(false);
        if ours {
            map.remove(&runner_key);
        }
        drop(map);
        if !runner_cancelled.load(Ordering::SeqCst) {
            let _ = runner_app.emit(
                "download-finished",
                DownloadFinishedDto {
                    variant_id: runner_key,
                    ok: result.is_ok(),
                    error: result.err(),
                },
            );
        }
    });

    let mut map = state.downloads.lock().await;
    if map.contains_key(&key) {
        handle.abort();
        return Err(format!("a download for {key} is already running"));
    }
    map.insert(key, DownloadSlot { cancelled, handle });
    Ok(())
}

/// Cancel the download registered under `key`, reporting the
/// cancellation through the same `download-finished` event a
/// completion would use.
async fn cancel_download(
    app: &AppHandle,
    state: &AppState,
    key: &str,
    message: &str,
) -> Result<(), String> {
    let slot = state.downloads.lock().await.remove(key);
    match slot {
        Some(slot) => {
            slot.cancelled.store(true, Ordering::SeqCst);
            slot.handle.abort();
            let _ = app.emit(
                "download-finished",
                DownloadFinishedDto {
                    variant_id: key.to_string(),
                    ok: false,
                    error: Some(message.to_string()),
                },
            );
            Ok(())
        }
        None => Err("no download is running for this item".to_string()),
    }
}

/// Cancel a running variant download. Partial files stay on
/// disk and resume on the next download.
#[tauri::command]
async fn cancel_variant_download(
    app: AppHandle,
    state: State<'_, AppState>,
    variant_id: String,
) -> Result<(), String> {
    cancel_download(
        &app,
        &state,
        &variant_id,
        "Download cancelled. Partial files are kept and resume on the next download.",
    )
    .await
}

/// Download and install the pinned engine into the engine
/// folder: the bundle archive is fetched with the same
/// resumable, verified downloader as the models, extracted,
/// and marked with the pinned release id.
async fn run_engine_download(
    app: AppHandle,
    zip_path: PathBuf,
    engine_dir: PathBuf,
) -> Result<(), String> {
    let task = DownloadTask {
        url: fluxgen_engine::ENGINE_DOWNLOAD_URL.to_string(),
        destination: zip_path.clone(),
        expected_size_bytes: fluxgen_engine::ENGINE_DOWNLOAD_SIZE_BYTES,
        expected_sha256: Some(fluxgen_engine::ENGINE_DOWNLOAD_SHA256.to_string()),
    };
    if fluxgen_models::download::verify_file(&task, &zip_path).is_err() {
        let progress_app = app.clone();
        let mut last_percent: u64 = u64::MAX;
        download_file(&task, move |written, total| {
            let percent = written
                .checked_mul(100)
                .and_then(|scaled| scaled.checked_div(total))
                .unwrap_or(100);
            if percent == last_percent {
                return;
            }
            last_percent = percent;
            let _ = progress_app.emit(
                "download-progress",
                DownloadProgressDto {
                    variant_id: "engine".to_string(),
                    file_name: "engine bundle archive".to_string(),
                    file_index: 1,
                    files_total: 1,
                    file_downloaded_bytes: written,
                    file_total_bytes: total,
                    overall_downloaded_bytes: written,
                    overall_total_bytes: total,
                },
            );
        })
        .await
        .map_err(err_string)?;
    }
    let extract_zip = zip_path.clone();
    let extract_dir = engine_dir.clone();
    tokio::task::spawn_blocking(move || extract_engine_archive(&extract_zip, &extract_dir))
        .await
        .map_err(|e| format!("engine extraction task failed: {e}"))??;
    std::fs::write(
        engine_dir.join("engine-version.txt"),
        fluxgen_engine::ENGINE_RELEASE_ID,
    )
    .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}

/// Extract the engine bundle archive into the engine folder.
/// Entries with paths that would escape the folder are
/// rejected.
fn extract_engine_archive(zip_path: &PathBuf, engine_dir: &PathBuf) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("engine archive is not a valid zip file: {e}"))?;
    std::fs::create_dir_all(engine_dir).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry
            .enclosed_name()
            .ok_or_else(|| format!("engine archive entry {} has an unsafe path", entry.name()))?;
        let out = engine_dir.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut writer = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut writer).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Start downloading the pinned engine in the background.
/// Progress and outcome arrive through the same download
/// events as model downloads, under the key `engine`.
#[tauri::command]
async fn start_engine_download(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let zip_path = state.paths.root.join("engine-bundle.zip");
    let engine_dir = state.paths.engine.clone();
    begin_download(&app, &state, "engine".to_string(), move |runner_app| async move {
        run_engine_download(runner_app, zip_path, engine_dir).await
    })
    .await
}

/// Cancel a running engine download. The partial archive
/// stays on disk and resumes on the next download.
#[tauri::command]
async fn cancel_engine_download(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    cancel_download(
        &app,
        &state,
        "engine",
        "Engine download cancelled. The partial archive is kept and resumes on the next download.",
    )
    .await
}

/// Check GitHub Releases for an application update. A found
/// update is held for `install_update`; nothing is downloaded
/// or installed by the check itself.
#[tauri::command]
async fn check_for_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<UpdateInfoDto>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let info = found.as_ref().map(|update| UpdateInfoDto {
        version: update.version.clone(),
        notes: update.body.clone(),
    });
    *state.pending_update.lock().await = found;
    Ok(info)
}

/// Download and install the update found by
/// `check_for_update`. The signature is verified by the
/// updater before anything installs. On success the installer
/// launches and the application exits so its files can be
/// replaced.
#[tauri::command]
async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let update = state
        .pending_update
        .lock()
        .await
        .take()
        .ok_or_else(|| "no update is ready to install; check for updates first".to_string())?;
    let progress_app = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk_length, _content_length| {
                downloaded += chunk_length as u64;
                let _ = progress_app.emit(
                    "update-progress",
                    UpdateProgressDto {
                        downloaded_bytes: downloaded,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
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
    // The server resolves per-request LoRAs from the variant's
    // LoRA directory. A variant's bundled LoRA downloads with
    // the model files, so link it into the LoRA directory where
    // the server can address it by file name.
    let loras_dir = paths.variant_loras_dir(variant);
    std::fs::create_dir_all(&loras_dir)?;
    if let Some(file) = profile
        .shared_files
        .iter()
        .find(|f| f.role == fluxgen_models::profiles::FileRole::BundledLora)
    {
        let src = dir.join(file.file_name);
        let dst = loras_dir.join(file.file_name);
        if src.is_file() && !dst.exists() {
            if std::fs::hard_link(&src, &dst).is_err() {
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }
    let binary_path = engine_binary_path(paths);
    Ok(EngineConfig {
        binary_path,
        port: ENGINE_PORT,
        model_path,
        text_encoder_paths: encoders,
        vae_path,
        lora_model_dir: Some(loras_dir),
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
    *state.loaded_engine.lock().await = Some((variant, quant));
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
    *state.loaded_engine.lock().await = None;
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

/// Generation request as submitted from the Generate page.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDto {
    /// Variant id.
    pub variant_id: String,
    /// Positive prompt.
    pub prompt: String,
    /// Negative prompt.
    pub negative_prompt: String,
    /// Seed; `None` chooses a random seed at run time.
    pub seed: Option<u64>,
    /// Sampling steps.
    pub steps: u32,
    /// Guidance scale.
    pub guidance: f32,
    /// Sampler name in the engine's spelling.
    pub sampler: String,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Source image path for image-to-image; empty or absent
    /// runs text-to-image.
    pub init_image: Option<String>,
    /// Denoise strength for image-to-image.
    pub denoise_strength: Option<f32>,
}

/// The settings document embedded into every output PNG and
/// stored in the gallery database: the generation settings
/// flattened alongside the variant, quantization, LoRA stack,
/// and source image that produced the image.
#[derive(Serialize)]
pub struct GenerationRecord {
    /// Variant id.
    pub variant: String,
    /// Quantization label.
    pub quant: String,
    /// The generation settings, flattened to the top level.
    #[serde(flatten)]
    pub settings: fluxgen_core::GenerationSettings,
    /// LoRA stack applied, in order.
    pub loras: Vec<fluxgen_core::LoraEntry>,
    /// Source image path for image-to-image outputs.
    pub init_image: Option<String>,
}

/// Shared handles the generation runner works through.
#[derive(Clone)]
struct JobContext {
    paths: DataPaths,
    store: Arc<Mutex<Store>>,
    engine: Arc<tokio::sync::Mutex<Option<EngineProcess>>>,
    jobs: Arc<tokio::sync::Mutex<Vec<fluxgen_core::Job>>>,
    queue_notify: Arc<tokio::sync::Notify>,
    loaded_engine: Arc<tokio::sync::Mutex<Option<(Variant, Quant)>>>,
    running_job: Arc<tokio::sync::Mutex<Option<tokio::task::AbortHandle>>>,
}

impl JobContext {
    fn from_state(state: &AppState) -> JobContext {
        JobContext {
            paths: state.paths.clone(),
            store: state.store.clone(),
            engine: state.engine.clone(),
            jobs: state.jobs.clone(),
            queue_notify: state.queue_notify.clone(),
            loaded_engine: state.loaded_engine.clone(),
            running_job: state.running_job.clone(),
        }
    }
}

/// The quantization a job runs at: the recommended pick when
/// that quantization is downloaded, otherwise the largest
/// downloaded one.
fn quant_for_job(
    state: &AppState,
    variant: Variant,
    width: u32,
    height: u32,
) -> Result<Quant, String> {
    let profile = profile_for(variant);
    let installed = profile.installed_quants(&state.paths.variant_models_dir(variant));
    if installed.is_empty() {
        return Err(format!(
            "{} is not downloaded; download it on the Models page first",
            variant.display_name()
        ));
    }
    if let Ok(specs) = detect_system(&state.paths.root) {
        if let Ok(choice) = pick_quant(variant, &specs, width, height) {
            if installed.contains(&choice.quant) {
                return Ok(choice.quant);
            }
        }
    }
    Ok(*installed
        .iter()
        .max()
        .expect("installed quants checked non-empty"))
}

/// The variant's bundled LoRA as a job entry, when the
/// variant ships one and it is downloaded. Its path points
/// into the variant's LoRA directory, where the engine
/// start links it for the server.
fn bundled_lora_entry(paths: &DataPaths, variant: Variant) -> Option<fluxgen_core::LoraEntry> {
    let profile = profile_for(variant);
    let file = profile
        .shared_files
        .iter()
        .find(|f| f.role == fluxgen_models::profiles::FileRole::BundledLora)?;
    let downloaded = paths.variant_models_dir(variant).join(file.file_name);
    if !downloaded.is_file() {
        return None;
    }
    Some(fluxgen_core::LoraEntry {
        name: format!("{} (bundled)", file.file_name),
        path: paths.variant_loras_dir(variant).join(file.file_name),
        variant,
        weight: 1.0,
        enabled: true,
    })
}

/// Queue a generation from the Generate page. The job carries
/// the variant's enabled LoRA stack plus its bundled LoRA
/// when it has one.
#[tauri::command]
async fn enqueue_generation(
    state: State<'_, AppState>,
    request: EnqueueDto,
) -> Result<fluxgen_core::Job, String> {
    let variant = Variant::from_id(&request.variant_id)
        .ok_or_else(|| format!("unknown variant id: {}", request.variant_id))?;
    let sampler = match request.sampler.as_str() {
        "euler" => fluxgen_core::Sampler::Euler,
        "euler_a" => fluxgen_core::Sampler::EulerAncestral,
        "heun" => fluxgen_core::Sampler::Heun,
        "dpm++2m" => fluxgen_core::Sampler::Dpmpp2m,
        "lcm" => fluxgen_core::Sampler::Lcm,
        other => return Err(format!("unknown sampler: {other}")),
    };
    let init_image = request
        .init_image
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    let mut settings = fluxgen_core::GenerationSettings {
        prompt: request.prompt,
        negative_prompt: request.negative_prompt,
        seed: request.seed,
        steps: request.steps,
        guidance: request.guidance,
        sampler,
        width: request.width,
        height: request.height,
        denoise_strength: request.denoise_strength,
    };
    if init_image.is_some() && settings.denoise_strength.is_none() {
        settings.denoise_strength = Some(0.75);
    }
    settings.validate().map_err(err_string)?;
    let quant = quant_for_job(&state, variant, request.width, request.height)?;
    let mut loras = {
        let store = state
            .store
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        store.list_loras(variant).map_err(err_string)?
    };
    if let Some(bundled) = bundled_lora_entry(&state.paths, variant) {
        loras.insert(0, bundled);
    }
    let id = state
        .next_job_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let job = fluxgen_core::Job::new(id, variant, quant, settings, loras, init_image);
    state.jobs.lock().await.push(job.clone());
    state.queue_notify.notify_one();
    Ok(job)
}

/// All jobs in the queue, in enqueue order.
#[tauri::command]
async fn list_jobs(state: State<'_, AppState>) -> Result<Vec<fluxgen_core::Job>, String> {
    Ok(state.jobs.lock().await.clone())
}

/// Cancel a queued or running job. A running job's request
/// is aborted; the engine may finish the image on its side,
/// but no output is recorded for a cancelled job.
#[tauri::command]
async fn cancel_job(app: AppHandle, state: State<'_, AppState>, job_id: u64) -> Result<(), String> {
    let updated = {
        let mut jobs = state.jobs.lock().await;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == job_id)
            .ok_or_else(|| format!("unknown job id: {job_id}"))?;
        match &job.status {
            fluxgen_core::JobStatus::Queued | fluxgen_core::JobStatus::Running { .. } => {
                job.status = fluxgen_core::JobStatus::Cancelled;
                job.clone()
            }
            _ => return Err("the job has already finished".to_string()),
        }
    };
    if let Some(handle) = state.running_job.lock().await.as_ref() {
        handle.abort();
    }
    let _ = app.emit("job-updated", updated);
    Ok(())
}

/// Remove a finished job from the queue list.
#[tauri::command]
async fn dismiss_job(state: State<'_, AppState>, job_id: u64) -> Result<(), String> {
    let mut jobs = state.jobs.lock().await;
    let position = jobs
        .iter()
        .position(|j| j.id == job_id && j.status.is_terminal())
        .ok_or_else(|| format!("job {job_id} is not finished or does not exist"))?;
    jobs.remove(position);
    Ok(())
}

/// One output of a completed job as a data URL, for the
/// preview pane.
#[tauri::command]
async fn get_job_output(
    state: State<'_, AppState>,
    job_id: u64,
    index: usize,
) -> Result<String, String> {
    let jobs = state.jobs.lock().await;
    let job = jobs
        .iter()
        .find(|j| j.id == job_id)
        .ok_or_else(|| format!("unknown job id: {job_id}"))?;
    let outputs = match &job.status {
        fluxgen_core::JobStatus::Completed { outputs } => outputs,
        _ => return Err("the job has no outputs".to_string()),
    };
    let path = outputs
        .get(index)
        .ok_or_else(|| format!("job {job_id} has no output {index}"))?;
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    use base64::Engine as _;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// A random seed from the system clock and process id.
fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15);
    nanos ^ (std::process::id() as u64).rotate_left(32)
}

/// Make sure the engine is running with the job's variant at
/// the job's quantization, restarting it when it holds a
/// different one. Returns the engine base URL.
async fn ensure_engine_for_job(
    ctx: &JobContext,
    job: &fluxgen_core::Job,
) -> Result<String, String> {
    let mut guard = ctx.engine.lock().await;
    if let Some(process) = guard.as_mut() {
        let alive = process.try_exit_status().map_err(err_string)?.is_none();
        if alive && *ctx.loaded_engine.lock().await == Some((job.variant, job.quant)) {
            return Ok(process.config().base_url());
        }
        if alive {
            process.shutdown().await.map_err(err_string)?;
        }
        *guard = None;
        *ctx.loaded_engine.lock().await = None;
    }
    let config = engine_config_for(&ctx.paths, job.variant, job.quant).map_err(err_string)?;
    let mut process = EngineProcess::launch(config).map_err(err_string)?;
    process.wait_until_ready().await.map_err(err_string)?;
    let base_url = process.config().base_url();
    *guard = Some(process);
    *ctx.loaded_engine.lock().await = Some((job.variant, job.quant));
    Ok(base_url)
}

/// Execute one job end to end and return its terminal
/// status.
async fn execute_job(ctx: &JobContext, job: &fluxgen_core::Job) -> fluxgen_core::JobStatus {
    match execute_job_inner(ctx, job).await {
        Ok(outputs) => fluxgen_core::JobStatus::Completed { outputs },
        Err(reason) => fluxgen_core::JobStatus::Failed { reason },
    }
}

async fn execute_job_inner(
    ctx: &JobContext,
    job: &fluxgen_core::Job,
) -> Result<Vec<PathBuf>, String> {
    let base_url = ensure_engine_for_job(ctx, job).await?;
    // Resolve the concrete seed now so the request and the
    // record agree.
    let seed = job.settings.seed.unwrap_or_else(random_seed);
    {
        let mut jobs = ctx.jobs.lock().await;
        if let Some(stored) = jobs.iter_mut().find(|j| j.id == job.id) {
            stored.settings.seed = Some(seed);
        }
    }
    let loras = job
        .loras
        .iter()
        .filter(|lora| lora.enabled)
        .map(|lora| {
            let name = lora
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| lora.name.clone());
            fluxgen_engine::LoraRequest {
                path: name,
                multiplier: lora.weight,
            }
        })
        .collect();
    let init_image = match &job.init_image {
        Some(path) => Some(
            std::fs::read(path)
                .map_err(|e| format!("could not read source image {}: {e}", path.display()))?,
        ),
        None => None,
    };
    let request = fluxgen_engine::GenerationRequest {
        prompt: job.settings.prompt.clone(),
        negative_prompt: job.settings.negative_prompt.clone(),
        seed,
        steps: job.settings.steps,
        cfg_scale: job.settings.guidance,
        width: job.settings.width,
        height: job.settings.height,
        sampler_name: job.settings.sampler.engine_name().to_string(),
        init_image,
        denoising_strength: job.settings.denoise_strength,
        loras,
    };
    let images = fluxgen_engine::generate(&base_url, &request)
        .await
        .map_err(|e| {
            let message = e.to_string();
            if message.contains("generation request failed") {
                // A transport failure usually means the engine
                // died; forget what it was loaded with.
                let loaded = ctx.loaded_engine.clone();
                tokio::spawn(async move {
                    *loaded.lock().await = None;
                });
            }
            message
        })?;

    let mut recorded = job.settings.clone();
    recorded.seed = Some(seed);
    let record = GenerationRecord {
        variant: job.variant.id().to_string(),
        quant: job.quant.label().to_string(),
        settings: recorded,
        loras: job.loras.clone(),
        init_image: job
            .init_image
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    };
    let record_json = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut outputs = Vec::new();
    for (index, bytes) in images.iter().enumerate() {
        let path = ctx
            .paths
            .outputs
            .join(format!("job-{}-{}-{}.png", job.id, index + 1, seed));
        std::fs::write(&path, bytes)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        fluxgen_store::write_settings_chunk(&path, &record_json).map_err(err_string)?;
        {
            let store = ctx
                .store
                .lock()
                .map_err(|_| "store lock poisoned".to_string())?;
            store
                .add_gallery_record(&fluxgen_store::GalleryRecord {
                    id: None,
                    path: path.clone(),
                    created_at,
                    variant: job.variant,
                    quant: job.quant,
                    settings_json: record_json.clone(),
                })
                .map_err(err_string)?;
        }
        outputs.push(path);
    }
    Ok(outputs)
}

/// Run one queued job: mark it running, execute it, record
/// the outcome, and notify the front-end at each step.
async fn run_one_job(ctx: JobContext, app: AppHandle, job_id: u64) {
    let job = {
        let mut jobs = ctx.jobs.lock().await;
        let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) else {
            return;
        };
        if job.status != fluxgen_core::JobStatus::Queued {
            return;
        }
        job.status = fluxgen_core::JobStatus::Running { progress: None };
        job.clone()
    };
    let _ = app.emit("job-updated", job.clone());
    let outcome = execute_job(&ctx, &job).await;
    let updated = {
        let mut jobs = ctx.jobs.lock().await;
        let Some(stored) = jobs.iter_mut().find(|j| j.id == job_id) else {
            return;
        };
        // A cancellation that landed during execution wins.
        if stored.status == fluxgen_core::JobStatus::Cancelled {
            return;
        }
        stored.status = outcome;
        stored.clone()
    };
    let _ = app.emit("job-updated", updated);
}

/// The generation runner: processes queued jobs one at a
/// time, in order, for as long as the application runs.
async fn generation_loop(app: AppHandle, ctx: JobContext) {
    loop {
        let next_id = {
            let jobs = ctx.jobs.lock().await;
            jobs.iter()
                .find(|j| j.status == fluxgen_core::JobStatus::Queued)
                .map(|j| j.id)
        };
        let Some(job_id) = next_id else {
            ctx.queue_notify.notified().await;
            continue;
        };
        let handle = tokio::spawn(run_one_job(ctx.clone(), app.clone(), job_id));
        *ctx.running_job.lock().await = Some(handle.abort_handle());
        let _ = handle.await;
        *ctx.running_job.lock().await = None;
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
        store: Arc::new(Mutex::new(store)),
        engine: Arc::new(tokio::sync::Mutex::new(None)),
        downloads: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        pending_update: tokio::sync::Mutex::new(None),
        jobs: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        next_job_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        queue_notify: Arc::new(tokio::sync::Notify::new()),
        loaded_engine: Arc::new(tokio::sync::Mutex::new(None)),
        running_job: Arc::new(tokio::sync::Mutex::new(None)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            let ctx = JobContext::from_state(&app.state::<AppState>());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(generation_loop(handle, ctx));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_system_info,
            get_engine_info,
            list_variants,
            start_variant_download,
            cancel_variant_download,
            start_engine_download,
            cancel_engine_download,
            check_for_update,
            install_update,
            get_quant_recommendation,
            engine_start,
            engine_stop,
            engine_status,
            enqueue_generation,
            list_jobs,
            cancel_job,
            dismiss_job,
            get_job_output,
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
