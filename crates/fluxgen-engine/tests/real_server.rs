//! Integration tests against a real `sd-server` binary from
//! stable-diffusion.cpp (Issue #9). These are skipped unless the
//! environment points at a real binary and real model files:
//!
//! - `FLUXGEN_SD_SERVER_BIN`: path to a real `sd-server` executable.
//! - `FLUXGEN_KLEIN_DIR`: folder containing the FLUX.2 Klein 4B
//!   test files: `flux-2-klein-4b-Q4_K_M.gguf` (diffusion model),
//!   `flux2-klein-4b-uncensored-q4_k_m.gguf` (text encoder),
//!   `ae.safetensors` (VAE).
//!
//! Verified against stable-diffusion.cpp master-1106-532d2c1.

use std::path::PathBuf;

use fluxgen_core::AppError;
use fluxgen_engine::{EngineConfig, EngineProcess};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

fn klein_config(
    binary: PathBuf,
    models: &std::path::Path,
    port: u16,
    startup_timeout_secs: u64,
) -> EngineConfig {
    EngineConfig {
        binary_path: binary,
        port,
        model_path: models.join("flux-2-klein-4b-Q4_K_M.gguf"),
        text_encoder_paths: vec![models.join("flux2-klein-4b-uncensored-q4_k_m.gguf")],
        vae_path: models.join("ae.safetensors"),
        startup_timeout_secs,
    }
}

/// A missing model file must surface as a typed engine error that
/// carries the real server's own output, never as a hang or a
/// silent failure.
#[tokio::test]
async fn missing_model_surfaces_real_engine_output() {
    let Some(binary) = env_path("FLUXGEN_SD_SERVER_BIN") else {
        return;
    };
    let models = std::env::temp_dir().join("fluxgen-no-such-models");
    let port = free_port();
    let mut engine =
        EngineProcess::launch(klein_config(binary, &models, port, 60)).expect("launch");
    match engine.wait_until_ready().await {
        Err(AppError::EngineExited { output, .. })
        | Err(AppError::EngineTimeout { output, .. }) => {
            assert!(
                !output.trim().is_empty(),
                "the error must carry the server's captured output"
            );
        }
        Err(other) => panic!("expected EngineExited or EngineTimeout, got: {other}"),
        Ok(()) => panic!("a server with a missing model must not report ready"),
    }
    let _ = engine.shutdown().await;
}

/// The full path: the real server starts under the supervisor,
/// loads the Klein model files with FluxGen's argument set,
/// answers a generation request over HTTP, and shuts down cleanly.
#[tokio::test]
async fn real_server_loads_klein_and_answers_generation() {
    let (Some(binary), Some(models)) = (
        env_path("FLUXGEN_SD_SERVER_BIN"),
        env_path("FLUXGEN_KLEIN_DIR"),
    ) else {
        return;
    };
    let port = free_port();
    // CPU loading of the 4B diffusion model plus the Qwen3 encoder
    // takes minutes; allow generous time.
    let mut engine =
        EngineProcess::launch(klein_config(binary, &models, port, 900)).expect("launch");
    if let Err(e) = engine.wait_until_ready().await {
        panic!("server did not become ready: {e}");
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .expect("client");
    let url = format!("{}/sdapi/v1/txt2img", engine.config().base_url());
    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "prompt": "a red cube on a wooden table",
            "negative_prompt": "",
            "width": 256,
            "height": 256,
            "steps": 1,
            "seed": 42,
            "cfg_scale": 1.0
        }))
        .send()
        .await
        .expect("generation request completes");
    let status = response.status();
    let body = response.text().await.expect("generation response body");
    assert!(
        status.is_success(),
        "generation must succeed, got HTTP {status}: {body}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&body).expect("generation response is JSON");
    let images = parsed["images"].as_array().expect("images array");
    assert!(!images.is_empty(), "at least one image is returned");
    let first = images[0].as_str().expect("image is a base64 string");
    assert!(
        first.len() > 1000,
        "image payload is non-trivial ({} chars)",
        first.len()
    );

    engine.shutdown().await.expect("clean shutdown");
}
