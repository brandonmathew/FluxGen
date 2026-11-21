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
//!
//! Files at or above [`SEGMENTED_MIN_BYTES`] download in segments
//! when the server answers Range requests: up to
//! [`MAX_SEGMENTS`] parallel connections each fetch one byte
//! range into the same pre-sized `.part` file. Per-segment
//! progress is kept in a `<destination>.part.segments.json`
//! sidecar so a cancelled or failed segmented download resumes
//! per segment. Servers without Range support, and all smaller
//! files, use the single-connection path.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use fluxgen_core::AppError;

/// Smallest file size that downloads in parallel segments.
pub const SEGMENTED_MIN_BYTES: u64 = 32 * 1024 * 1024;

/// Maximum number of parallel segment connections.
pub const MAX_SEGMENTS: u64 = 8;

/// Smallest size of one segment; also caps the segment count
/// for files just above the segmented threshold.
pub const MIN_SEGMENT_BYTES: u64 = 4 * 1024 * 1024;

/// Per-segment progress is written to the sidecar state file
/// at most once per this many bytes.
const STATE_PERSIST_INTERVAL: u64 = 1024 * 1024;

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

/// Download a file, resuming from partial state, then verify
/// and move it into place.
///
/// Files at or above [`SEGMENTED_MIN_BYTES`] use the segmented
/// path when the server supports Range requests and fall back
/// to a single connection when it does not. Progress is
/// reported through `on_progress` as
/// `(bytes_written_total, expected_total)`.
pub async fn download_file<F>(task: &DownloadTask, on_progress: F) -> Result<PathBuf, AppError>
where
    F: FnMut(u64, u64),
{
    // A partial file already holding every byte (a previous
    // run finished transferring but not verifying) completes
    // without any transfer. Segmented partials are pre-sized
    // to the full length, so this shortcut must not fire when
    // segment state exists: the segmented path decides what
    // is still missing.
    if task.expected_size_bytes > 0 && !segment_state_path(task).exists() {
        if let Ok(meta) = std::fs::metadata(task.part_path()) {
            if meta.len() == task.expected_size_bytes {
                verify_file(task, &task.part_path())?;
                std::fs::rename(task.part_path(), &task.destination)?;
                return Ok(task.destination.clone());
            }
        }
    }
    if task.expected_size_bytes >= SEGMENTED_MIN_BYTES && server_supports_ranges(task).await {
        return download_segmented(task, on_progress).await;
    }
    download_single(task, on_progress).await
}

/// Whether the server answers a one-byte Range request with
/// `206 Partial Content`. Any failure reads as "no ranges":
/// the single-connection path then surfaces the real error.
async fn server_supports_ranges(task: &DownloadTask) -> bool {
    let client = reqwest::Client::new();
    match client
        .get(&task.url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
    {
        Ok(response) => response.status() == reqwest::StatusCode::PARTIAL_CONTENT,
        Err(_) => false,
    }
}

/// Download a file over a single connection, resuming from an
/// existing `.part` file, then verify and move it into place.
async fn download_single<F>(task: &DownloadTask, mut on_progress: F) -> Result<PathBuf, AppError>
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

/// Progress of one segment of a segmented download. `end` is
/// exclusive.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SegmentProgress {
    /// First byte of the segment.
    start: u64,
    /// One past the last byte of the segment.
    end: u64,
    /// Bytes of this segment already written to the `.part`
    /// file.
    downloaded: u64,
}

impl SegmentProgress {
    /// Length of the segment in bytes.
    fn len(&self) -> u64 {
        self.end - self.start
    }

    /// Whether every byte of the segment is written.
    fn is_complete(&self) -> bool {
        self.downloaded >= self.len()
    }
}

/// Sidecar state of a segmented download, stored as JSON next
/// to the `.part` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentState {
    /// Expected total size; a mismatch with the task restarts
    /// the download.
    expected_size_bytes: u64,
    /// Segments in file order.
    segments: Vec<SegmentProgress>,
}

/// Split `size` bytes into segment ranges: at most
/// [`MAX_SEGMENTS`] segments, none smaller than
/// [`MIN_SEGMENT_BYTES`] except the last.
fn plan_segments(size: u64) -> Vec<SegmentProgress> {
    let count = (size / MIN_SEGMENT_BYTES).clamp(1, MAX_SEGMENTS);
    let chunk = size.div_ceil(count);
    let mut segments = Vec::new();
    let mut start = 0;
    while start < size {
        let end = (start + chunk).min(size);
        segments.push(SegmentProgress {
            start,
            end,
            downloaded: 0,
        });
        start = end;
    }
    segments
}

/// Path of the segmented-download sidecar state file.
fn segment_state_path(task: &DownloadTask) -> PathBuf {
    let mut name = task.part_path().into_os_string();
    name.push(".segments.json");
    PathBuf::from(name)
}

