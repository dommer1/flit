//! Attachment files on disk: metadata for files dropped on the compose
//! window (only stat calls — the bytes are read when the MIME message is
//! built, mail::smtp), the copies Quick Look shows of received attachments
//! that were never saved, and the temp files that carry a reopened draft's
//! attachments into a compose window. Received attachments' bytes are
//! fetched from the server on demand — they never touch the local DB.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::models::{AttachmentInfo, AttachmentRef, MessageAttachment};
use crate::state::AppState;
use crate::{mail, storage};

/// Where Quick Look's copy of a received attachment lives under the app
/// cache: a folder per attachment id, so two attachments named alike never
/// collide, holding the file under its own name — the panel shows that
/// name as its title.
pub fn quick_look_copy(cache_dir: &Path, attachment_id: i64, filename: &str) -> PathBuf {
    cache_dir
        .join("quick-look")
        .join(attachment_id.to_string())
        .join(filename)
}

/// Write a Quick Look copy. It is written beside its final name and renamed
/// into place, so a fetch cut off half-way never leaves a truncated file
/// that a later Space would show as if it were whole.
pub async fn write_quick_look_copy(path: &Path, data: &[u8]) -> Result<(), AppError> {
    let dir = path
        .parent()
        .ok_or_else(|| AppError::Invalid(format!("{} has no folder", path.display())))?;
    tokio::fs::create_dir_all(dir).await?;
    let partial = dir.join(".partial");
    tokio::fs::write(&partial, data).await?;
    tokio::fs::rename(&partial, path).await?;
    Ok(())
}

