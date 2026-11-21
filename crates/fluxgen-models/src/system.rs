//! System specification detection.
//!
//! Detection sits behind a plain function returning [`SystemSpecs`]
//! so a platform implementation can be swapped without touching
//! quantization selection. The current implementation covers Linux
//! (via `/proc` and `statvfs`-equivalent filesystem metadata);
//! Windows detection via DXGI/NVML slots into the same function.

use std::path::Path;

use fluxgen_core::AppError;

/// Detected hardware facts that quantization selection depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSpecs {
    /// Total GPU video memory in bytes, when a GPU was detected.
    pub vram_bytes: Option<u64>,
    /// GPU display name, when detected.
    pub gpu_name: Option<String>,
    /// Total system RAM in bytes.
    pub ram_bytes: u64,
    /// Free disk space in bytes on the volume holding the data root.
    pub free_disk_bytes: u64,
}

impl SystemSpecs {
    /// Construct specs directly. Used by tests and by platform
    /// detection code that gathers the values itself.
    pub fn new(
        vram_bytes: Option<u64>,
        gpu_name: Option<String>,
        ram_bytes: u64,
        free_disk_bytes: u64,
    ) -> Self {
        SystemSpecs {
            vram_bytes,
            gpu_name,
            ram_bytes,
            free_disk_bytes,
        }
    }
}

/// Detect the current machine's specifications.
///
/// `data_root` selects the volume whose free disk space is
/// reported. VRAM detection failure is not fatal: the returned
/// specs carry `None` and quantization selection treats the machine
/// as disk-streaming-only instead of guessing.
pub fn detect_system(data_root: &Path) -> Result<SystemSpecs, AppError> {
    let ram_bytes = total_ram_bytes()?;
    let free_disk_bytes = free_disk_bytes(data_root)?;
    let (vram_bytes, gpu_name) = detect_gpu();
    Ok(SystemSpecs {
        vram_bytes,
        gpu_name,
        ram_bytes,
        free_disk_bytes,
    })
}

#[cfg(target_os = "linux")]
fn total_ram_bytes() -> Result<u64, AppError> {
    let text = std::fs::read_to_string("/proc/meminfo")?;
    parse_meminfo_total(&text)
        .ok_or_else(|| AppError::Storage("MemTotal not found in /proc/meminfo".to_string()))
}

#[cfg(not(target_os = "linux"))]
fn total_ram_bytes() -> Result<u64, AppError> {
    Err(AppError::Storage(
        "system RAM detection is not implemented on this platform yet".to_string(),
    ))
}

/// Parse the `MemTotal` line (reported in kB) from `/proc/meminfo`
/// contents into bytes.
pub fn parse_meminfo_total(text: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

fn free_disk_bytes(path: &Path) -> Result<u64, AppError> {
    fs_free_bytes(path)
}

/// Free bytes on the volume containing `path`.
///
/// Implemented with the platform C library through a minimal FFI
/// declaration on Linux (`statvfs`) — kept in this module so the
/// rest of the crate stays free of platform calls.
fn fs_free_bytes(path: &Path) -> Result<u64, AppError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        #[repr(C)]
        struct Statvfs {
            f_bsize: u64,
            f_frsize: u64,
            f_blocks: u64,
            f_bfree: u64,
            f_bavail: u64,
            f_files: u64,
            f_ffree: u64,
            f_favail: u64,
            f_fsid: u64,
            f_flag: u64,
            f_namemax: u64,
            spare: [i32; 6],
        }
        extern "C" {
            fn statvfs(path: *const std::ffi::c_char, buf: *mut Statvfs) -> i32;
        }
        let mut existing = path.to_path_buf();
        while !existing.exists() {
            if !existing.pop() {
                break;
            }
        }
        let c_path = std::ffi::CString::new(existing.as_os_str().as_bytes())
            .map_err(|_| AppError::Storage("data root path contains a NUL byte".to_string()))?;
        let mut buf = Statvfs {
            f_bsize: 0,
            f_frsize: 0,
            f_blocks: 0,
            f_bfree: 0,
            f_bavail: 0,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_fsid: 0,
            f_flag: 0,
            f_namemax: 0,
            spare: [0; 6],
        };
        let rc = unsafe { statvfs(c_path.as_ptr(), &mut buf) };
        if rc != 0 {
            return Err(AppError::Io(std::io::Error::last_os_error()));
        }
        return Ok(buf.f_bavail * buf.f_frsize);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(AppError::Storage(
            "free disk detection is not implemented on this platform yet".to_string(),
        ))
    }
}

/// Detect GPU VRAM and name. On Linux this reads the DRM sysfs
/// entries; when nothing is found the result is `(None, None)` and
/// callers must not guess.
fn detect_gpu() -> (Option<u64>, Option<String>) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let dir = entry.path().join("device");
                if let Ok(text) = std::fs::read_to_string(dir.join("mem_info_vram_total")) {
                    if let Ok(bytes) = text.trim().parse::<u64>() {
                        let name = std::fs::read_to_string(dir.join("label"))
                            .ok()
                            .map(|s| s.trim().to_string());
                        return (Some(bytes), name);
                    }
                }
            }
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_total_parses_to_bytes() {
        let text = "MemTotal:       16384000 kB\nMemFree:         1024000 kB\n";
        assert_eq!(parse_meminfo_total(text), Some(16_384_000 * 1024));
    }

    #[test]
    fn meminfo_without_total_is_none() {
        assert_eq!(parse_meminfo_total("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn free_disk_of_temp_dir_is_positive() {
        let bytes = free_disk_bytes(&std::env::temp_dir()).expect("statvfs on temp dir");
        assert!(bytes > 0);
    }
}
