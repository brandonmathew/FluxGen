//! Quantization auto-selection.
//!
//! The rule: the diffusion model runs on the GPU; the text encoders
//! stream from disk and are freed after encoding, so they do not
//! consume VRAM during sampling. Selection therefore picks the
//! largest quantization whose model file fits in
//! `VRAM − reserve − VAE allowance`, where the reserve covers
//! sampling activations and grows for output sizes above
//! 1024×1024. When no GPU VRAM is
//! known, selection falls back to the smallest quantization, which
//! the engine streams from disk.

use fluxgen_core::{AppError, Quant, Variant};

use crate::profiles::{profile_for, VariantProfile};
use crate::system::SystemSpecs;

/// VRAM held back for sampling activations at up to 1024×1024:
/// 1.5 GB.
pub const VRAM_RESERVE_BYTES: u64 = 1_500_000_000;
/// VRAM allowance for the VAE decode stage, which runs on the GPU
/// after sampling with its own activation footprint: 1.0 GB.
pub const VAE_DECODE_ALLOWANCE_BYTES: u64 = 1_000_000_000;
/// Extra reserve per megapixel of output above 1024×1024.
pub const VRAM_RESERVE_PER_EXTRA_MEGAPIXEL: u64 = 250_000_000;

/// The outcome of quantization selection: the pick and the factual
/// reasoning behind it, shown in the Models page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantChoice {
    /// Selected quantization.
    pub quant: Quant,
    /// Publisher label for the selected quantization.
    pub label: String,
    /// Model file size in bytes at the selected quantization.
    pub model_size_bytes: u64,
    /// VRAM budget the selection was made against, in bytes.
    pub vram_budget_bytes: u64,
    /// Human-readable reasoning, one line per fact.
    pub reasons: Vec<String>,
}

/// Activation reserve for a given output size.
pub fn reserve_bytes(width: u32, height: u32) -> u64 {
    let pixels = u64::from(width) * u64::from(height);
    let base_pixels = 1024 * 1024;
    if pixels <= base_pixels {
        return VRAM_RESERVE_BYTES;
    }
    let extra_megapixels = (pixels - base_pixels) / (1024 * 1024);
    VRAM_RESERVE_BYTES + extra_megapixels * VRAM_RESERVE_PER_EXTRA_MEGAPIXEL
}

/// Pick the quantization for a variant on a machine.
///
/// Returns an error when the variant cannot fit at any
/// quantization within the disk budget, or when even the smallest
/// quantization exceeds free disk space.
pub fn pick_quant(
    variant: Variant,
    specs: &SystemSpecs,
    width: u32,
    height: u32,
) -> Result<QuantChoice, AppError> {
    let profile = profile_for(variant);
    pick_from_profile(&profile, specs, width, height)
}

fn pick_from_profile(
    profile: &VariantProfile,
    specs: &SystemSpecs,
    width: u32,
    height: u32,
) -> Result<QuantChoice, AppError> {
    let mut reasons = Vec::new();
    let reserve = reserve_bytes(width, height);
    let budget = match specs.vram_bytes {
        Some(vram) => {
            reasons.push(format!(
                "Detected {} GB of GPU VRAM; holding back {} GB for sampling activations at {}x{} and {} GB for VAE decode.",
                gb1(vram),
                gb1(reserve),
                width,
                height,
                gb1(VAE_DECODE_ALLOWANCE_BYTES)
            ));
            vram.saturating_sub(reserve + VAE_DECODE_ALLOWANCE_BYTES)
        }
        None => {
            reasons.push(
                "No GPU VRAM was detected; selecting the smallest quantization for disk streaming."
                    .to_string(),
            );
            0
        }
    };

    // Largest-first walk.
    let mut options: Vec<_> = profile.quant_options.iter().collect();
    options.sort_by_key(|o| std::cmp::Reverse(o.size_bytes));

    let chosen = if specs.vram_bytes.is_some() {
        options.iter().find(|o| o.size_bytes <= budget).copied()
    } else {
        options.last().copied()
    };

    let option = chosen.ok_or_else(|| {
        AppError::validation(
            "quant",
            format!(
                "no quantization of {} fits the VRAM budget of {} GB",
                profile.variant.display_name(),
                gb1(budget)
            ),
        )
    })?;

    let total = profile
        .total_size_bytes(option.quant)
        .unwrap_or(option.size_bytes);
    if total > specs.free_disk_bytes {
        return Err(AppError::validation(
            "quant",
            format!(
                "{} at {} needs {} GB of disk space but only {} GB is free",
                profile.variant.display_name(),
                option.label,
                gb1(total),
                gb1(specs.free_disk_bytes)
            ),
        ));
    }

    reasons.push(format!(
        "Selected {} ({} GB model file) for {}; total download {} GB.",
        option.label,
        gb1(option.size_bytes),
        profile.variant.display_name(),
        gb1(total)
    ));

    Ok(QuantChoice {
        quant: option.quant,
        label: option.label.to_string(),
        model_size_bytes: option.size_bytes,
        vram_budget_bytes: budget,
        reasons,
    })
}

