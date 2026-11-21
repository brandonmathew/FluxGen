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
