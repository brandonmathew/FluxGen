//! The image model variants FluxGen can run.

use serde::{Deserialize, Serialize};

/// A supported model variant.
///
/// Each variant is a distinct model family with its own text encoder
/// and quantization table; LoRA entries and presets are tagged with
/// the variant they belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Variant {
    /// Chroma1-HD: FLUX.1-schnell derivative retrained without
    /// alignment, distributed as GGUF with a T5-XXL encoder.
    ChromaHd,
    /// FLUX.1 [dev]: GGUF build with CLIP-L and T5-XXL encoders.
    Flux1Dev,
    /// FLUX.2 Klein 4B: GGUF build with a Qwen3-4B text encoder.
    Flux2Klein4b,
}

impl Variant {
    /// All variants, in display order.
    pub const ALL: [Variant; 3] = [Variant::ChromaHd, Variant::Flux1Dev, Variant::Flux2Klein4b];

    /// Stable identifier used in file paths, database rows, and JSON.
    pub fn id(self) -> &'static str {
        match self {
            Variant::ChromaHd => "chroma1-hd",
            Variant::Flux1Dev => "flux1-dev",
            Variant::Flux2Klein4b => "flux2-klein-4b",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            Variant::ChromaHd => "Chroma1-HD",
            Variant::Flux1Dev => "FLUX.1 [dev]",
            Variant::Flux2Klein4b => "FLUX.2 Klein 4B",
        }
    }

    /// Parse a variant from its stable identifier.
    pub fn from_id(id: &str) -> Option<Variant> {
        Variant::ALL.into_iter().find(|v| v.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for v in Variant::ALL {
            assert_eq!(Variant::from_id(v.id()), Some(v));
        }
    }

    #[test]
    fn unknown_id_is_none() {
        assert_eq!(Variant::from_id("sdxl"), None);
    }
}
