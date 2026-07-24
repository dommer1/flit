//! Splitting a typed recipient field ("a@x, Name <b@y>") into pieces —
//! shared by the draft and send paths so both read the compose window's
//! text the same way.

/// Split one comma-separated recipient field into typed entries, preserving
/// the text of each. A piece without an `@` after the first entry belongs to
/// a display name containing a comma (`"Novák, Ján" <jan@x>`), so it is
/// glued back onto the previous piece — the same heuristic the compose
/// window uses in src/lib/recipients.ts.
pub fn split(field: &str) -> Vec<String> {
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
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_plain_list() {
        assert_eq!(
            split("a@x.com, Bob <b@y.com>"),
            vec!["a@x.com", "Bob <b@y.com>"]
        );
    }

    #[test]
    fn keeps_a_comma_inside_a_display_name_together() {
        assert_eq!(
            split("Obchod, Gavaplast s.r.o. <obchod@gavaplast.sk>, b@y.com"),
            vec!["Obchod, Gavaplast s.r.o. <obchod@gavaplast.sk>", "b@y.com"]
        );
    }

    #[test]
    fn empty_and_blank_fields_yield_nothing() {
        assert!(split("").is_empty());
        assert!(split("  ,  , ").is_empty());
    }
}
