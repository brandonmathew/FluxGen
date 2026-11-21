//! Variant profiles: the exact file sets FluxGen downloads for each
//! variant, with published sizes and quantization tables.
//!
//! Sizes are decimal gigabytes (1 GB = 1,000,000,000 bytes) as
//! listed by the publishing repositories. A file is only accepted
//! after download when its size matches the listing and, where the
//! publisher provides one, its SHA-256 hash matches.

use fluxgen_core::{Quant, Variant};

/// One downloadable file belonging to a variant.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelFile {
    /// Repository the file is downloaded from, in `owner/name` form.
    pub repo: &'static str,
    /// File name inside the repository.
    pub file_name: &'static str,
    /// Expected size in bytes from the repository listing.
    pub size_bytes: u64,
    /// Role of the file inside the variant.
    pub role: FileRole,
}

/// What a model file is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    /// The diffusion transformer (the per-quantization file).
    DiffusionModel,
    /// A text encoder.
    TextEncoder,
    /// The VAE.
    Vae,
    /// A LoRA that ships as part of the variant.
    BundledLora,
}

/// One row of a variant quantization table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantOption {
    /// Quantization level.
    pub quant: Quant,
    /// Publisher's exact quantization label for the file name.
    pub label: &'static str,
    /// Diffusion model file name at this quantization.
    pub file_name: &'static str,
    /// Diffusion model file size in bytes.
    pub size_bytes: u64,
}

/// Everything FluxGen needs to install and run one variant.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantProfile {
    /// The variant described.
    pub variant: Variant,
    /// Files shared by every quantization of this variant
    /// (encoders, VAE, bundled LoRA).
    pub shared_files: &'static [ModelFile],
    /// Quantization table, smallest first.
    pub quant_options: &'static [QuantOption],
}

impl VariantProfile {
    /// The quantization option for a level, when the variant
    /// publishes one.
    pub fn quant_option(&self, quant: Quant) -> Option<&QuantOption> {
        self.quant_options.iter().find(|o| o.quant == quant)
    }

    /// Total download size in bytes at a given quantization:
    /// shared files plus the diffusion model file.
    pub fn total_size_bytes(&self, quant: Quant) -> Option<u64> {
        let option = self.quant_option(quant)?;
        let shared: u64 = self.shared_files.iter().map(|f| f.size_bytes).sum();
        Some(shared + option.size_bytes)
    }
}

// ---------------------------------------------------------------------------
// Chroma1-HD (silveroxides/Chroma1-HD-GGUF, QuantStack/Chroma1-HD-GGUF)
// ---------------------------------------------------------------------------

const CHROMA_SHARED: &[ModelFile] = &[
    ModelFile {
        repo: "silveroxides/Chroma1-HD-GGUF",
        file_name: "t5xxl_fp8_e4m3fn.safetensors",
        size_bytes: 4_900_000_000,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "silveroxides/Chroma1-HD-GGUF",
        file_name: "ae.safetensors",
        size_bytes: 340_000_000,
        role: FileRole::Vae,
    },
];

const CHROMA_QUANTS: &[QuantOption] = &[
    QuantOption {
        quant: Quant::Q4,
        label: "Q4_K_M",
        file_name: "Chroma1-HD-Q4_K_M.gguf",
        size_bytes: 5_570_000_000,
    },
    QuantOption {
        quant: Quant::Q5,
        label: "Q5_K_M",
        file_name: "Chroma1-HD-Q5_K_M.gguf",
        size_bytes: 6_650_000_000,
    },
    QuantOption {
        quant: Quant::Q6,
        label: "Q6_K",
        file_name: "Chroma1-HD-Q6_K.gguf",
        size_bytes: 7_650_000_000,
    },
    QuantOption {
        quant: Quant::Q8,
        label: "Q8_0",
        file_name: "Chroma1-HD-Q8_0.gguf",
        size_bytes: 9_740_000_000,
    },
    QuantOption {
        quant: Quant::Bf16,
        label: "BF16",
        file_name: "Chroma1-HD-BF16.gguf",
        size_bytes: 17_800_000_000,
    },
];

// ---------------------------------------------------------------------------
// FLUX.1 [dev] (city96/FLUX.1-dev-gguf) + Flux-Uncensored-V2 LoRA
// (enhanceaiteam/Flux-Uncensored-V2)
// ---------------------------------------------------------------------------

const FLUX1_SHARED: &[ModelFile] = &[
    ModelFile {
        repo: "city96/t5-v1_1-xxl-encoder-gguf",
        file_name: "t5-v1_1-xxl-encoder-Q5_K_M.gguf",
        size_bytes: 3_400_000_000,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "city96/t5-v1_1-xxl-encoder-gguf",
        file_name: "clip_l.safetensors",
        size_bytes: 240_000_000,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "city96/FLUX.1-dev-gguf",
        file_name: "ae.safetensors",
        size_bytes: 340_000_000,
        role: FileRole::Vae,
    },
    ModelFile {
        repo: "enhanceaiteam/Flux-Uncensored-V2",
        file_name: "lora.safetensors",
        size_bytes: 687_000_000,
        role: FileRole::BundledLora,
    },
];

