//! Where downloaded model files live and whether a model is ready to use.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::llm::catalog::ModelSpec;

/// The directory holding model files: `<app data dir>/models`. Created on
/// demand so callers can write into it straight away.
pub fn models_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_data_dir()?.join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The final path of a model file.
pub fn model_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file_name)
}

/// Whether the model's file is complete on disk. Only the size is checked:
/// the hash is verified once, when the download finishes, and re-hashing
/// gigabytes on every status call would make Settings crawl. A file of the
/// wrong size — a download that died before the rename, a truncated copy —
/// reads as not ready.
pub fn is_ready(dir: &Path, spec: &ModelSpec) -> bool {
    std::fs::metadata(model_path(dir, spec))
        .map(|meta| meta.is_file() && meta.len() == spec.size)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(size: u64) -> ModelSpec {
        ModelSpec {
            id: "test",
            name: "Test",
            description: "",
            file_name: "test.gguf",
            size,
            sha256: "",
            url: "",
            recommended: false,
        }
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flit-llm-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_is_not_ready() {
        let dir = scratch_dir("missing");
        assert!(!is_ready(&dir, &spec(3)));
    }

    #[test]
    fn a_file_of_the_pinned_size_is_ready() {
        let dir = scratch_dir("ready");
        std::fs::write(model_path(&dir, &spec(3)), b"abc").unwrap();
        assert!(is_ready(&dir, &spec(3)));
    }

    #[test]
    fn a_file_of_the_wrong_size_is_not_ready() {
        let dir = scratch_dir("short");
        std::fs::write(model_path(&dir, &spec(3)), b"ab").unwrap();
        assert!(!is_ready(&dir, &spec(3)));
    }
}
