//! Integration tests for the engine supervisor against a fake
//! server binary: a small Python HTTP responder that accepts
//! sd-server style arguments and stands in for `sd-server`, plus a
//! crashing stand-in for the exit path.
//!
//! Unix-only: the fake binaries are scripts with a shebang line.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use fluxgen_core::AppError;
use fluxgen_engine::{EngineConfig, EngineProcess};

const FAKE_SERVER: &str = r#"#!/usr/bin/env python3
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
for i, a in enumerate(args):
    if a == "--listen-port" and i + 1 < len(args):
        port = int(args[i + 1])

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"status":"ok"}')
    def log_message(self, *args):
        pass

print(f"fake sd-server listening on {port}", flush=True)
HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;

const CRASHING_SERVER: &str = r#"#!/usr/bin/env python3
import sys
print("fake sd-server fatal: model file is corrupt", flush=True)
sys.exit(3)
"#;

fn write_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).expect("create script");
    file.write_all(body.as_bytes()).expect("write script");
    let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fluxgen-engine-test-{}-{}",
        std::process::id(),
        name
    ));
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn config(binary: PathBuf, port: u16) -> EngineConfig {
    EngineConfig {
        binary_path: binary,
        port,
        model_path: PathBuf::from("model.gguf"),
        text_encoder_paths: vec![PathBuf::from("enc.gguf")],
        vae_path: PathBuf::from("ae.safetensors"),
        startup_timeout_secs: 15,
    }
}

#[tokio::test]
async fn launches_fake_server_waits_for_health_and_shuts_down() {
    let dir = test_dir("ok");
    let script = write_script(&dir, "fake-sd-server", FAKE_SERVER);
    let port = free_port();
    let mut engine = EngineProcess::launch(config(script, port)).expect("launch");
    engine.wait_until_ready().await.expect("becomes ready");
    // Give the reader task a moment to capture the banner line.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        engine.captured_output().contains("fake sd-server listening"),
        "captured output: {}",
        engine.captured_output()
    );
    engine.shutdown().await.expect("shutdown");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test]
async fn crashing_server_surfaces_output_in_error() {
    let dir = test_dir("crash");
    let script = write_script(&dir, "crashing-sd-server", CRASHING_SERVER);
    let port = free_port();
    let mut engine = EngineProcess::launch(config(script, port)).expect("launch");
    match engine.wait_until_ready().await {
        Err(AppError::EngineExited { code, output }) => {
            assert_eq!(code, Some(3));
            assert!(
                output.contains("model file is corrupt"),
                "error output must carry the engine's own message, got: {output}"
            );
        }
        Err(other) => panic!("expected EngineExited, got: {other}"),
        Ok(()) => panic!("a crashing server must not report ready"),
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
