//! Portable data-root resolution.
//!
//! FluxGen keeps all of its data in one folder next to the
//! executable: models, LoRAs, outputs, presets, and crash reports.
//! During development the `FLUXGEN_DATA_DIR` environment variable
//! overrides the root.

use std::path::PathBuf;

use crate::error::Result;

/// Environment variable that overrides the data root.
pub const DATA_DIR_ENV: &str = "FLUXGEN_DATA_DIR";

/// The set of directories that make up the portable data root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// Root folder containing every other entry.
    pub root: PathBuf,
    /// Downloaded model files, one subfolder per variant.
    pub models: PathBuf,
    /// Imported user LoRA files.
    pub loras: PathBuf,
    /// Generated output images.
    pub outputs: PathBuf,
    /// Preset storage (the preset database lives in the root).
    pub presets: PathBuf,
    /// Crash report text files, newest kept for review at launch.
    pub crashes: PathBuf,
    /// SQLite database file for gallery, presets, and LoRA library.
    pub database: PathBuf,
}

impl DataPaths {
    /// Build the path set for a given root without touching disk.
    pub fn from_root(root: PathBuf) -> Self {
        DataPaths {
            models: root.join("models"),
            loras: root.join("loras"),
            outputs: root.join("outputs"),
            presets: root.join("presets"),
            crashes: root.join("crashes"),
            database: root.join("fluxgen.db"),
            root,
        }
    }

    /// Resolve the data root: `FLUXGEN_DATA_DIR` when set, otherwise
    /// the folder containing the current executable.
    pub fn resolve() -> Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
            if !dir.is_empty() {
                return Ok(DataPaths::from_root(PathBuf::from(dir)));
            }
        }
        let exe = std::env::current_exe()?;
        let folder = exe
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(DataPaths::from_root(folder))
    }

    /// Create every data directory. Called on first run and at every
    /// launch; existing directories are left untouched.
    pub fn ensure_created(&self) -> Result<()> {
        for dir in [
            &self.root,
            &self.models,
            &self.loras,
            &self.outputs,
            &self.presets,
            &self.crashes,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Per-variant model directory.
    pub fn variant_models_dir(&self, variant: crate::Variant) -> PathBuf {
        self.models.join(variant.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_rooted_in_one_folder() {
        let paths = DataPaths::from_root(PathBuf::from("/data/fluxgen"));
        assert_eq!(paths.models, PathBuf::from("/data/fluxgen/models"));
        assert_eq!(paths.loras, PathBuf::from("/data/fluxgen/loras"));
        assert_eq!(paths.outputs, PathBuf::from("/data/fluxgen/outputs"));
        assert_eq!(paths.crashes, PathBuf::from("/data/fluxgen/crashes"));
        assert_eq!(paths.database, PathBuf::from("/data/fluxgen/fluxgen.db"));
    }

    #[test]
    fn variant_dirs_use_stable_ids() {
        let paths = DataPaths::from_root(PathBuf::from("/d"));
        assert_eq!(
            paths.variant_models_dir(crate::Variant::ChromaHd),
            PathBuf::from("/d/models/chroma1-hd")
        );
    }

    #[test]
    fn ensure_created_makes_all_dirs() {
        let tmp = std::env::temp_dir().join(format!("fluxgen-paths-test-{}", std::process::id()));
        let paths = DataPaths::from_root(tmp.clone());
        paths.ensure_created().expect("create dirs");
        for dir in [
            &paths.models,
            &paths.loras,
            &paths.outputs,
            &paths.presets,
            &paths.crashes,
        ] {
            assert!(dir.is_dir(), "missing {}", dir.display());
        }
        std::fs::remove_dir_all(&tmp).expect("cleanup");
    }
}
