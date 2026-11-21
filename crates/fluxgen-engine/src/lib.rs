//! Supervision of the stable-diffusion.cpp server process.
//!
//! [`EngineProcess`] launches `sd-server`, captures its stdout and
//! stderr into a bounded buffer, waits for its HTTP endpoint to
//! answer, detects unexpected exit (returning the captured output
//! as part of the error), and shuts the process down cleanly.

pub mod config;
pub mod process;

pub use config::EngineConfig;
pub use process::EngineProcess;

/// The pinned stable-diffusion.cpp release, verified against a
/// real binary in the engine integration tests (Issue #9).
pub const ENGINE_VERSION: &str = "stable-diffusion.cpp master-1106-532d2c1";

/// Release identifier of the pinned engine. The engine folder
/// carries a marker file with this value once the pinned engine
/// is installed.
pub const ENGINE_RELEASE_ID: &str = "master-1106-532d2c1";

/// Download URL of the engine bundle archive (the pinned
/// engine files, flat) published on the project's GitHub
/// Releases under the engine release tag.
pub const ENGINE_DOWNLOAD_URL: &str = "https://github.com/brandonmathew/FluxGen/releases/download/engine-master-1106-532d2c1/FluxGen-engine-master-1106-532d2c1.zip";

/// Expected size in bytes of the engine bundle archive.
pub const ENGINE_DOWNLOAD_SIZE_BYTES: u64 = 981_622_874;

/// Expected SHA-256 hex digest of the engine bundle archive.
pub const ENGINE_DOWNLOAD_SHA256: &str =
    "3d6be756da9cdf0590f938a6ebeca8cb8002e0c47887746fc490cc0d3880389a";
