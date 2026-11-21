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

// The shared files do not live in the GGUF repository (Issue #11):
// the T5 encoder is the standard FLUX text-encoder file, and the VAE
// comes from the variant's original Diffusers repository.
const CHROMA_SHARED: &[ModelFile] = &[
    ModelFile {
        repo: "comfyanonymous/flux_text_encoders",
        file_name: "t5xxl_fp8_e4m3fn.safetensors",
        size_bytes: 4_893_934_904,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "lodestones/Chroma1-HD",
        file_name: "vae/diffusion_pytorch_model.safetensors",
        size_bytes: 167_666_902,
        role: FileRole::Vae,
    },
];

const CHROMA_QUANTS: &[QuantOption] = &[
    QuantOption {
        quant: Quant::Q4,
        label: "Q4_K_M",
        file_name: "Chroma1-HD-Q4_K_M.gguf",
        size_bytes: 5_566_533_792,
    },
    QuantOption {
        quant: Quant::Q5,
        label: "Q5_K_M",
        file_name: "Chroma1-HD-Q5_K_M.gguf",
        size_bytes: 6_650_777_760,
    },
    QuantOption {
        quant: Quant::Q6,
        label: "Q6_K",
        file_name: "Chroma1-HD-Q6_K.gguf",
        size_bytes: 7_650_971_808,
    },
    QuantOption {
        quant: Quant::Q8,
        label: "Q8_0",
        file_name: "Chroma1-HD-Q8_0.gguf",
        size_bytes: 9_735_409_824,
    },
    QuantOption {
        quant: Quant::Bf16,
        label: "BF16",
        file_name: "Chroma1-HD-BF16.gguf",
        size_bytes: 17_804_202_144,
    },
];

// ---------------------------------------------------------------------------
// FLUX.1 [dev] (city96/FLUX.1-dev-gguf) + Flux-Uncensored-V2 LoRA
// (enhanceaiteam/Flux-Uncensored-V2)
// ---------------------------------------------------------------------------

// CLIP-L and the VAE do not live in the GGUF repositories
// (Issue #11): CLIP-L is the standard FLUX text-encoder file and
// the VAE is the FLUX.1 autoencoder in BF16.
const FLUX1_SHARED: &[ModelFile] = &[
    ModelFile {
        repo: "city96/t5-v1_1-xxl-encoder-gguf",
        file_name: "t5-v1_1-xxl-encoder-Q5_K_M.gguf",
        size_bytes: 3_386_856_640,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "comfyanonymous/flux_text_encoders",
        file_name: "clip_l.safetensors",
        size_bytes: 246_144_152,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "Kijai/flux-fp8",
        file_name: "flux-vae-bf16.safetensors",
        size_bytes: 167_664_710,
        role: FileRole::Vae,
    },
    ModelFile {
        repo: "enhanceaiteam/Flux-Uncensored-V2",
        file_name: "lora.safetensors",
        size_bytes: 687_476_088,
        role: FileRole::BundledLora,
    },
];

const FLUX1_QUANTS: &[QuantOption] = &[
    QuantOption {
        quant: Quant::Q4,
        label: "Q4_K_S",
        file_name: "flux1-dev-Q4_K_S.gguf",
        size_bytes: 6_805_988_640,
    },
    QuantOption {
        quant: Quant::Q5,
        label: "Q5_K_S",
        file_name: "flux1-dev-Q5_K_S.gguf",
        size_bytes: 8_285_267_232,
    },
    QuantOption {
        quant: Quant::Q6,
        label: "Q6_K",
        file_name: "flux1-dev-Q6_K.gguf",
        size_bytes: 9_857_000_736,
    },
    QuantOption {
        quant: Quant::Q8,
        label: "Q8_0",
        file_name: "flux1-dev-Q8_0.gguf",
        size_bytes: 12_708_281_504,
    },
    QuantOption {
        quant: Quant::Bf16,
        label: "FP16",
        file_name: "flux1-dev-F16.gguf",
        size_bytes: 23_802_870_944,
    },
];

// ---------------------------------------------------------------------------
// FLUX.2 Klein 4B (unsloth/FLUX.2-klein-4B-GGUF) + abliterated Qwen3-4B
// text encoder + FLUX.2 VAE (black-forest-labs/FLUX.2-klein-4B).
//
// The encoder's original repository
// (ponpoke/flux2-klein-4b-uncensored-text-encoder) is gated: anonymous
// downloads return HTTP 401, and FluxGen downloads anonymously. The
// mirror below serves the identical file anonymously; its SHA-256
// (97dac4e931ffb1fed023c564aa2d73b1de82795612548369810478c2d69a3e49)
// matches the original repository's listing exactly (Issue #11).
// ---------------------------------------------------------------------------

const KLEIN_SHARED: &[ModelFile] = &[
    ModelFile {
        repo: "nerualdreming/flux2-klein-4b-uncensored-text-encoder",
        file_name: "flux2-klein-4b-uncensored-q4_k_m.gguf",
        size_bytes: 2_497_280_416,
        role: FileRole::TextEncoder,
    },
    ModelFile {
        repo: "black-forest-labs/FLUX.2-klein-4B",
        file_name: "vae/diffusion_pytorch_model.safetensors",
        size_bytes: 168_120_878,
        role: FileRole::Vae,
    },
];

const KLEIN_QUANTS: &[QuantOption] = &[
    QuantOption {
        quant: Quant::Q4,
        label: "Q4_K_M",
        file_name: "flux-2-klein-4b-Q4_K_M.gguf",
        size_bytes: 2_604_311_104,
    },
    QuantOption {
        quant: Quant::Q8,
        label: "Q8_0",
        file_name: "flux-2-klein-4b-Q8_0.gguf",
        size_bytes: 4_300_644_928,
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
        // 7,650,971,808 model + 4,893,934,904 T5 FP8 + 167,666,902 VAE.
        assert_eq!(p.total_size_bytes(Quant::Q6), Some(12_712_573_614));
    }

    #[test]
    fn flux1_includes_the_bundled_lora() {
        let p = profile_for(Variant::Flux1Dev);
        assert!(p
            .shared_files
            .iter()
            .any(|f| f.role == FileRole::BundledLora && f.size_bytes == 687_476_088));
    }

    #[test]
    fn unknown_quant_for_klein_is_none() {
        let p = profile_for(Variant::Flux2Klein4b);
        assert!(p.quant_option(Quant::Q5).is_none());
        assert!(p.total_size_bytes(Quant::Q5).is_none());
    }
}
