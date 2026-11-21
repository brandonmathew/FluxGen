//! GGUF quantization levels.
//!
//! The ordering is by on-disk size (and therefore memory use),
//! smallest first. Auto-selection walks this order to find the
//! largest quantization that fits a memory budget; step-down moves
//! one position toward the start of the order.

use serde::{Deserialize, Serialize};

/// A GGUF quantization level.
///
/// The declaration order defines the size order used by
/// quantization selection and step-down logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quant {
    /// ~2-bit class quants (IQ2 / Q2_K family).
    Q2,
    /// ~3-bit class quants (IQ3 / Q3_K family).
    Q3,
    /// ~4-bit class quants (Q4_K_S / Q4_K_M family).
    Q4,
    /// ~5-bit class quants (Q5_K_S / Q5_K_M family).
    Q5,
    /// ~6-bit class quants (Q6_K family).
    Q6,
    /// 8-bit quant (Q8_0 family).
    Q8,
    /// Full precision (BF16/F16) weights.
    Bf16,
}

impl Quant {
    /// File-name fragment used by GGUF releases for this level, in
    /// the most common naming form. Profiles list the exact
    /// per-variant labels; this is the canonical short label.
    pub fn label(self) -> &'static str {
        match self {
            Quant::Q2 => "Q2",
            Quant::Q3 => "Q3",
            Quant::Q4 => "Q4",
            Quant::Q5 => "Q5",
            Quant::Q6 => "Q6",
            Quant::Q8 => "Q8_0",
            Quant::Bf16 => "BF16",
        }
    }

    /// The next smaller quantization, or `None` at the bottom.
    pub fn step_down(self) -> Option<Quant> {
        match self {
            Quant::Q2 => None,
            Quant::Q3 => Some(Quant::Q2),
            Quant::Q4 => Some(Quant::Q3),
            Quant::Q5 => Some(Quant::Q4),
            Quant::Q6 => Some(Quant::Q5),
            Quant::Q8 => Some(Quant::Q6),
            Quant::Bf16 => Some(Quant::Q8),
        }
    }

    /// The next larger quantization, or `None` at the top.
    pub fn step_up(self) -> Option<Quant> {
        match self {
            Quant::Q2 => Some(Quant::Q3),
            Quant::Q3 => Some(Quant::Q4),
            Quant::Q4 => Some(Quant::Q5),
            Quant::Q5 => Some(Quant::Q6),
            Quant::Q6 => Some(Quant::Q8),
            Quant::Q8 => Some(Quant::Bf16),
            Quant::Bf16 => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_by_size() {
        assert!(Quant::Q2 < Quant::Q3);
        assert!(Quant::Q3 < Quant::Q4);
        assert!(Quant::Q4 < Quant::Q5);
        assert!(Quant::Q5 < Quant::Q6);
        assert!(Quant::Q6 < Quant::Q8);
        assert!(Quant::Q8 < Quant::Bf16);
    }

    #[test]
    fn step_down_walks_the_order() {
        assert_eq!(Quant::Q6.step_down(), Some(Quant::Q5));
        assert_eq!(Quant::Bf16.step_down(), Some(Quant::Q8));
        assert_eq!(Quant::Q2.step_down(), None);
    }

    #[test]
    fn step_up_is_the_inverse() {
        let mut q = Quant::Q2;
        while let Some(next) = q.step_up() {
            assert_eq!(next.step_down(), Some(q));
            q = next;
        }
        assert_eq!(q, Quant::Bf16);
    }
}
