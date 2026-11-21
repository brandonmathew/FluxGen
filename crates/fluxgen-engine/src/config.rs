//! Engine launch configuration.

use std::path::PathBuf;

/// Everything needed to launch one engine instance for one variant
/// at one quantization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    /// Path to the `sd-server` executable.
    pub binary_path: PathBuf,
    /// TCP port the server listens on (loopback only).
    pub port: u16,
    /// Diffusion model GGUF file.
    pub model_path: PathBuf,
    /// Text encoder files, in load order. For FLUX.1 these are
    /// CLIP-L then T5-XXL; for Chroma the T5-XXL; for Klein the
    /// Qwen3 encoder.
    pub text_encoder_paths: Vec<PathBuf>,
    /// VAE file.
    pub vae_path: PathBuf,
    /// Seconds to wait for the health endpoint before failing.
    pub startup_timeout_secs: u64,
}

impl EngineConfig {
    /// Base URL of the running server.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Command-line arguments for `sd-server`.
    ///
    /// Text encoders and the VAE are loaded with the disk
    /// params-backend so their weights are read from file on use
    /// instead of being held in RAM; the diffusion model runs on
    /// the GPU.
    pub fn server_args(&self) -> Vec<String> {
        let mut args = vec![
            "--listen-ip".to_string(),
            "127.0.0.1".to_string(),
            "--listen-port".to_string(),
            self.port.to_string(),
            "--model".to_string(),
            self.model_path.to_string_lossy().into_owned(),
            "--vae".to_string(),
            self.vae_path.to_string_lossy().into_owned(),
            "--params-backend".to_string(),
            "te=disk,vae=disk".to_string(),
        ];
        // sd-server encoder flags: CLIP-L uses --clip_l, T5-XXL
        // uses --t5xxl, a single Qwen3 encoder uses --llm.
        match self.text_encoder_paths.len() {
            1 => {
                args.push("--llm".to_string());
                args.push(
                    self.text_encoder_paths[0]
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            2 => {
                args.push("--clip_l".to_string());
                args.push(
                    self.text_encoder_paths[0]
                        .to_string_lossy()
                        .into_owned(),
                );
                args.push("--t5xxl".to_string());
                args.push(
                    self.text_encoder_paths[1]
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            _ => {}
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(encoders: usize) -> EngineConfig {
        EngineConfig {
            binary_path: PathBuf::from("/engine/sd-server"),
            port: 1234,
            model_path: PathBuf::from("/models/model.gguf"),
            text_encoder_paths: (0..encoders)
                .map(|i| PathBuf::from(format!("/models/enc{i}.gguf")))
                .collect(),
            vae_path: PathBuf::from("/models/ae.safetensors"),
            startup_timeout_secs: 30,
        }
    }

    #[test]
    fn args_include_disk_params_backend() {
        let args = config(1).server_args();
        let joined = args.join(" ");
        assert!(joined.contains("--params-backend te=disk,vae=disk"), "{joined}");
        assert!(joined.contains("--listen-port 1234"), "{joined}");
        assert!(joined.contains("--model /models/model.gguf"), "{joined}");
    }

    #[test]
    fn two_encoders_map_to_clip_and_t5_flags() {
        let args = config(2).server_args();
        let joined = args.join(" ");
        assert!(joined.contains("--clip_l /models/enc0.gguf"), "{joined}");
        assert!(joined.contains("--t5xxl /models/enc1.gguf"), "{joined}");
    }

    #[test]
    fn one_encoder_maps_to_llm_flag() {
        let args = config(1).server_args();
        assert!(args.join(" ").contains("--llm /models/enc0.gguf"));
    }
}