/// Load resumable segment state: the sidecar must parse, match
/// the task size, and sit next to a `.part` file of exactly
/// the expected length (segmented partials are pre-sized).
fn load_segment_state(task: &DownloadTask) -> Option<SegmentState> {
    let state: SegmentState =
        serde_json::from_slice(&std::fs::read(segment_state_path(task)).ok()?).ok()?;
    if state.expected_size_bytes != task.expected_size_bytes {
        return None;
    }
    let meta = std::fs::metadata(task.part_path()).ok()?;
    if meta.len() != task.expected_size_bytes {
        return None;
    }
    Some(state)
}

/// Write the sidecar state file. A torn write fails to parse
/// on the next run and simply restarts the download, so no
/// atomic rename is needed.
async fn persist_segment_state(task: &DownloadTask, state: &SegmentState) -> Result<(), AppError> {
    let json = serde_json::to_vec(state)
        .map_err(|e| AppError::Transfer(format!("segment state encode failed: {e}")))?;
    tokio::fs::write(segment_state_path(task), json).await?;
    Ok(())
}

/// Download one segment into the shared `.part` file through
/// its own connection and file handle.
async fn download_one_segment<F>(
    client: reqwest::Client,
    task: &DownloadTask,
    index: usize,
    state: Arc<Mutex<SegmentState>>,
    total_downloaded: Arc<AtomicU64>,
    on_progress: Arc<Mutex<F>>,
) -> Result<(), AppError>
where
    F: FnMut(u64, u64),
{
    let segment = {
        let guard = state.lock().expect("segment state lock");
        guard.segments[index]
    };
    if segment.is_complete() {
        return Ok(());
    }
    let part = task.part_path();
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&part)
        .await?;
    let position = segment.start + segment.downloaded;
    file.seek(std::io::SeekFrom::Start(position)).await?;

    let response = client
        .get(&task.url)
        .header(
            reqwest::header::RANGE,
            format!("bytes={}-{}", position, segment.end - 1),
        )
        .send()
        .await
        .map_err(|e| AppError::Transfer(format!("request to {} failed: {e}", task.url)))?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(AppError::Transfer(format!(
            "request to {} returned HTTP {} for a segment range",
            task.url,
            response.status()
        )));
    }

    let mut written_here = segment.downloaded;
    let mut since_persist = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Transfer(format!("transfer interrupted: {e}")))?;
        file.write_all(&chunk).await?;
        written_here += chunk.len() as u64;
        since_persist += chunk.len() as u64;
        {
            let mut guard = state.lock().expect("segment state lock");
            guard.segments[index].downloaded = written_here;
        }
        let total =
            total_downloaded.fetch_add(chunk.len() as u64, Ordering::SeqCst) + chunk.len() as u64;
        on_progress.lock().expect("progress lock")(total, task.expected_size_bytes);
        if since_persist >= STATE_PERSIST_INTERVAL {
            since_persist = 0;
            let snapshot = {
                let guard = state.lock().expect("segment state lock");
                guard.clone()
            };
            persist_segment_state(task, &snapshot).await?;
        }
    }
    file.flush().await?;
    if written_here < segment.len() {
        return Err(AppError::Transfer(format!(
            "segment {} of {} ended early at {} of {} bytes",
            index + 1,
            state.lock().expect("segment state lock").segments.len(),
            written_here,
            segment.len()
        )));
    }
    let snapshot = {
        let guard = state.lock().expect("segment state lock");
        guard.clone()
    };
    persist_segment_state(task, &snapshot).await?;
    Ok(())
}

