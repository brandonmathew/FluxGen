//! Saveable generation presets.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::{GenerationSettings, LoraEntry, Variant};

/// A named bundle that re-applies a full generation setup in one
/// action: variant, settings, and the LoRA stack with weights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// Preset name, unique in the preset list.
    pub name: String,
    /// Variant the preset generates with.
    pub variant: Variant,
    /// Generation settings captured in the preset.
    pub settings: GenerationSettings,
    /// LoRA stack captured in the preset, in application order.
    pub loras: Vec<LoraEntry>,
}

impl Preset {
    /// Validate the preset name, its settings, and every LoRA.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::validation("preset name", "name must not be empty"));
        }
        self.settings.validate()?;
        for lora in &self.loras {
            lora.validate()?;
            if lora.variant != self.variant {
                return Err(AppError::validation(
                    "preset loras",
                    format!(
                        "LoRA '{}' belongs to {} but the preset uses {}",
                        lora.name,
                        lora.variant.display_name(),
                        self.variant.display_name()
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn preset() -> Preset {
        let mut settings = GenerationSettings::default();
        settings.prompt = "portrait".to_string();
        Preset {
            name: "Portrait".to_string(),
            variant: Variant::ChromaHd,
            settings,
            loras: vec![LoraEntry::new(
                "skin",
                PathBuf::from("skin.safetensors"),
                Variant::ChromaHd,
            )],
        }
    }

    #[test]
    fn matching_variant_lora_is_valid() {
        assert!(preset().validate().is_ok());
    }

    #[test]
    fn cross_variant_lora_is_rejected() {
        let mut p = preset();
        p.loras[0].variant = Variant::Flux1Dev;
        assert!(p.validate().is_err());
    }
}
