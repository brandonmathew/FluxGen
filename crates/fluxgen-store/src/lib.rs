//! Persistence for FluxGen: a SQLite database in the portable
//! data root holding gallery records, presets, and the LoRA
//! library, plus PNG settings metadata embed/read.

pub mod db;
pub mod png_meta;

pub use db::{GalleryRecord, Store};
pub use png_meta::{read_settings_chunk, write_settings_chunk, SETTINGS_CHUNK_TYPE};