const FLUX1_QUANTS: &[QuantOption] = &[
    QuantOption {
        quant: Quant::Q4,
        label: "Q4_K_S",
        file_name: "flux1-dev-Q4_K_S.gguf",
        size_bytes: 6_800_000_000,
    },
    QuantOption {
        quant: Quant::Q5,
        label: "Q5_K_S",
        file_name: "flux1-dev-Q5_K_S.gguf",
        size_bytes: 8_300_000_000,
    },
    QuantOption {
        quant: Quant::Q6,
        label: "Q6_K",
        file_name: "flux1-dev-Q6_K.gguf",
        size_bytes: 9_900_000_000,
    },
    QuantOption {
        quant: Quant::Q8,
        label: "Q8_0",
        file_name: "flux1-dev-Q8_0.gguf",
        size_bytes: 12_700_000_000,
    },
    QuantOption {
        quant: Quant::Bf16,
        label: "FP16",
        file_name: "flux1-dev-F16.gguf",
        size_bytes: 23_800_000_000,
    },
];

// ---------------------------------------------------------------------------
// FLUX.2 Klein 4B (unsloth/FLUX.2-klein-4B-GGUF) + abliterated Qwen3-4B
// text encoder (ponpoke/flux2-klein-4b-uncensored-text-encoder)
// ---------------------------------------------------------------------------

const KLEIN_SHARED: &[ModelFile] = &[
    ModelFile {
        repo: "ponpoke/flux2-klein-4b-uncensored-text-encoder",
        file_name: "qwen3-4b-abl-q4_k_m.gguf",
        size_bytes: 2_500_000_000,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "unsloth/FLUX.2-klein-4B-GGUF",
        file_name: "ae.safetensors",
        size_bytes: 340_000_000,
        role: FileRole::Vae,
    },
];

const KLEIN_QUANTS: &[QuantOption] = &[
    QuantOption {
        quant: Quant::Q4,
        label: "Q4_K_M",
        file_name: "flux-2-klein-4b-Q4_K_M.gguf",
        size_bytes: 2_600_000_000,
    },
    QuantOption {
        quant: Quant::Q8,
        label: "Q8_0",
        file_name: "flux-2-klein-4b-Q8_0.gguf",
        size_bytes: 5_200_000_000,
    },
];

/// The profile for a variant.
pub fn profile_for(variant: Variant) -> VariantProfile {
    match variant {
        Variant::ChromaHd => VariantProfile {
            variant,
            shared_files: CHROMA_SHARED,
            quant_options: CHROMA_QUANTS,
        },
        Variant::Flux1Dev => VariantProfile {
            variant,
            shared_files: FLUX1_SHARED,
            quant_options: FLUX1_QUANTS,
        },
        Variant::Flux2Klein4b => VariantProfile {
            variant,
            shared_files: KLEIN_SHARED,
            quant_options: KLEIN_QUANTS,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_a_profile_with_quants() {
        for v in Variant::ALL {
            let p = profile_for(v);
            assert_eq!(p.variant, v);
            assert!(!p.quant_options.is_empty());
            assert!(!p.shared_files.is_empty());
        }
    }

    #[test]
    fn quant_tables_are_smallest_first() {
        for v in Variant::ALL {
            let p = profile_for(v);
            let sizes: Vec<u64> = p.quant_options.iter().map(|o| o.size_bytes).collect();
            let mut sorted = sizes.clone();
            sorted.sort_unstable();
            assert_eq!(sizes, sorted, "quant table for {} is not ordered", v.id());
        }
    }

    #[test]
    fn chroma_q6_total_matches_component_sizes() {
        let p = profile_for(Variant::ChromaHd);
        // 7.65 GB model + 4.90 GB T5 FP8 + 0.34 GB VAE.
        assert_eq!(p.total_size_bytes(Quant::Q6), Some(12_890_000_000));
    }

    #[test]
    fn flux1_includes_the_bundled_lora() {
        let p = profile_for(Variant::Flux1Dev);
        assert!(p
            .shared_files
            .iter()
            .any(|f| f.role == FileRole::BundledLora && f.size_bytes == 687_000_000));
    }

    #[test]
    fn unknown_quant_for_klein_is_none() {
        let p = profile_for(Variant::Flux2Klein4b);
        assert!(p.quant_option(Quant::Q5).is_none());
        assert!(p.total_size_bytes(Quant::Q5).is_none());
    }
}
