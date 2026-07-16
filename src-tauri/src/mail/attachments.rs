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

/// Sources above this size get no thumbnail — decoding a huge image for a
/// 256px preview is wasted work and a decompression-bomb risk.
const MAX_PREVIEW_SOURCE_BYTES: u64 = 10 * 1024 * 1024;
/// Longest edge of the generated thumbnail.
const PREVIEW_EDGE: u32 = 256;

/// Best-effort thumbnail for one attachment as a `data:image/png` URI.
/// `None` for anything that is not a decodable raster image (only PNG, JPEG
/// and WebP are compiled in) — the card then shows a generic placeholder.
/// Never errors: a preview is decoration, not part of the message.
pub async fn preview(path: String) -> Option<String> {
    let meta = tokio::fs::metadata(&path).await.ok()?;
    if !meta.is_file() || meta.len() > MAX_PREVIEW_SOURCE_BYTES {
        return None;
    }
    // why spawn_blocking: decode + resize is CPU-bound; on the async runtime
    // it would stall every other task for the duration.
    tokio::task::spawn_blocking(move || render_thumbnail(&path))
        .await
        .ok()
        .flatten()
}

fn render_thumbnail(path: &str) -> Option<String> {
    // why with_guessed_format: sniffs the actual bytes instead of trusting
    // the extension, so a mislabeled file can't pick a decoder we did not
    // intend (and unsupported formats bail out to None here).
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumb = img.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE);
    let mut buf = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    use base64::Engine;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf)
    ))
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
    async fn preview_thumbnails_a_png() {
        let dir = std::env::temp_dir().join("flit-attach-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tiny.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([200, 40, 40]))
            .save(&path)
            .unwrap();

        let uri = preview(path.to_string_lossy().into_owned()).await;

        assert!(uri.unwrap().starts_with("data:image/png;base64,"));
    }

    #[tokio::test]
    async fn preview_declines_non_images() {
        let path = temp_file("not-an-image.pdf", b"%PDF-1.4 nope");

        assert_eq!(preview(path).await, None);
    }

    #[tokio::test]
    async fn preview_declines_missing_and_oversized_files() {
        assert_eq!(preview("/nonexistent/flit/img.png".to_string()).await, None);

        let big = vec![0u8; (MAX_PREVIEW_SOURCE_BYTES + 1) as usize];
        let path = temp_file("huge.png", &big);
        assert_eq!(preview(path).await, None);
    }

    #[tokio::test]
    async fn missing_files_error_at_drop_time() {
        let err = inspect(vec!["/nonexistent/flit/void.txt".to_string()])
            .await
            .unwrap_err();

        assert!(err.to_string().contains("void.txt"));
    }
}
