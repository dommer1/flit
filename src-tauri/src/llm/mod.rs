//! Experimental on-device summaries: which models the user may pick, where
//! their files live, and (later) the engine that runs them.
//!
//! PRIVACY: nothing here ever sends message content anywhere. The only
//! network use in this module is fetching a model file the user explicitly
//! chose and clicked to download — see CLAUDE.md's hard rules.

pub mod catalog;
pub mod download;
pub mod engine;
pub mod store;
pub mod summarize;

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::AppError;
use crate::models::{LlmDownloadProgress, LlmModel, LlmModelState, LlmStatus, MessageHeader};
use crate::state::AppState;
use crate::storage::{self, settings};

/// Progress events are throttled to this — the UI needs a moving bar, not
/// one event per network chunk.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// The current state of the feature, for Settings and the main window.
pub async fn status(app: &AppHandle, pool: &SqlitePool) -> Result<LlmStatus, AppError> {
    let enabled = settings::llm_summary_enabled(pool).await?;
    let active = settings::llm_model(pool).await?;
    let downloads = app.state::<AppState>().llm_downloads.snapshot();
    Ok(describe(
        &store::models_dir(app)?,
        enabled,
        active,
        &downloads,
    ))
}

/// Assemble the status from its inputs — kept free of app handles and the
/// database so it can be tested against a scratch directory.
fn describe(
    dir: &Path,
    enabled: bool,
    active_model: Option<String>,
    downloads: &HashMap<String, download::Snapshot>,
) -> LlmStatus {
    let models: Vec<LlmModel> = catalog::MODELS
        .iter()
        .map(|spec| LlmModel {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            description: spec.description.to_string(),
            size: spec.size,
            recommended: spec.recommended,
            state: match downloads.get(spec.id) {
                Some(download::Snapshot {
                    error: Some(error), ..
                }) => LlmModelState::Failed {
                    error: error.clone(),
                },
                Some(snapshot) => LlmModelState::Downloading {
                    received: snapshot.received,
                    total: snapshot.total,
                },
                None if store::is_ready(dir, spec) => LlmModelState::Ready,
                None => LlmModelState::Missing,
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

/// The picked model, only when the feature is on and its file is here;
/// otherwise an error that reads as guidance for the user.
async fn ready_model(
    app: &AppHandle,
    pool: &SqlitePool,
) -> Result<(&'static catalog::ModelSpec, std::path::PathBuf), AppError> {
    if !settings::llm_summary_enabled(pool).await? {
        return Err(AppError::Invalid(
            "Summaries are switched off — see Settings → Experimental.".to_string(),
        ));
    }
    let spec = settings::llm_model(pool)
        .await?
        .as_deref()
        .and_then(catalog::find)
        .ok_or_else(|| {
            AppError::Invalid("Pick and download a model in Settings → Experimental.".to_string())
        })?;
    let dir = store::models_dir(app)?;
    if !store::is_ready(&dir, spec) {
        return Err(AppError::Invalid(format!(
            "{} is not downloaded — see Settings → Experimental.",
            spec.name
        )));
    }
    Ok((spec, store::model_path(&dir, spec)))
}

/// Summarize one message with the picked model. `body_text` is the raw
/// stored text; the prompt builder trims it. A cached summary of the same
/// inputs is returned unless `fresh` asks for a new one.
pub async fn summarize_message(
    app: &AppHandle,
    header: &MessageHeader,
    body_text: &str,
    fresh: bool,
) -> Result<String, AppError> {
    let state = app.state::<AppState>();
    let (spec, model_path) = ready_model(app, &state.pool).await?;
    if summarize::prepare_text(body_text, summarize::MESSAGE_CHARS).is_empty() {
        return Err(AppError::Invalid(
            "This message has no text to summarize.".to_string(),
        ));
    }
    let language = settings::llm_summary_language(&state.pool).await?;
    let id = header.id.to_string();
    let key = summarize::cache_key("message", spec.id, &language, [id.as_str(), body_text]);
    if !fresh {
        if let Some(cached) = storage::summaries::get(&state.pool, &key).await? {
            return Ok(cached);
        }
    }
    let source = summarize::Source {
        from: header.from.clone(),
        date: header.date.clone(),
        subject: header.subject.clone(),
        text: body_text.to_string(),
    };
    let text = state
        .llm_engine
        .complete(engine::Request {
            model_path,
            messages: summarize::message_prompt(&source, &language),
            max_tokens: summarize::MAX_ANSWER_TOKENS,
            assistant_prefix: spec.assistant_prefix.to_string(),
        })
        .await?;
    storage::summaries::put(&state.pool, &key, &text).await?;
    Ok(text)
}

/// Summarize a whole conversation, `messages` oldest first as
/// `(header, stored body text)`. Cached like `summarize_message`; a
/// conversation that gained a message hashes differently and is redone.
pub async fn summarize_thread(
    app: &AppHandle,
    messages: &[(MessageHeader, String)],
    fresh: bool,
) -> Result<String, AppError> {
    let state = app.state::<AppState>();
    let (spec, model_path) = ready_model(app, &state.pool).await?;
    let sources: Vec<summarize::Source> = messages
        .iter()
        .filter(|(_, text)| !summarize::prepare_text(text, summarize::MESSAGE_CHARS).is_empty())
        .map(|(header, text)| summarize::Source {
            from: header.from.clone(),
            date: header.date.clone(),
            subject: header.subject.clone(),
            text: text.clone(),
        })
        .collect();
    if sources.is_empty() {
        return Err(AppError::Invalid(
            "This conversation has no text to summarize.".to_string(),
        ));
    }
    let language = settings::llm_summary_language(&state.pool).await?;
    let ids: Vec<String> = messages.iter().map(|(h, _)| h.id.to_string()).collect();
    let parts = messages
        .iter()
        .zip(&ids)
        .flat_map(|((_, text), id)| [id.as_str(), text.as_str()]);
    let key = summarize::cache_key("thread", spec.id, &language, parts);
    if !fresh {
        if let Some(cached) = storage::summaries::get(&state.pool, &key).await? {
            return Ok(cached);
        }
    }
    let text = state
        .llm_engine
        .complete(engine::Request {
            model_path,
            messages: summarize::thread_prompt(&sources, &language),
            max_tokens: summarize::MAX_THREAD_ANSWER_TOKENS,
            assistant_prefix: spec.assistant_prefix.to_string(),
        })
        .await?;
    storage::summaries::put(&state.pool, &key, &text).await?;
    Ok(text)
}

/// Start fetching a model in the background; returns as soon as the task is
/// spawned. Progress arrives as `llm-download-progress` events and the end —
/// done, cancelled or failed — as `llm-models-changed`.
pub fn start_download(app: AppHandle, id: &str) -> Result<(), AppError> {
    let spec =
        catalog::find(id).ok_or_else(|| AppError::Invalid(format!("Unknown model: {id}")))?;
    let dir = store::models_dir(&app)?;
    if store::is_ready(&dir, spec) {
        return Ok(());
    }
    let Some(cancel) = app
        .state::<AppState>()
        .llm_downloads
        .begin(spec.id, spec.size)
    else {
        return Ok(()); // already running — the running task covers it
    };
    let client = download::client()?;

    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut last_emit = Instant::now();
        let result = download::fetch(&client, spec, &dir, &cancel, |received| {
            state.llm_downloads.progress(spec.id, received);
            if last_emit.elapsed() >= PROGRESS_EVERY {
                last_emit = Instant::now();
                let _ = app.emit(
                    "llm-download-progress",
                    LlmDownloadProgress {
                        id: spec.id.to_string(),
                        received,
                        total: spec.size,
                    },
                );
            }
        })
        .await;

        match result {
            Ok(download::Outcome::Done) => {
                state.llm_downloads.finish(spec.id);
                // The first model to arrive becomes the pick — the user
                // downloaded it to use it, and an explicit choice is one
                // radio click away.
                if let Ok(None) = settings::llm_model(&state.pool).await {
                    let _ = settings::set_llm_model(&state.pool, spec.id).await;
                }
            }
            Ok(download::Outcome::Cancelled) => state.llm_downloads.finish(spec.id),
            Err(err) => state.llm_downloads.fail(spec.id, err.to_string()),
        }
        let _ = app.emit("llm-models-changed", ());
    });
    Ok(())
}

/// Delete a downloaded model file (or dismiss a failed download). A
/// download still running must be cancelled first.
pub async fn remove_model(app: &AppHandle, id: &str) -> Result<(), AppError> {
    let spec =
        catalog::find(id).ok_or_else(|| AppError::Invalid(format!("Unknown model: {id}")))?;
    let downloads = &app.state::<AppState>().llm_downloads;
    if downloads.is_running(spec.id) {
        return Err(AppError::Invalid(
            "This model is still downloading — cancel the download first.".to_string(),
        ));
    }
    downloads.finish(spec.id);
    let path = store::model_path(&store::models_dir(app)?, spec);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    app.emit("llm-models-changed", ())?;
    Ok(())
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
        let status = describe(&scratch_dir("fresh"), true, None, &HashMap::new());

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

        let none = HashMap::new();
        // File present but the feature is off.
        assert!(!describe(&dir, false, Some("qwen3.5-2b".into()), &none).ready);
        // On, but nothing picked.
        assert!(!describe(&dir, true, None, &none).ready);
        // On, but the pick is a model that is not downloaded.
        assert!(!describe(&dir, true, Some("qwen3.5-4b".into()), &none).ready);

        let status = describe(&dir, true, Some("qwen3.5-2b".into()), &none);
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

    #[test]
    fn a_download_in_flight_or_failed_shows_as_such() {
        let dir = scratch_dir("downloading");
        let mut downloads = HashMap::new();
        downloads.insert(
            "qwen3.5-2b".to_string(),
            download::Snapshot {
                received: 5,
                total: 10,
                error: None,
            },
        );
        downloads.insert(
            "qwen3.5-4b".to_string(),
            download::Snapshot {
                received: 3,
                total: 10,
                error: Some("boom".to_string()),
            },
        );

        let status = describe(&dir, true, Some("qwen3.5-2b".into()), &downloads);

        let state_of = |id: &str| {
            status
                .models
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.state.clone())
                .unwrap()
        };
        assert_eq!(
            state_of("qwen3.5-2b"),
            LlmModelState::Downloading {
                received: 5,
                total: 10
            }
        );
        assert_eq!(
            state_of("qwen3.5-4b"),
            LlmModelState::Failed {
                error: "boom".to_string()
            }
        );
        assert_eq!(state_of("gemma-4-e2b"), LlmModelState::Missing);
        // Picked but still downloading is not ready.
        assert!(!status.ready);
    }
}
