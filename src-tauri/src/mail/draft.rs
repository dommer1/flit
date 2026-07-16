//! Draft MIME building — lenient on purpose. A draft is work in progress:
//! recipients may be missing or half-typed, so unlike `smtp::build_message`
//! nothing here validates. Whatever the user typed is written verbatim and
//! must survive the round trip through the server (Gmail web included).
//!
//! why mail-builder and not lettre: lettre refuses to build a message
//! without at least one valid recipient — correct for sending, wrong for
//! drafts. mail-builder writes arbitrary address text and handles the
//! RFC 2047 encoding of non-ASCII headers (Slovak subjects) for us.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mail_builder::headers::address::Address;
use mail_builder::MessageBuilder;

use crate::error::AppError;
use crate::models::{AttachmentRef, OutgoingMessage};

/// A fresh Message-ID (without the angle brackets) for one draft version.
/// Every save gets a new one — it is how the previous version is found and
/// deleted on the server, where IMAP has no "edit in place".
pub fn generate_message_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("flit-draft-{nanos}-{seq}@flit.local")
}

/// Build the raw RFC 5322 bytes of a draft, stamped with `message_id`.
/// Empty address fields produce no header; Bcc is kept — a draft never
/// leaves for recipients, and losing it on reopen would be data loss.
pub async fn build_draft(
    from_email: &str,
    outgoing: &OutgoingMessage,
    message_id: &str,
) -> Result<Vec<u8>, AppError> {
    let mut builder = MessageBuilder::new()
        .message_id(message_id)
        .from(Address::new_address(None::<&str>, from_email))
        .subject(outgoing.subject.as_str())
        .text_body(outgoing.body.as_str());
    if let Some(to) = recipient_list(&outgoing.to) {
        builder = builder.to(to);
    }
    if let Some(cc) = recipient_list(&outgoing.cc) {
        builder = builder.cc(cc);
    }
    if let Some(bcc) = recipient_list(&outgoing.bcc) {
        builder = builder.bcc(bcc);
    }
    let mut total = 0usize;
    for attachment in &outgoing.attachments {
        let (content_type, bytes) = load_attachment(attachment, &mut total).await?;
        builder = builder.attachment(content_type, attachment.name.clone(), bytes);
    }
    Ok(builder.write_to_vec()?)
}

/// Split one comma-separated recipient field into addresses, preserving the
/// typed text; `None` when the field is empty. A piece without an `@` after
/// the first entry belongs to a display name containing a comma
/// (`"Novák, Ján" <jan@x>`), so it is glued back onto the previous piece —
/// the same heuristic the compose window uses in draft.ts.
fn recipient_list(field: &str) -> Option<Address<'_>> {
    let mut pieces: Vec<String> = Vec::new();
    for piece in field.split(',') {
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            continue;
        }
        match pieces.last_mut() {
            Some(last) if !last.contains('@') => {
                last.push_str(", ");
                last.push_str(trimmed);
            }
            _ => pieces.push(trimmed.to_string()),
        }
    }
    if pieces.is_empty() {
        return None;
    }
    Some(Address::new_list(
        pieces.into_iter().map(recipient_piece).collect(),
    ))
}

/// One typed recipient: `Name <addr>` splits into display name + address;
/// anything else (a bare address, a half-typed fragment) passes through
/// verbatim as the address.
fn recipient_piece(piece: String) -> Address<'static> {
    if let Some((name, rest)) = piece.rsplit_once('<') {
        if let Some(addr) = rest.strip_suffix('>') {
            let name = name.trim().trim_matches('"').trim();
            let name = (!name.is_empty()).then(|| name.to_string());
            return Address::new_address(name, addr.trim().to_string());
        }
    }
    Address::new_address(None::<String>, piece)
}