/// Step down one quantization at a time until a level fits the
/// budget, used after a failed engine load test. Returns the first
/// fitting level at or below `start`, or `None` when even the
/// smallest level does not fit.
pub fn step_down_until_fit(
    variant: Variant,
    start: Quant,
    vram_budget_bytes: u64,
) -> Option<Quant> {
    let profile = profile_for(variant);
    let mut current = Some(start);
    while let Some(quant) = current {
        if let Some(option) = profile.quant_option(quant) {
            if option.size_bytes <= vram_budget_bytes {
                return Some(quant);
            }
        }
        current = quant.step_down();
    }
    None
}

/// Format bytes as GB with one decimal place.
fn gb1(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    /// The reference target machine: 12 GB VRAM, 16 GB RAM,
    /// 152 GB free disk.
    fn target_machine() -> SystemSpecs {
        SystemSpecs::new(
            Some(12 * GB),
            Some("Test GPU 12GB".to_string()),
            16 * GB,
            152 * GB,
        )
    }

    #[test]
    fn chroma_pick_on_target_machine_is_q6() {
        // Budget: 12 GB VRAM - 1.5 GB sampling reserve - 1.0 GB VAE
        // allowance = 9.5 GB. Q8_0 (9.74 GB) exceeds the budget;
        // Q6_K (7.65 GB) is the largest level that fits.
        let choice =
            pick_quant(Variant::ChromaHd, &target_machine(), 1024, 1024).expect("chroma pick");
        assert_eq!(choice.quant, Quant::Q6);
    }

    #[test]
    fn flux1_pick_on_target_machine_is_q5() {
        let choice =
            pick_quant(Variant::Flux1Dev, &target_machine(), 1024, 1024).expect("flux1 pick");
        assert_eq!(choice.quant, Quant::Q5);
    }

    #[test]
    fn klein_pick_on_target_machine_is_top_quant() {
        let choice =
            pick_quant(Variant::Flux2Klein4b, &target_machine(), 1024, 1024).expect("klein pick");
        assert_eq!(choice.quant, Quant::Q8);
    }

    #[test]
    fn no_vram_falls_back_to_smallest() {
        let specs = SystemSpecs::new(None, None, 16 * GB, 152 * GB);
        let choice = pick_quant(Variant::ChromaHd, &specs, 1024, 1024).expect("pick");
        assert_eq!(choice.quant, Quant::Q4);
    }

    #[test]
    fn insufficient_disk_is_an_error() {
        let specs = SystemSpecs::new(Some(12 * GB), None, 16 * GB, 5 * GB);
        assert!(pick_quant(Variant::ChromaHd, &specs, 1024, 1024).is_err());
    }

    #[test]
    fn larger_output_shrinks_the_pick() {
        let small = pick_quant(Variant::ChromaHd, &target_machine(), 1024, 1024).expect("1024");
        let large = pick_quant(Variant::ChromaHd, &target_machine(), 2048, 2048).expect("2048");
        assert!(large.quant <= small.quant);
    }

    #[test]
    fn step_down_walks_until_fit() {
        // Budget 8 GB: Q8 (9.74) and Q6 (7.65 fits) — Chroma.
        assert_eq!(
            step_down_until_fit(Variant::ChromaHd, Quant::Q8, 8 * GB),
            Some(Quant::Q6)
        );
        // Budget below the smallest Chroma quant (5.57 GB).
        assert_eq!(
            step_down_until_fit(Variant::ChromaHd, Quant::Q6, 5 * GB),
            None
        );
    }
}
