//! Generation settings and their validation.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

/// Samplers exposed by the engine for Flux-family models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampler {
    /// Euler sampler.
    Euler,
    /// Euler ancestral sampler.
    EulerAncestral,
    /// Heun sampler.
    Heun,
    /// DPM++ 2M sampler.
    Dpmpp2m,
    /// LCM sampler, for few-step distilled runs.
    Lcm,
}

impl Sampler {
    /// Sampler name as passed to the engine command line.
    pub fn engine_name(self) -> &'static str {
        match self {
            Sampler::Euler => "euler",
            Sampler::EulerAncestral => "euler_a",
            Sampler::Heun => "heun",
            Sampler::Dpmpp2m => "dpm++2m",
            Sampler::Lcm => "lcm",
        }
    }
}

/// Smallest width or height the UI offers, in pixels.
pub const MIN_DIMENSION: u32 = 256;
/// Largest width or height the UI offers, in pixels.
pub const MAX_DIMENSION: u32 = 2048;
/// Dimensions are multiples of this value.
pub const DIMENSION_STEP: u32 = 16;
/// Minimum sampling steps.
pub const MIN_STEPS: u32 = 1;
/// Maximum sampling steps.
pub const MAX_STEPS: u32 = 150;

/// The complete set of values that define one generation.
///
/// This is the record embedded into output PNG files and stored in
/// the gallery database, together with the variant, quantization,
/// and LoRA stack recorded alongside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationSettings {
    /// Positive prompt, passed to the engine exactly as entered.
    pub prompt: String,
    /// Negative prompt; may be empty for models that do not use one.
    pub negative_prompt: String,
    /// Seed. `None` means a random seed is chosen at run time; the
    /// concrete seed used is recorded on the job and the output.
    pub seed: Option<u64>,
    /// Sampling steps.
    pub steps: u32,
    /// Guidance scale (CFG / distilled guidance as the model uses).
    pub guidance: f32,
    /// Sampler selection.
    pub sampler: Sampler,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Strength for image-to-image runs; `None` for text-to-image.
    pub denoise_strength: Option<f32>,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        GenerationSettings {
            prompt: String::new(),
            negative_prompt: String::new(),
            seed: None,
            steps: 20,
            guidance: 3.5,
            sampler: Sampler::Euler,
            width: 1024,
            height: 1024,
            denoise_strength: None,
        }
    }
}

impl GenerationSettings {
    /// Validate every field, returning the first problem found with
    /// the field name and the reason.
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            return Err(AppError::validation("prompt", "prompt must not be empty"));
        }
        if !(MIN_STEPS..=MAX_STEPS).contains(&self.steps) {
            return Err(AppError::validation(
                "steps",
                format!("steps must be between {MIN_STEPS} and {MAX_STEPS}"),
            ));
        }
        if !self.guidance.is_finite() || !(0.0..=30.0).contains(&self.guidance) {
            return Err(AppError::validation(
                "guidance",
                "guidance must be a finite number between 0 and 30",
            ));
        }
        self.validate_dimension("width", self.width)?;
        self.validate_dimension("height", self.height)?;
        if let Some(strength) = self.denoise_strength {
            if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
                return Err(AppError::validation(
                    "denoise_strength",
                    "denoise strength must be between 0 and 1",
                ));
            }
        }
        Ok(())
    }

    fn validate_dimension(&self, field: &str, value: u32) -> Result<()> {
        if !(MIN_DIMENSION..=MAX_DIMENSION).contains(&value) {
            return Err(AppError::validation(
                field,
                format!("{field} must be between {MIN_DIMENSION} and {MAX_DIMENSION}"),
            ));
        }
        if value % DIMENSION_STEP != 0 {
            return Err(AppError::validation(
                field,
                format!("{field} must be a multiple of {DIMENSION_STEP}"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> GenerationSettings {
        GenerationSettings {
            prompt: "a lighthouse at dusk".to_string(),
            ..GenerationSettings::default()
        }
    }

    #[test]
    fn defaults_are_valid() {
        assert!(valid().validate().is_ok());
    }

    #[test]
    fn empty_prompt_is_rejected() {
        let mut s = valid();
        s.prompt = "   ".to_string();
        let err = s.validate().err();
        assert!(matches!(err, Some(AppError::Validation { .. })));
    }

    #[test]
    fn steps_out_of_range_is_rejected() {
        let mut s = valid();
        s.steps = 0;
        assert!(s.validate().is_err());
        s.steps = MAX_STEPS + 1;
        assert!(s.validate().is_err());
    }

    #[test]
    fn non_multiple_dimension_is_rejected() {
        let mut s = valid();
        s.width = 1000;
        assert!(s.validate().is_err());
    }

    #[test]
    fn oversized_dimension_is_rejected() {
        let mut s = valid();
        s.height = MAX_DIMENSION + DIMENSION_STEP;
        assert!(s.validate().is_err());
    }

    #[test]
    fn denoise_strength_range_is_checked() {
        let mut s = valid();
        s.denoise_strength = Some(1.5);
        assert!(s.validate().is_err());
        s.denoise_strength = Some(0.75);
        assert!(s.validate().is_ok());
    }
}
