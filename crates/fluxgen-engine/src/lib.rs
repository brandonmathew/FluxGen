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