/// Download a file in parallel segments, resuming from the
/// sidecar state when it is present, then verify and move the
/// file into place.
async fn download_segmented<F>(task: &DownloadTask, on_progress: F) -> Result<PathBuf, AppError>
where
    F: FnMut(u64, u64),
{
    if let Some(parent) = task.destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = task.part_path();
    let state = match load_segment_state(task) {
        Some(state) => state,
        None => {
            // Fresh start: pre-size the partial file and record
            // an empty segment plan.
            let file = std::fs::File::create(&part)?;
            file.set_len(task.expected_size_bytes)?;
            drop(file);
            let state = SegmentState {
                expected_size_bytes: task.expected_size_bytes,
                segments: plan_segments(task.expected_size_bytes),
            };
            persist_segment_state(task, &state).await?;
            state
        }
    };
    let resumed: u64 = state.segments.iter().map(|s| s.downloaded).sum();
    let total_downloaded = Arc::new(AtomicU64::new(resumed));
    let state = Arc::new(Mutex::new(state));
    let on_progress = Arc::new(Mutex::new(on_progress));
    if resumed > 0 {
        on_progress.lock().expect("progress lock")(resumed, task.expected_size_bytes);
    }

    let client = reqwest::Client::new();
    let mut jobs = Vec::new();
    for index in 0..state.lock().expect("segment state lock").segments.len() {
        jobs.push(download_one_segment(
            client.clone(),
            task,
            index,
            state.clone(),
            total_downloaded.clone(),
            on_progress.clone(),
        ));
    }
    for result in futures_util::future::join_all(jobs).await {
        result?;
    }

    if let Err(e) = verify_file(task, &part) {
        // A segment marked complete held wrong bytes. Drop
        // the state so the next attempt replans from scratch
        // instead of trusting the same segments again.
        let _ = std::fs::remove_file(segment_state_path(task));
        return Err(e);
    }
    std::fs::rename(&part, &task.destination)?;
    let _ = std::fs::remove_file(segment_state_path(task));
    on_progress.lock().expect("progress lock")(task.expected_size_bytes, task.expected_size_bytes);
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

    #[test]
    fn plan_segments_covers_the_file() {
        let size = 40 * 1024 * 1024;
        let segments = plan_segments(size);
        assert_eq!(segments.len(), 8);
        assert_eq!(segments.first().expect("first").start, 0);
        assert_eq!(segments.last().expect("last").end, size);
        for pair in segments.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "segments must be contiguous");
        }
        // A file just above the minimum segment size stays whole.
        assert_eq!(plan_segments(5 * 1024 * 1024).len(), 1);
    }

    /// A Range-capable localhost server that records every
    /// requested range. Returns the base URL and the recorded
    /// `(start, end)` list (`end` is `None` for an open range).
    async fn start_range_server(
        payload: Vec<u8>,
    ) -> (
        String,
        Arc<Mutex<Vec<(u64, Option<u64>)>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let payload = Arc::new(payload);
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let server_ranges = ranges.clone();
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let payload = payload.clone();
                let ranges = server_ranges.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).to_string();
                    let mut start = 0usize;
                    let mut end: Option<usize> = None;
                    let mut status = "200 OK";
                    for line in request.lines() {
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            status = "206 Partial Content";
                            let mut parts = v.splitn(2, '-');
                            start = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
                            end = parts.next().and_then(|p| p.parse().ok());
                            ranges
                                .lock()
                                .expect("ranges lock")
                                .push((start as u64, end.map(|e| e as u64)));
                        }
                    }
                    let last = end.unwrap_or(payload.len() - 1).min(payload.len() - 1);
                    let body = &payload[start..=last];
                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                });
            }
        });
        (format!("http://{addr}/model.bin"), ranges, server)
    }

    #[tokio::test]
    async fn segmented_download_uses_multiple_connections() {
        // 40 MiB: above the segmented threshold.
        let payload: Vec<u8> = (0u32..40 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let (url, ranges, server) = start_range_server(payload.clone()).await;
        let dir = test_dir("segmented");
        let digest = {
            let mut hasher = Sha256::new();
            hasher.update(&payload);
            hex::encode(hasher.finalize())
        };
        let task = DownloadTask {
            url,
            destination: dir.join("model.bin"),
            expected_size_bytes: payload.len() as u64,
            expected_sha256: Some(digest),
        };
        let out = download_file(&task, |_, _| {}).await.expect("download");
        assert_eq!(std::fs::read(&out).expect("read result"), payload);
        assert!(
            !segment_state_path(&task).exists(),
            "sidecar state must be removed after completion"
        );
        let recorded = ranges.lock().expect("ranges lock").clone();
        let mut starts: Vec<u64> = recorded.iter().map(|(start, _)| *start).collect();
        starts.sort_unstable();
        starts.dedup();
        assert!(
            starts.len() >= 4,
            "expected several segment connections, got range starts {starts:?}"
        );
        server.abort();
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn segmented_download_resumes_completed_segments() {
        let payload: Vec<u8> = (0u32..40 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let (url, ranges, server) = start_range_server(payload.clone()).await;
        let dir = test_dir("segmented-resume");
        let digest = {
            let mut hasher = Sha256::new();
            hasher.update(&payload);
            hex::encode(hasher.finalize())
        };
        let task = DownloadTask {
            url,
            destination: dir.join("model.bin"),
            expected_size_bytes: payload.len() as u64,
            expected_sha256: Some(digest),
        };
        // Seed a partial segmented download: segment 0 fully
        // written, the rest untouched.
        let mut segments = plan_segments(task.expected_size_bytes);
        let first = segments[0];
        {
            let mut file = std::fs::File::create(task.part_path()).expect("create part");
            file.set_len(task.expected_size_bytes).expect("size part");
            use std::io::Write;
            file.write_all(&payload[first.start as usize..first.end as usize])
                .expect("write segment 0");
        }
        segments[0].downloaded = first.len();
        let state = SegmentState {
            expected_size_bytes: task.expected_size_bytes,
            segments,
        };
        std::fs::write(
            segment_state_path(&task),
            serde_json::to_vec(&state).expect("encode state"),
        )
        .expect("write state");

        let out = download_file(&task, |_, _| {}).await.expect("download");
        assert_eq!(std::fs::read(&out).expect("read result"), payload);
        let recorded = ranges.lock().expect("ranges lock").clone();
        // The one-byte probe asks for bytes=0-0; no real segment
        // request may start at 0, because segment 0 was already
        // complete.
        assert!(
            recorded
                .iter()
                .all(|(start, end)| *start != 0 || *end == Some(0)),
            "segment 0 was re-downloaded: {recorded:?}"
        );
        server.abort();
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
