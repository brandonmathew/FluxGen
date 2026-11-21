//! Domain types and shared logic for FluxGen.
//!
//! This crate has no UI, engine, storage, or network dependencies. It
//! defines the vocabulary the other crates share: model variants,
//! quantization levels, generation settings, LoRA entries, presets,
//! jobs, portable data paths, and the application error type.

pub mod error;
pub mod job;
pub mod lora;
pub mod paths;
pub mod preset;
pub mod quant;
pub mod settings;
pub mod variant;

pub use error::AppError;
pub use job::{Job, JobId, JobStatus};
pub use lora::LoraEntry;
pub use paths::DataPaths;
pub use preset::Preset;
pub use quant::Quant;
pub use settings::{GenerationSettings, Sampler};
pub use variant::Variant;

/// Parse an `X.Y.Z` version string into its numeric parts.
/// Returns `None` for anything that is not exactly three
/// dot-separated numbers.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether `candidate` is a strictly newer version than
/// `current`. Unparseable versions are never newer.
pub fn version_is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[cfg(test)]
mod version_tests {
    use super::{parse_version, version_is_newer};

    #[test]
    fn parses_three_part_versions() {
        assert_eq!(parse_version("0.1.8"), Some((0, 1, 8)));
        assert_eq!(parse_version("1.20.3"), Some((1, 20, 3)));
        assert_eq!(parse_version("0.1"), None);
        assert_eq!(parse_version("0.1.8.1"), None);
        assert_eq!(parse_version("v0.1.8"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(version_is_newer("0.1.9", "0.1.8"));
        assert!(version_is_newer("0.2.0", "0.1.9"));
        assert!(version_is_newer("1.0.0", "0.9.9"));
        assert!(!version_is_newer("0.1.8", "0.1.8"));
        assert!(!version_is_newer("0.1.7", "0.1.8"));
        assert!(!version_is_newer("garbage", "0.1.8"));
        assert!(!version_is_newer("0.1.9", "garbage"));
    }
}
