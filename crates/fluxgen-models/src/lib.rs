//! Model variant profiles, system detection, quantization
//! selection, and downloads for FluxGen.

pub mod download;
pub mod profiles;
pub mod quant_select;
pub mod system;

pub use download::{download_file, DownloadTask};
pub use profiles::{profile_for, ModelFile, VariantProfile};
pub use quant_select::{pick_quant, step_down_until_fit, QuantChoice};
pub use system::{detect_system, SystemSpecs};
