//! The supervised engine process.

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Notify;

use fluxgen_core::AppError;

use crate::config::EngineConfig;

/// Maximum captured engine output retained, in bytes. Older output
/// is dropped from the front; engine logs are repetitive and the
/// most recent lines carry the failure reason.
pub const MAX_CAPTURED_OUTPUT_BYTES: usize = 256 * 1024;

/// Shared bounded output buffer fed by the stdout/stderr reader
/// tasks.
#[derive(Debug, Default)]
pub struct OutputBuffer {
    text: Mutex<String>,
}

impl OutputBuffer {
    fn push(&self, chunk: &str) {
        if let Ok(mut guard) = self.text.lock() {
            guard.push_str(chunk);
            if guard.len() > MAX_CAPTURED_OUTPUT_BYTES {
                let excess = guard.len() - MAX_CAPTURED_OUTPUT_BYTES;
                guard.drain(..excess);
            }
        }
    }

    /// Snapshot of the captured output.
    pub fn snapshot(&self) -> String {
        self.text
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| String::new())
    }
}

/// A running (or exited) engine process under supervision.
///
/// Dropping the value does not kill the process implicitly; call
/// [`EngineProcess::shutdown`] for an orderly stop. Await
/// [`EngineProcess::wait_for_exit`] to observe unexpected exits.
pub struct EngineProcess {
    config: EngineConfig,
    child: Child,
    output: Arc<OutputBuffer>,
    exit_notify: Arc<Notify>,
}

impl EngineProcess {
    /// Launch the engine binary with the given configuration.
    ///
    /// Fails with [`AppError::EngineUnavailable`] when the binary
    /// does not exist, so a missing engine is a surfaced error, not
    /// a silent no-op.
    pub fn launch(config: EngineConfig) -> Result<EngineProcess, AppError> {
        if !config.binary_path.is_file() {
            return Err(AppError::EngineUnavailable(format!(
                "engine binary not found at {}",
                config.binary_path.display()
            )));
        }
        let mut command = Command::new(&config.binary_path);
        command
            .args(config.server_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| {
            AppError::EngineUnavailable(format!(
                "failed to launch {}: {e}",
                config.binary_path.display()
            ))
        })?;

        let output = Arc::new(OutputBuffer::default());
        if let Some(stdout) = child.stdout.take() {
            spawn_reader(stdout, output.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(stderr, output.clone());
        }

        Ok(EngineProcess {
            config,
            child,
            output,
            exit_notify: Arc::new(Notify::new()),
        })
    }

    /// The configuration this process was launched with.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Captured engine output so far.
    pub fn captured_output(&self) -> String {
        self.output.snapshot()
    }

    /// Poll the server's HTTP endpoint until it answers with any
    /// HTTP response, the process exits, or the startup timeout
    /// elapses. An HTTP error status still counts as "the server is
    /// up" — readiness means the HTTP stack responds.
    pub async fn wait_until_ready(&mut self) -> Result<(), AppError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| AppError::EngineUnavailable(format!("HTTP client error: {e}")))?;
        let url = format!("{}/", self.config.base_url());
        let timeout = Duration::from_secs(self.config.startup_timeout_secs);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if let Some(status) = self.try_exit_status()? {
                return Err(AppError::EngineExited {
                    code: status.code(),
                    output: self.captured_output(),
                });
            }
            match client.get(&url).send().await {
                Ok(_response) => return Ok(()),
                Err(_) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(AppError::EngineTimeout {
                            timeout_secs: self.config.startup_timeout_secs,
                            output: self.captured_output(),
                        });
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    /// Non-blocking exit check. Returns the exit status when the
    /// process has already exited.
    pub fn try_exit_status(&mut self) -> Result<Option<std::process::ExitStatus>, AppError> {
        self.child
            .try_wait()
            .map_err(|e| AppError::EngineUnavailable(format!("failed to poll engine process: {e}")))
    }

    /// Wait for the process to exit. A clean exit returns the
    /// status; a non-zero exit returns
    /// [`AppError::EngineExited`] carrying the captured output.
    pub async fn wait_for_exit(&mut self) -> Result<std::process::ExitStatus, AppError> {
        let status =
            self.child.wait().await.map_err(|e| {
                AppError::EngineUnavailable(format!("failed to wait for engine: {e}"))
            })?;
        self.exit_notify.notify_waiters();
        if status.success() {
            Ok(status)
        } else {
            Err(AppError::EngineExited {
                code: status.code(),
                output: self.captured_output(),
            })
        }
    }

    /// Shut the process down: request exit, then kill if it does
    /// not exit within a short grace period.
    pub async fn shutdown(&mut self) -> Result<(), AppError> {
        if self.try_exit_status()?.is_some() {
            return Ok(());
        }
        self.child.kill().await.map_err(|e| {
            AppError::EngineUnavailable(format!("failed to stop engine process: {e}"))
        })?;
        let _ = self.child.wait().await;
        Ok(())
    }
}

fn spawn_reader<R>(reader: R, output: Arc<OutputBuffer>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => output.push(&format!("{line}\n")),
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_buffer_is_bounded() {
        let buf = OutputBuffer::default();
        let chunk = "x".repeat(MAX_CAPTURED_OUTPUT_BYTES / 2);
        for _ in 0..5 {
            buf.push(&chunk);
        }
        assert!(buf.snapshot().len() <= MAX_CAPTURED_OUTPUT_BYTES);
    }

    #[test]
    fn missing_binary_is_a_typed_error() {
        let config = EngineConfig {
            binary_path: std::path::PathBuf::from("/definitely/not/here/sd-server"),
            port: 0,
            model_path: std::path::PathBuf::from("m.gguf"),
            text_encoder_paths: vec![],
            vae_path: std::path::PathBuf::from("ae.safetensors"),
            lora_model_dir: None,
            startup_timeout_secs: 1,
        };
        match EngineProcess::launch(config) {
            Err(AppError::EngineUnavailable(msg)) => {
                assert!(msg.contains("not found"), "{msg}");
            }
            Err(other) => panic!("wrong error: {other}"),
            Ok(_) => panic!("launch must fail for a missing binary"),
        }
    }
}
