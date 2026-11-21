//! LoRA library entries.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::Variant;

/// One LoRA file in the application library, as applied to a
/// generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoraEntry {
    /// Display name (file stem at import time; user-editable).
    pub name: String,
    /// Path of the LoRA file inside the portable LoRA library.
    pub path: PathBuf,
    /// Variant this LoRA belongs to. A LoRA only applies to its own
    /// variant.
    pub variant: Variant,
    /// Application weight. Typical range 0.0–2.0; 1.0 is the trained
    /// strength.
    pub weight: f32,
    /// Whether the LoRA participates in the next generation.
    pub enabled: bool,
}

impl LoraEntry {
    /// Create an enabled entry at full weight.
    pub fn new(name: impl Into<String>, path: PathBuf, variant: Variant) -> Self {
        LoraEntry {
            name: name.into(),
            path,
            variant,
            weight: 1.0,
            enabled: true,
        }
    }

    /// Validate the weight and name.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::validation("lora name", "name must not be empty"));
        }
        if !self.weight.is_finite() || !(0.0..=2.0).contains(&self.weight) {
            return Err(AppError::validation(
                "lora weight",
                "weight must be a finite number between 0 and 2",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_entry_is_valid() {
        let e = LoraEntry::new("detail", PathBuf::from("detail.safetensors"), Variant::ChromaHd);
        assert!(e.validate().is_ok());
        assert!(e.enabled);
        assert_eq!(e.weight, 1.0);
    }

    #[test]
    fn out_of_range_weight_is_rejected() {
        let mut e = LoraEntry::new("x", PathBuf::from("x.safetensors"), Variant::Flux1Dev);
        e.weight = 2.5;
        assert!(e.validate().is_err());
        e.weight = f32::NAN;
        assert!(e.validate().is_err());
    }
}
