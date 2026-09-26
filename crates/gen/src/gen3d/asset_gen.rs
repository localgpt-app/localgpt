//! AI-powered 3D asset generation infrastructure (AI1).
//!
//! Tracks generation tasks for text/image → 3D mesh and PBR texture
//! generation. The HTTP work happens in [`super::model_server`]; this is the
//! state `gen_generation_status` reports.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported 3D generation models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationModel {
    #[serde(rename = "tripo_sg")]
    TripoSG,
    Hunyuan3d,
    Hunyuan3dMini,
    Step1x,
}

impl GenerationModel {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::TripoSG => "TripoSG",
            Self::Hunyuan3d => "Hunyuan3D 2.1",
            Self::Hunyuan3dMini => "Hunyuan3D 2mini",
            Self::Step1x => "Step1X-3D",
        }
    }

    pub fn estimated_vram_gb(&self) -> f32 {
        match self {
            Self::TripoSG => 8.0,
            Self::Hunyuan3d => 10.0,
            Self::Hunyuan3dMini => 5.5,
            Self::Step1x => 16.0,
        }
    }

    pub fn estimated_seconds(&self, quality: GenerationQuality) -> u32 {
        let base = match self {
            Self::TripoSG => 30,
            Self::Hunyuan3d => 60,
            Self::Hunyuan3dMini => 45,
            Self::Step1x => 90,
        };
        match quality {
            GenerationQuality::Draft => base / 2,
            GenerationQuality::Standard => base,
            GenerationQuality::High => base * 2,
        }
    }
}

/// Quality presets for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationQuality {
    Draft,
    Standard,
    High,
}

/// Texture style presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureStyle {
    Realistic,
    Stylized,
    PixelArt,
    HandPainted,
    Toon,
}

/// State of a generation task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationTaskState {
    Queued,
    Generating,
    Loading,
    Complete,
    Failed,
    Cancelled,
}

/// A tracked generation task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationTask {
    pub task_id: String,
    pub task_type: GenerationTaskType,
    pub prompt: String,
    pub model: GenerationModel,
    pub quality: GenerationQuality,
    pub state: GenerationTaskState,
    pub entity_name: String,
    pub position: [f32; 3],
    pub scale: f32,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub created_at: f64,
    pub elapsed_seconds: f32,
    pub estimated_seconds: u32,
}

/// Type of generation task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationTaskType {
    Mesh,
    Texture,
}

/// Bevy resource tracking generation tasks.
#[derive(Resource, Default)]
pub struct AssetGenManager {
    pub tasks: HashMap<String, GenerationTask>,
    next_task_id: u32,
}