/// Read one attachment from disk, keeping the running size total honest
/// against the same budget the send path enforces.
async fn load_attachment(
    attachment: &AttachmentRef,
    total: &mut usize,
) -> Result<(String, Vec<u8>), AppError> {
    let bytes = tokio::fs::read(&attachment.path).await.map_err(|e| {
        AppError::Io(std::io::Error::other(format!(
            "cannot read attachment {}: {e}",
            attachment.name
        )))
    })?;
    *total += bytes.len();
    crate::mail::smtp::ensure_attachment_budget(*total)?;
    let mime = mime_guess::from_path(&attachment.name).first_or_octet_stream();
    Ok((mime.essence_str().to_string(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::{MessageParser, MimeHeaders};

    fn outgoing() -> OutgoingMessage {
        OutgoingMessage {
            account_id: 1,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: "Pozvánka na obed".to_string(),
            body: "Ahoj, prídeš?".to_string(),
            attachments: Vec::new(),
        }
    }

    async fn parsed(outgoing: &OutgoingMessage) -> mail_parser::Message<'static> {
        let raw = build_draft("domco@example.com", outgoing, "test-id@flit.local")
            .await
            .unwrap();
        // why leak: mail_parser::Message borrows the raw bytes; tests want to
        // return it from this helper, so give the bytes a 'static life.
        MessageParser::default().parse(&*raw.leak()).unwrap()
    }

    #[tokio::test]
    async fn builds_a_draft_with_no_recipients() {
        let message = parsed(&outgoing()).await;

        assert!(message.to().is_none());
        assert_eq!(message.subject(), Some("Pozvánka na obed"));
        assert_eq!(
            message.body_text(0).as_deref(),
            Some("Ahoj, prídeš?"),
            "diacritics must survive the round trip"
        );
    }

    #[tokio::test]
    async fn stamps_the_given_message_id() {
        let message = parsed(&outgoing()).await;

        assert_eq!(message.message_id(), Some("test-id@flit.local"));
    }

    #[tokio::test]
    async fn keeps_a_half_typed_recipient_verbatim() {
        let mut out = outgoing();
        out.to = "jan".to_string();

        let message = parsed(&out).await;

        let to = message.to().unwrap().first().unwrap();
        assert_eq!(to.address(), Some("jan"));
    }

    #[tokio::test]
    async fn splits_recipient_lists_and_keeps_display_names() {
        let mut out = outgoing();
        out.to = "Ján Novák <jan@example.sk>, maria@example.sk".to_string();

        let message = parsed(&out).await;

        let to: Vec<_> = message.to().unwrap().iter().collect();
        assert_eq!(to.len(), 2);
        assert_eq!(to[0].name(), Some("Ján Novák"));
        assert_eq!(to[0].address(), Some("jan@example.sk"));
        assert_eq!(to[1].address(), Some("maria@example.sk"));
    }

    #[tokio::test]
    async fn glues_a_comma_inside_a_display_name_back_together() {
        let mut out = outgoing();
        out.to = "\"Novák, Ján\" <jan@example.sk>".to_string();

        let message = parsed(&out).await;

        let to: Vec<_> = message.to().unwrap().iter().collect();
        assert_eq!(to.len(), 1);
        assert_eq!(to[0].address(), Some("jan@example.sk"));
    }

    #[tokio::test]
    async fn keeps_bcc_in_the_draft() {
        // The opposite of the send path: a draft never reaches recipients,
        // and dropping Bcc here would lose it on reopen.
        let mut out = outgoing();
        out.bcc = "hidden@example.com".to_string();

        let message = parsed(&out).await;

        let bcc = message.bcc().unwrap().first().unwrap();
        assert_eq!(bcc.address(), Some("hidden@example.com"));
    }

    #[tokio::test]
    async fn includes_attachment_bytes() {
        let path = std::env::temp_dir().join("flit-draft-test-notes.txt");
        std::fs::write(&path, b"poznamky").unwrap();
        let mut out = outgoing();
        out.attachments = vec![AttachmentRef {
            path: path.to_string_lossy().into_owned(),
            name: "notes.txt".to_string(),
        }];

        let message = parsed(&out).await;

        let attachment = message.attachment(0).unwrap();
        assert_eq!(attachment.attachment_name(), Some("notes.txt"));
        assert_eq!(attachment.contents(), b"poznamky");
    }

    #[tokio::test]
    async fn fails_on_a_missing_attachment_file() {
        let mut out = outgoing();
        out.attachments = vec![AttachmentRef {
            path: "/nonexistent/flit-draft-gone".to_string(),
            name: "gone.txt".to_string(),
        }];

        let err = build_draft("domco@example.com", &out, "id@flit.local")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("gone.txt"));
    }

    #[test]
    fn message_ids_are_unique_and_addressable() {
        let a = generate_message_id();
        let b = generate_message_id();

        assert_ne!(a, b);
        assert!(a.contains('@'));
        assert!(!a.contains('<') && !a.contains('>'));
    }
}
