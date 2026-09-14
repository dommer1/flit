//! Experimental on-device summaries: which models the user may pick, where
//! their files live, and (later) the engine that runs them.
//!
//! PRIVACY: nothing here ever sends message content anywhere. The only
//! network use in this module is fetching a model file the user explicitly
//! chose and clicked to download — see CLAUDE.md's hard rules.

pub mod catalog;
pub mod download;
pub mod store;

use std::path::Path;

use sqlx::SqlitePool;
use tauri::AppHandle;

use crate::error::AppError;
use crate::models::{LlmModel, LlmModelState, LlmStatus};
use crate::storage::settings;

/// The current state of the feature, for Settings and the main window.
pub async fn status(app: &AppHandle, pool: &SqlitePool) -> Result<LlmStatus, AppError> {
    let enabled = settings::llm_summary_enabled(pool).await?;
    let active = settings::llm_model(pool).await?;
    Ok(describe(&store::models_dir(app)?, enabled, active))
}

/// Assemble the status from its inputs — kept free of app handles and the
/// database so it can be tested against a scratch directory.
fn describe(dir: &Path, enabled: bool, active_model: Option<String>) -> LlmStatus {
    let models: Vec<LlmModel> = catalog::MODELS
        .iter()
        .map(|spec| LlmModel {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            description: spec.description.to_string(),
            size: spec.size,
            recommended: spec.recommended,
            state: if store::is_ready(dir, spec) {
                LlmModelState::Ready
            } else {
                LlmModelState::Missing
            },
        })
        .collect();
    let ready = enabled
        && active_model
            .as_deref()
            .and_then(catalog::find)
            .is_some_and(|spec| store::is_ready(dir, spec));
    LlmStatus {
        enabled,
        active_model,
        ready,
        models,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("flit-llm-status-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Fake a complete download: a file of exactly the pinned size.
    fn fake_download(dir: &Path, id: &str) {
        let spec = catalog::find(id).unwrap();
        let file = std::fs::File::create(store::model_path(dir, spec)).unwrap();
        file.set_len(spec.size).unwrap();
    }

    #[test]
    fn lists_every_catalog_model_as_missing_on_a_fresh_machine() {
        let status = describe(&scratch_dir("fresh"), true, None);

        assert_eq!(status.models.len(), catalog::MODELS.len());
        assert!(status
            .models
            .iter()
            .all(|m| m.state == LlmModelState::Missing));
        assert!(!status.ready);
    }

    #[test]
    fn ready_needs_the_switch_and_the_picked_models_file() {
        let dir = scratch_dir("ready");
        fake_download(&dir, "qwen3.5-2b");

        // File present but the feature is off.
        assert!(!describe(&dir, false, Some("qwen3.5-2b".into())).ready);
        // On, but nothing picked.
        assert!(!describe(&dir, true, None).ready);
        // On, but the pick is a model that is not downloaded.
        assert!(!describe(&dir, true, Some("qwen3.5-4b".into())).ready);

        let status = describe(&dir, true, Some("qwen3.5-2b".into()));
        assert!(status.ready);
        assert_eq!(
            status
                .models
                .iter()
                .find(|m| m.id == "qwen3.5-2b")
                .map(|m| m.state.clone()),
            Some(LlmModelState::Ready)
        );
    }
}