impl AssetGenManager {
    /// Create a new task and return its ID.
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &mut self,
        task_type: GenerationTaskType,
        prompt: String,
        entity_name: String,
        model: GenerationModel,
        quality: GenerationQuality,
        position: [f32; 3],
        scale: f32,
    ) -> String {
        self.next_task_id += 1;
        let task_id = format!("gen_{:06x}", self.next_task_id);
        let estimated = model.estimated_seconds(quality);
        let task = GenerationTask {
            task_id: task_id.clone(),
            task_type,
            prompt,
            model,
            quality,
            state: GenerationTaskState::Queued,
            entity_name,
            position,
            scale,
            output_path: None,
            error: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or_default(),
            elapsed_seconds: 0.0,
            estimated_seconds: estimated,
        };
        self.tasks.insert(task_id.clone(), task);
        task_id
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<&GenerationTask> {
        self.tasks.get(task_id)
    }

    /// Cancel a task if it's still queued or generating.
    pub fn cancel_task(&mut self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            match task.state {
                GenerationTaskState::Queued | GenerationTaskState::Generating => {
                    task.state = GenerationTaskState::Cancelled;
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Get all active (queued or generating) tasks.
    pub fn active_tasks(&self) -> Vec<&GenerationTask> {
        self.tasks
            .values()
            .filter(|t| {
                matches!(
                    t.state,
                    GenerationTaskState::Queued | GenerationTaskState::Generating
                )
            })
            .collect()
    }

    /// Get all completed tasks.
    pub fn completed_tasks(&self) -> Vec<&GenerationTask> {
        self.tasks
            .values()
            .filter(|t| matches!(t.state, GenerationTaskState::Complete))
            .collect()
    }

    /// Get all failed tasks.
    pub fn failed_tasks(&self) -> Vec<&GenerationTask> {
        self.tasks
            .values()
            .filter(|t| matches!(t.state, GenerationTaskState::Failed))
            .collect()
    }

    /// Clean up old completed/failed tasks (older than max_age_seconds).
    pub fn cleanup_old_tasks(&mut self, max_age_seconds: f32) {
        self.tasks.retain(|_, t| match t.state {
            GenerationTaskState::Complete
            | GenerationTaskState::Failed
            | GenerationTaskState::Cancelled => t.elapsed_seconds < max_age_seconds,
            _ => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_task() {
        let mut mgr = AssetGenManager::default();
        let id = mgr.create_task(
            GenerationTaskType::Mesh,
            "a wooden chair".into(),
            "chair_01".into(),
            GenerationModel::TripoSG,
            GenerationQuality::Standard,
            [1.0, 0.0, 2.0],
            1.0,
        );
        assert!(id.starts_with("gen_"));
        let task = mgr.get_task(&id).unwrap();
        assert_eq!(task.prompt, "a wooden chair");
        assert_eq!(task.entity_name, "chair_01");
        assert_eq!(task.state, GenerationTaskState::Queued);
        assert_eq!(task.model, GenerationModel::TripoSG);
        assert_eq!(task.quality, GenerationQuality::Standard);
        assert_eq!(task.position, [1.0, 0.0, 2.0]);
        assert_eq!(task.scale, 1.0);
        assert_eq!(task.estimated_seconds, 30);
    }

    #[test]
    fn test_cancel_task() {
        let mut mgr = AssetGenManager::default();
        let id = mgr.create_task(
            GenerationTaskType::Mesh,
            "a tree".into(),
            "tree_01".into(),
            GenerationModel::Hunyuan3d,
            GenerationQuality::Draft,
            [0.0, 0.0, 0.0],
            1.5,
        );
        // Cancel a queued task succeeds
        assert!(mgr.cancel_task(&id));
        assert_eq!(
            mgr.get_task(&id).unwrap().state,
            GenerationTaskState::Cancelled
        );

        // Cancelling again fails (already cancelled)
        assert!(!mgr.cancel_task(&id));

        // Cancelling a nonexistent task fails
        assert!(!mgr.cancel_task("gen_nonexistent"));
    }

    #[test]
    fn test_active_tasks() {
        let mut mgr = AssetGenManager::default();
        let id1 = mgr.create_task(
            GenerationTaskType::Mesh,
            "rock".into(),
            "rock_01".into(),
            GenerationModel::TripoSG,
            GenerationQuality::Draft,
            [0.0, 0.0, 0.0],
            1.0,
        );
        let _id2 = mgr.create_task(
            GenerationTaskType::Texture,
            "wood texture".into(),
            "plank_01".into(),
            GenerationModel::Hunyuan3dMini,
            GenerationQuality::High,
            [0.0, 0.0, 0.0],
            1.0,
        );
        assert_eq!(mgr.active_tasks().len(), 2);

        // Cancel one — active count drops
        mgr.cancel_task(&id1);
        assert_eq!(mgr.active_tasks().len(), 1);
    }

    #[test]
    fn test_completed_tasks() {
        let mut mgr = AssetGenManager::default();
        let id = mgr.create_task(
            GenerationTaskType::Mesh,
            "sword".into(),
            "sword_01".into(),
            GenerationModel::Step1x,
            GenerationQuality::Standard,
            [0.0, 1.0, 0.0],
            0.5,
        );
        assert_eq!(mgr.completed_tasks().len(), 0);

        // Manually mark complete (simulating server callback)
        mgr.tasks.get_mut(&id).unwrap().state = GenerationTaskState::Complete;
        assert_eq!(mgr.completed_tasks().len(), 1);
    }

    #[test]
    fn test_cleanup() {
        let mut mgr = AssetGenManager::default();
        let id1 = mgr.create_task(
            GenerationTaskType::Mesh,
            "old".into(),
            "old_01".into(),
            GenerationModel::TripoSG,
            GenerationQuality::Draft,
            [0.0, 0.0, 0.0],
            1.0,
        );
        let id2 = mgr.create_task(
            GenerationTaskType::Mesh,
            "new".into(),
            "new_01".into(),
            GenerationModel::TripoSG,
            GenerationQuality::Draft,
            [0.0, 0.0, 0.0],
            1.0,
        );
        let id3 = mgr.create_task(
            GenerationTaskType::Mesh,
            "active".into(),
            "active_01".into(),
            GenerationModel::TripoSG,
            GenerationQuality::Draft,
            [0.0, 0.0, 0.0],
            1.0,
        );

        // Mark id1 as completed with high elapsed (should be cleaned)
        mgr.tasks.get_mut(&id1).unwrap().state = GenerationTaskState::Complete;
        mgr.tasks.get_mut(&id1).unwrap().elapsed_seconds = 200.0;

        // Mark id2 as failed with low elapsed (should survive)
        mgr.tasks.get_mut(&id2).unwrap().state = GenerationTaskState::Failed;
        mgr.tasks.get_mut(&id2).unwrap().elapsed_seconds = 10.0;

        // id3 stays queued (should survive regardless of elapsed)

        mgr.cleanup_old_tasks(100.0);
        assert!(mgr.get_task(&id1).is_none()); // cleaned
        assert!(mgr.get_task(&id2).is_some()); // survived
        assert!(mgr.get_task(&id3).is_some()); // active, always kept
    }

    #[test]
    fn test_model_estimates() {
        // TripoSG
        assert_eq!(
            GenerationModel::TripoSG.estimated_seconds(GenerationQuality::Draft),
            15
        );
        assert_eq!(
            GenerationModel::TripoSG.estimated_seconds(GenerationQuality::Standard),
            30
        );
        assert_eq!(
            GenerationModel::TripoSG.estimated_seconds(GenerationQuality::High),
            60
        );

        // Hunyuan3D
        assert_eq!(
            GenerationModel::Hunyuan3d.estimated_seconds(GenerationQuality::Draft),
            30
        );
        assert_eq!(
            GenerationModel::Hunyuan3d.estimated_seconds(GenerationQuality::Standard),
            60
        );
        assert_eq!(
            GenerationModel::Hunyuan3d.estimated_seconds(GenerationQuality::High),
            120
        );

        // Step1X
        assert_eq!(
            GenerationModel::Step1x.estimated_seconds(GenerationQuality::Draft),
            45
        );
        assert_eq!(
            GenerationModel::Step1x.estimated_seconds(GenerationQuality::Standard),
            90
        );
        assert_eq!(
            GenerationModel::Step1x.estimated_seconds(GenerationQuality::High),
            180
        );

        // VRAM estimates
        assert_eq!(GenerationModel::TripoSG.estimated_vram_gb(), 8.0);
        assert_eq!(GenerationModel::Hunyuan3dMini.estimated_vram_gb(), 5.5);
        assert_eq!(GenerationModel::Step1x.estimated_vram_gb(), 16.0);

        // Display names
        assert_eq!(GenerationModel::TripoSG.display_name(), "TripoSG");
        assert_eq!(GenerationModel::Hunyuan3d.display_name(), "Hunyuan3D 2.1");
        assert_eq!(
            GenerationModel::Hunyuan3dMini.display_name(),
            "Hunyuan3D 2mini"
        );
        assert_eq!(GenerationModel::Step1x.display_name(), "Step1X-3D");
    }
}
