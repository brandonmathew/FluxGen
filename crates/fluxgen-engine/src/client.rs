//! HTTP client for a running engine's generation API.
//!
//! The engine serves the A1111-style endpoints
//! `/sdapi/v1/txt2img` and `/sdapi/v1/img2img`. Field names
//! and the LoRA request shape were verified against the
//! pinned server's own route source and a live run
//! (Issue #9): LoRAs travel as a `lora` array of
//! `{path, multiplier}` objects whose paths resolve inside
//! the server's `--lora-model-dir`, and image-to-image takes
//! `init_images` (base64) plus `denoising_strength`.

use base64::Engine as _;
use fluxgen_core::AppError;
use serde::Deserialize;

/// One LoRA to apply to a generation.
#[derive(Debug, Clone, PartialEq)]
pub struct LoraRequest {
    /// Path of the LoRA file relative to the server's LoRA
    /// model directory.
    pub path: String,
    /// Application weight.
    pub multiplier: f32,
}

/// One generation request.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationRequest {
    /// Positive prompt, passed exactly as entered.
    pub prompt: String,
    /// Negative prompt; may be empty.
    pub negative_prompt: String,
    /// Concrete seed (randomness is resolved by the caller so
    /// the seed used is the seed recorded).
    pub seed: u64,
    /// Sampling steps.
    pub steps: u32,
    /// Guidance scale.
    pub cfg_scale: f32,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Sampler name in the engine's own spelling.
    pub sampler_name: String,
    /// Source image bytes (PNG or JPEG) for image-to-image;
    /// `None` runs text-to-image.
    pub init_image: Option<Vec<u8>>,
    /// Denoise strength for image-to-image.
    pub denoising_strength: Option<f32>,
    /// LoRA stack, in application order.
    pub loras: Vec<LoraRequest>,
}

/// Generation calls can run for many minutes on local
/// hardware; the ceiling is generous and cancellation is the
/// application's job (dropping the request).
const GENERATION_TIMEOUT_SECS: u64 = 3600;

#[derive(Deserialize)]
struct GenerateResponse {
    images: Vec<String>,
}

/// Run one generation against the engine at `base_url` and
/// return the decoded image bytes, one entry per image.
pub async fn generate(
    base_url: &str,
    request: &GenerationRequest,
) -> Result<Vec<Vec<u8>>, AppError> {
    let img2img = request.init_image.is_some();
    let endpoint = if img2img {
        format!("{base_url}/sdapi/v1/img2img")
    } else {
        format!("{base_url}/sdapi/v1/txt2img")
    };

    let mut body = serde_json::json!({
        "prompt": request.prompt,
        "negative_prompt": request.negative_prompt,
        "seed": request.seed,
        "steps": request.steps,
        "cfg_scale": request.cfg_scale,
        "width": request.width,
        "height": request.height,
        "sampler_name": request.sampler_name,
        "batch_size": 1,
    });
    if !request.loras.is_empty() {
        body["lora"] = serde_json::Value::Array(
            request
                .loras
                .iter()
                .map(|lora| {
                    serde_json::json!({
                        "path": lora.path,
                        "multiplier": lora.multiplier,
                    })
                })
                .collect(),
        );
    }
    if let Some(init) = &request.init_image {
        body["init_images"] =
            serde_json::json!([base64::engine::general_purpose::STANDARD.encode(init)]);
        body["denoising_strength"] = serde_json::json!(request.denoising_strength.unwrap_or(0.75));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(GENERATION_TIMEOUT_SECS))
        .build()
        .map_err(|e| AppError::EngineRequest(format!("generation client failed: {e}")))?;
    let response = client
        .post(&endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::EngineRequest(format!("generation request failed: {e}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(AppError::EngineRequest(format!(
            "engine returned HTTP {status} for generation: {text}"
        )));
    }
    let parsed: GenerateResponse = response
        .json()
        .await
        .map_err(|e| AppError::EngineRequest(format!("generation response did not parse: {e}")))?;
    if parsed.images.is_empty() {
        return Err(AppError::EngineRequest(
            "engine returned no images for the generation".to_string(),
        ));
    }
    let mut images = Vec::new();
    for encoded in &parsed.images {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| {
                AppError::EngineRequest(format!("engine image was not valid base64: {e}"))
            })?;
        images.push(bytes);
    }
    Ok(images)
}
