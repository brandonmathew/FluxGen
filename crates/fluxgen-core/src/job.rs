//! Generation job model for the batch queue.

use serde::{Deserialize, Serialize};

use crate::{GenerationSettings, LoraEntry, Quant, Variant};

/// Identifier of a queued or running generation job.
pub type JobId = u64;

/// Lifecycle state of a generation job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum JobStatus {
    /// Waiting in the queue.
    Queued,
    /// Currently generating; `progress` is a fraction in 0.0–1.0
    /// when the engine reports step progress.
    Running {
        /// Progress fraction, if known.
        progress: Option<f32>,
    },
    /// Finished; holds the output image paths produced.
    Completed {
        /// Output image paths, in generation order.
        outputs: Vec<std::path::PathBuf>,
    },
    /// Failed; holds the full failure reason, including captured
    /// engine output when the engine was involved. Failed jobs stay
    /// in the queue list until dismissed — they never disappear
    /// silently.
    Failed {
        /// Full failure reason shown to the user.
        reason: String,
    },
    /// Cancelled by the user before completion.
    Cancelled,
}

impl JobStatus {
    /// Whether the job has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed { .. } | JobStatus::Failed { .. } | JobStatus::Cancelled
        )
    }
}

/// One generation request in the queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Queue-unique identifier.
    pub id: JobId,
    /// Variant to generate with.
    pub variant: Variant,
    /// Quantization the variant is loaded at.
    pub quant: Quant,
    /// Settings for this job.
    pub settings: GenerationSettings,
    /// LoRA stack for this job, in application order.
    pub loras: Vec<LoraEntry>,
    /// Source image for image-to-image jobs; `None` for
    /// text-to-image.
    pub init_image: Option<std::path::PathBuf>,
    /// Current lifecycle state.
    pub status: JobStatus,
}

impl Job {
    /// Create a queued job.
    pub fn new(
        id: JobId,
        variant: Variant,
        quant: Quant,
        settings: GenerationSettings,
        loras: Vec<LoraEntry>,
        init_image: Option<std::path::PathBuf>,
    ) -> Self {
        Job {
            id,
            variant,
            quant,
            settings,
            loras,
            init_image,
            status: JobStatus::Queued,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_job_starts_queued() {
        let job = Job::new(
            1,
            Variant::Flux2Klein4b,
            Quant::Q8,
            GenerationSettings::default(),
            Vec::new(),
            None,
        );
        assert_eq!(job.status, JobStatus::Queued);
        assert!(!job.status.is_terminal());
    }

    #[test]
    fn terminal_states_are_detected() {
        assert!(JobStatus::Cancelled.is_terminal());
        assert!(JobStatus::Failed {
            reason: "engine exited".to_string()
        }
        .is_terminal());
        assert!(!JobStatus::Running { progress: None }.is_terminal());
    }
}