/// Delete every Quick Look copy. Runs at launch: they are mail content on
/// disk outside the database, and must not pile up there.
pub async fn clear_quick_look_copies(cache_dir: &Path) -> Result<(), AppError> {
    match tokio::fs::remove_dir_all(cache_dir.join("quick-look")).await {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

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

/// Write attachment bytes into temp files — a folder per part, named
/// `{label}-{index}` — so the compose window can treat them exactly like
/// freshly dropped files (only paths travel through the app). The names
/// must already be safe filenames (parse_draft, safe_filename), so the files
/// always land inside the temp directory.
pub async fn stash(
    label: &str,
    parts: Vec<mail::parse::DraftAttachment>,
) -> Result<Vec<AttachmentRef>, AppError> {
    let mut refs = Vec::with_capacity(parts.len());
    for (index, part) in parts.into_iter().enumerate() {
        let dir = std::env::temp_dir().join(format!("{label}-{index}"));
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(&part.name);
        tokio::fs::write(&path, &part.data).await?;
        refs.push(AttachmentRef {
            path: path.to_string_lossy().into_owned(),
            name: part.name,
        });
    }
    Ok(refs)
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

/// Download the raw RFC822 bytes of one cached message from its server.
pub async fn fetch_raw_message(
    state: &AppState,
    loc: &storage::messages::MessageLocation,
) -> Result<Vec<u8>, AppError> {
    let account = storage::accounts::get(&state.pool, loc.account_id).await?;
    let credential = state.credential(&account).await?;
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &credential,
    )
    .await?;
    session
        .select(&loc.mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {}: {e}", loc.mailbox)))?;
    let raw = mail::imap::fetch_body(&mut session, loc.uid).await;
    let _ = session.logout().await;
    raw?.ok_or_else(|| AppError::Imap("message no longer on the server".to_string()))
}

/// One attachment's bytes, fetched for saving or previewing: its message is
/// re-fetched from the server and the part re-extracted by index —
/// attachment bytes never touch the local DB.
pub async fn fetch_attachment(
    state: &AppState,
    meta: &MessageAttachment,
) -> Result<Vec<u8>, AppError> {
    let loc = storage::messages::location(&state.pool, meta.message_id).await?;
    let raw = fetch_raw_message(state, &loc).await?;
    attachment_bytes(&raw, meta)
}

/// Extract one attachment from its raw message.
pub fn attachment_bytes(raw: &[u8], meta: &MessageAttachment) -> Result<Vec<u8>, AppError> {
    mail::parse::attachment_data(raw, meta.part_index).ok_or_else(|| {
        AppError::Imap(format!(
            "attachment {} not found in the message",
            meta.filename
        ))
    })
}

/// A received message's attachments as files a forward can carry: the
/// message is downloaded once, every part taken out of it and written to
/// temp files of this forward's own. Nothing is downloaded when there is
/// nothing to forward, or more than a message can carry.
pub async fn for_forward(
    state: &AppState,
    message_id: i64,
    attachments: &[MessageAttachment],
) -> Result<Vec<AttachmentRef>, AppError> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }
    ensure_forwardable(attachments)?;
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let raw = fetch_raw_message(state, &loc).await?;
    let parts = forward_parts(&raw, attachments)?;
    stash(&forward_label(message_id), parts).await
}

/// Refuse a forward whose attachments alone exceed the send limit — the
/// send would fail anyway, only later, after the user wrote the message.
/// The sizes are the decoded ones the limit counts at send time.
fn ensure_forwardable(attachments: &[MessageAttachment]) -> Result<(), AppError> {
    let total: i64 = attachments.iter().map(|a| a.size.max(0)).sum();
    mail::smtp::ensure_attachment_budget(usize::try_from(total).unwrap_or(usize::MAX)).map_err(
        |_| {
            AppError::Invalid(format!(
                "Can't forward this message: its attachments total {:.1} MB, \
                 over the 25 MB a message can carry.",
                total as f64 / (1024.0 * 1024.0)
            ))
        },
    )
}

/// Every listed attachment taken out of one raw message, under its cached
/// (already safe) filename.
fn forward_parts(
    raw: &[u8],
    attachments: &[MessageAttachment],
) -> Result<Vec<mail::parse::DraftAttachment>, AppError> {
    attachments
        .iter()
        .map(|meta| {
            Ok(mail::parse::DraftAttachment {
                name: meta.filename.clone(),
                data: attachment_bytes(raw, meta)?,
            })
        })
        .collect()
}

/// Temp folder label for one forward. why unique per call: forwarding the
/// same message again must not rewrite files an earlier forward, still in
/// its undo window, is about to read.
fn forward_label(message_id: i64) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("flit-forward-{message_id}-{nanos}-{seq}")
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
    async fn stashed_parts_get_a_folder_each_so_alike_names_never_collide() {
        let part = |data: &[u8]| mail::parse::DraftAttachment {
            name: "scan.pdf".to_string(),
            data: data.to_vec(),
        };
        let label = format!("flit-stash-test-{}", std::process::id());

        let refs = stash(&label, vec![part(b"first"), part(b"second")])
            .await
            .unwrap();

        assert_eq!(refs.len(), 2);
        assert_ne!(refs[0].path, refs[1].path);
        assert_eq!(refs[0].name, "scan.pdf");
        assert_eq!(refs[1].name, "scan.pdf");
        assert_eq!(tokio::fs::read(&refs[0].path).await.unwrap(), b"first");
        assert_eq!(tokio::fs::read(&refs[1].path).await.unwrap(), b"second");
        for r in &refs {
            let _ = tokio::fs::remove_dir_all(Path::new(&r.path).parent().unwrap()).await;
        }
    }

    fn meta(part_index: i64, filename: &str, size: i64) -> MessageAttachment {
        MessageAttachment {
            id: part_index + 1,
            message_id: 7,
            part_index,
            filename: filename.to_string(),
            content_type: "application/octet-stream".to_string(),
            size,
        }
    }

    const RAW_TWO_FILES: &[u8] = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: Files\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain\r\n\
\r\n\
See attached.\r\n\
--b1\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi1mYWtl\r\n\
--b1\r\n\
Content-Type: text/csv; name=\"data.csv\"\r\n\
Content-Disposition: attachment; filename=\"data.csv\"\r\n\
\r\n\
a,b\r\n\
--b1--\r\n";

    #[test]
    fn a_forward_takes_every_attachment_out_of_one_raw_message() {
        let metas = [meta(0, "report.pdf", 9), meta(1, "data.csv", 3)];

        let parts = forward_parts(RAW_TWO_FILES, &metas).unwrap();

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "report.pdf");
        assert_eq!(parts[0].data, b"%PDF-fake");
        assert_eq!(parts[1].name, "data.csv");
        assert_eq!(parts[1].data, b"a,b");
    }

    #[test]
    fn a_forward_fails_when_an_attachment_is_gone_from_the_message() {
        let err = forward_parts(RAW_TWO_FILES, &[meta(5, "lost.zip", 10)]).unwrap_err();

        assert!(err.to_string().contains("lost.zip"), "{err}");
    }

    #[test]
    fn a_forward_over_the_send_limit_is_refused_before_any_download() {
        const MB: i64 = 1024 * 1024;
        assert!(ensure_forwardable(&[meta(0, "a.zip", 20 * MB), meta(1, "b.zip", 5 * MB)]).is_ok());

        let err = ensure_forwardable(&[meta(0, "a.zip", 20 * MB), meta(1, "b.zip", 11 * MB)])
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Can't forward this message: its attachments total 31.0 MB, \
             over the 25 MB a message can carry."
        );
    }

    #[test]
    fn every_forward_stashes_into_folders_of_its_own() {
        // A second forward of the same message must never overwrite the
        // files a first one, still waiting in the outbox, is about to send.
        assert_ne!(forward_label(7), forward_label(7));
        assert!(forward_label(7).starts_with("flit-forward-7-"));
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

    #[test]
    fn each_attachment_gets_its_own_quick_look_folder() {
        let cache = std::path::Path::new("/cache");

        let copy = quick_look_copy(cache, 7, "report.pdf");

        // Same name in two messages never collides; the file keeps its
        // name, which the panel shows as the title.
        assert_eq!(copy, cache.join("quick-look/7/report.pdf"));
        assert_ne!(copy, quick_look_copy(cache, 8, "report.pdf"));
    }

    #[tokio::test]
    async fn a_quick_look_copy_appears_only_once_complete() {
        let cache = std::env::temp_dir().join(format!("flit-ql-write-{}", std::process::id()));
        let copy = quick_look_copy(&cache, 3, "notes.txt");

        write_quick_look_copy(&copy, b"hello").await.unwrap();

        assert_eq!(tokio::fs::read(&copy).await.unwrap(), b"hello");
        // Nothing but the file itself is left in its folder.
        let mut entries = tokio::fs::read_dir(copy.parent().unwrap()).await.unwrap();
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name());
        }
        assert_eq!(names, ["notes.txt"]);
        let _ = tokio::fs::remove_dir_all(&cache).await;
    }

    #[tokio::test]
    async fn clearing_quick_look_copies_leaves_the_rest_of_the_cache() {
        let cache = std::env::temp_dir().join(format!("flit-ql-clear-{}", std::process::id()));
        let copy = quick_look_copy(&cache, 3, "notes.txt");
        write_quick_look_copy(&copy, b"hello").await.unwrap();
        tokio::fs::write(cache.join("other"), b"x").await.unwrap();

        clear_quick_look_copies(&cache).await.unwrap();

        assert!(!tokio::fs::try_exists(cache.join("quick-look"))
            .await
            .unwrap());
        assert!(tokio::fs::try_exists(cache.join("other")).await.unwrap());
        // Nothing to clear (first launch) is fine too.
        clear_quick_look_copies(&cache).await.unwrap();
        let _ = tokio::fs::remove_dir_all(&cache).await;
    }

    #[tokio::test]
    async fn missing_files_error_at_drop_time() {
        let err = inspect(vec!["/nonexistent/flit/void.txt".to_string()])
            .await
            .unwrap_err();

        assert!(err.to_string().contains("void.txt"));
    }
}
