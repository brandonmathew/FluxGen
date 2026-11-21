//! Resumable, verified file downloads.
//!
//! A download writes to `<destination>.part`. On restart, an
//! existing `.part` file is resumed with an HTTP Range request from
//! its current length. When the transfer completes, the file size
//! is checked against the expected size and the SHA-256 hash is
//! checked when one is known; only then is the `.part` file renamed
//! to the destination. A failed verification deletes nothing the
//! user already had — the `.part` file is left in place and the
//! error carries the mismatch detail.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use fluxgen_core::AppError;

/// A single file download job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    /// Source URL.
    pub url: String,
    /// Final destination path (the `.part` suffix is managed
    /// internally).
    pub destination: PathBuf,
    /// Expected final size in bytes, from the repository listing.
    pub expected_size_bytes: u64,
    /// Expected SHA-256 hex digest, when the publisher provides one.
    pub expected_sha256: Option<String>,
}

impl DownloadTask {
    /// Path of the in-progress partial file.
    pub fn part_path(&self) -> PathBuf {
        part_path_for(&self.destination)
    }
}

/// The `.part` path for a destination path.
pub fn part_path_for(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Bytes already downloaded for a task (the resume offset): the
/// length of the `.part` file, or 0 when none exists. A `.part`
/// file longer than the expected size is stale and reports 0 so
/// the transfer restarts cleanly.
pub fn resume_offset(task: &DownloadTask) -> u64 {
    let part = task.part_path();
    match std::fs::metadata(&part) {
        Ok(meta) if meta.len() <= task.expected_size_bytes => meta.len(),
        _ => 0,
    }
}

/// Compute the SHA-256 hex digest of a file.
pub fn sha256_of_file(path: &Path) -> Result<String, AppError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

/// Verify a completed file against the task expectations: exact
/// size, and hash when one is known.
pub fn verify_file(task: &DownloadTask, path: &Path) -> Result<(), AppError> {
    let meta = std::fs::metadata(path)?;
    if meta.len() != task.expected_size_bytes {
        return Err(AppError::VerificationFailed {
            path: path.to_path_buf(),
            reason: format!(
                "size mismatch: expected {} bytes, found {} bytes",
                task.expected_size_bytes,
                meta.len()
            ),
        });
    }
    if let Some(expected) = &task.expected_sha256 {
        let actual = sha256_of_file(path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(AppError::VerificationFailed {
                path: path.to_path_buf(),
                reason: format!("SHA-256 mismatch: expected {expected}, computed {actual}"),
            });
        }
    }
    Ok(())
}

/// Download a file, resuming from an existing `.part` file, then
/// verify and move it into place.
///
/// Progress is reported through `on_progress` as
/// `(bytes_written_total, expected_total)` after each chunk.
pub async fn download_file<F>(task: &DownloadTask, mut on_progress: F) -> Result<PathBuf, AppError>
where
    F: FnMut(u64, u64),
{
    if let Some(parent) = task.destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = task.part_path();
    let mut offset = resume_offset(task);
    if offset == task.expected_size_bytes && task.expected_size_bytes > 0 {
        // A previous run downloaded everything but did not finish
        // verification/rename.
        verify_file(task, &part)?;
        std::fs::rename(&part, &task.destination)?;
        return Ok(task.destination.clone());
    }
    if offset == 0 && part.exists() {
        // Stale oversized partial: start over.
        std::fs::remove_file(&part)?;
    }

    let client = reqwest::Client::new();
    let mut request = client.get(&task.url);
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| AppError::Transfer(format!("request to {} failed: {e}", task.url)))?;

    let mut append = offset > 0;
    if offset > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        // Server ignored the Range header: restart from zero.
        offset = 0;
        append = false;
    }
    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return Err(AppError::Transfer(format!(
            "request to {} returned HTTP {}",
            task.url,
            response.status()
        )));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(append)
        .write(!append)
        .truncate(!append)
        .open(&part)
        .await?;

    let mut written = offset;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Transfer(format!("transfer interrupted: {e}")))?;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        on_progress(written, task.expected_size_bytes);
    }
    file.flush().await?;
    drop(file);

    verify_file(task, &part)?;
    std::fs::rename(&part, &task.destination)?;
    on_progress(task.expected_size_bytes, task.expected_size_bytes);
    Ok(task.destination.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fluxgen-download-test-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn part_path_appends_suffix() {
        let p = part_path_for(Path::new("/models/model.gguf"));
        assert_eq!(p, PathBuf::from("/models/model.gguf.part"));
    }

    #[test]
    fn resume_offset_reads_part_length() {
        let dir = test_dir("offset");
        let task = DownloadTask {
            url: "http://localhost/file".to_string(),
            destination: dir.join("f.bin"),
            expected_size_bytes: 100,
            expected_sha256: None,
        };
        assert_eq!(resume_offset(&task), 0);
        std::fs::write(task.part_path(), vec![0u8; 40]).expect("write part");
        assert_eq!(resume_offset(&task), 40);
        // Oversized partial is stale and restarts.
        std::fs::write(task.part_path(), vec![0u8; 140]).expect("write oversized part");
        assert_eq!(resume_offset(&task), 0);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn verify_checks_size_and_hash() {
        let dir = test_dir("verify");
        let data = b"fluxgen download verification payload";
        let path = dir.join("payload.bin");
        std::fs::write(&path, data).expect("write payload");
        let digest = sha256_of_file(&path).expect("hash");
        let task = DownloadTask {
            url: "http://localhost/payload".to_string(),
            destination: path.clone(),
            expected_size_bytes: data.len() as u64,
            expected_sha256: Some(digest),
        };
        assert!(verify_file(&task, &path).is_ok());

        let mut wrong_size = task.clone();
        wrong_size.expected_size_bytes += 1;
        assert!(verify_file(&wrong_size, &path).is_err());

        let mut wrong_hash = task.clone();
        wrong_hash.expected_sha256 = Some("0".repeat(64));
        match verify_file(&wrong_hash, &path) {
            Err(AppError::VerificationFailed { reason, .. }) => {
                assert!(reason.contains("SHA-256 mismatch"), "reason: {reason}");
            }
            other => panic!("expected VerificationFailed, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn download_completes_from_localhost_server() {
        // Minimal one-shot HTTP responder on localhost serving a
        // fixed payload, with Range support for resume.
        let payload: Vec<u8> = (0u32..50_000).map(|i| (i % 251) as u8).collect();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server_payload = std::sync::Arc::new(payload.clone());
        let server = tokio::spawn(async move {
            // Serve up to two requests (initial + resumed).
            for _ in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let server_payload = server_payload.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).to_string();
                    let mut start = 0usize;
                    let mut status = "200 OK";
                    for line in request.lines() {
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            if let Some(end) = v.find('-') {
                                start = v[..end].parse().unwrap_or(0);
                                status = "206 Partial Content";
                            }
                        }
                    }
                    let body = &server_payload[start..];
                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                });
            }
        });

        let dir = test_dir("http");
        let digest = {
            let mut hasher = Sha256::new();
            hasher.update(&payload);
            hex::encode(hasher.finalize())
        };
        let task = DownloadTask {
            url: format!("http://{addr}/model.bin"),
            destination: dir.join("model.bin"),
            expected_size_bytes: payload.len() as u64,
            expected_sha256: Some(digest),
        };

        // Seed a partial file with the first 10,000 bytes to prove
        // the resume path completes the file.
        std::fs::write(task.part_path(), &payload[..10_000]).expect("seed part");
        let out = download_file(&task, |_, _| {}).await.expect("download");
        assert_eq!(out, task.destination);
        let written = std::fs::read(&out).expect("read result");
        assert_eq!(written, payload);
        assert!(!task.part_path().exists(), "part file must be renamed away");

        server.abort();
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
