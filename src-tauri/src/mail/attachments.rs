//! Metadata for files dropped on the compose window. Only stat calls —
//! attachment bytes are read later, when the MIME message is built
//! (mail::smtp).

use crate::error::AppError;
use crate::models::AttachmentInfo;

/// Stat dropped paths into chip metadata: duplicates collapse (same file
/// dropped twice attaches once), directories are skipped (a drop of e.g. a
/// Finder selection may include folders we can't attach). A missing or
/// unreadable file errors so the user learns at drop time, not at send time.
pub async fn inspect(paths: Vec<String>) -> Result<Vec<AttachmentInfo>, AppError> {
    let mut seen = std::collections::HashSet::new();
    let mut infos = Vec::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(|e| AppError::Smtp(format!("cannot attach {path}: {e}")))?;
        if meta.is_dir() {
            continue;
        }
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        infos.push(AttachmentInfo {
            path,
            name,
            size: meta.len(),
        });
    }
    Ok(infos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, contents: &[u8]) -> String {
        let dir = std::env::temp_dir().join("flit-attach-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn returns_name_and_size_per_file() {
        let path = temp_file("notes.txt", b"hello");

        let infos = inspect(vec![path.clone()]).await.unwrap();

        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].path, path);
        assert_eq!(infos[0].name, "notes.txt");
        assert_eq!(infos[0].size, 5);
    }

    #[tokio::test]
    async fn duplicate_paths_collapse_to_one() {
        let path = temp_file("dup.txt", b"x");

        let infos = inspect(vec![path.clone(), path]).await.unwrap();

        assert_eq!(infos.len(), 1);
    }

    #[tokio::test]
    async fn directories_are_skipped() {
        let file = temp_file("kept.txt", b"x");
        let dir = std::env::temp_dir().to_string_lossy().into_owned();

        let infos = inspect(vec![dir, file]).await.unwrap();

        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "kept.txt");
    }

    #[tokio::test]
    async fn missing_files_error_at_drop_time() {
        let err = inspect(vec!["/nonexistent/flit/void.txt".to_string()])
            .await
            .unwrap_err();

        assert!(err.to_string().contains("void.txt"));
    }
}
