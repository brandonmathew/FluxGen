//! Engine spike: launch a real `sd-server` binary, wait for its
//! HTTP endpoint to answer, print the outcome, and shut it down.
//!
//! Usage:
//!   cargo run -p fluxgen-engine --example engine_spike -- <path-to-sd-server> [port]
//!
//! Exit code 0: the server answered an HTTP request.
//! Exit code 1: launch/readiness failed; the captured engine
//! output is printed so the reason is visible.

use std::path::PathBuf;
use std::process::ExitCode;

use fluxgen_engine::{EngineConfig, EngineProcess};

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(binary) = args.get(1) else {
        eprintln!("usage: engine_spike <path-to-sd-server> [port]");
        return ExitCode::from(2);
    };
    let port: u16 = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(18099);

    let config = EngineConfig {
        binary_path: PathBuf::from(binary),
        port,
        model_path: PathBuf::from("spike-model.gguf"),
        text_encoder_paths: vec![PathBuf::from("spike-encoder.gguf")],
        vae_path: PathBuf::from("spike-ae.safetensors"),
        lora_model_dir: None,
        startup_timeout_secs: 20,
    };

    let mut engine = match EngineProcess::launch(config) {
        Ok(engine) => engine,
        Err(e) => {
            println!("SPIKE RESULT: launch failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    match engine.wait_until_ready().await {
        Ok(()) => {
            println!(
                "SPIKE RESULT: server answered HTTP at {}",
                engine.config().base_url()
            );
            println!("--- captured engine output ---");
            println!("{}", engine.captured_output());
            let _ = engine.shutdown().await;
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("SPIKE RESULT: not ready: {e}");
            let _ = engine.shutdown().await;
            ExitCode::FAILURE
        }
    }
}
