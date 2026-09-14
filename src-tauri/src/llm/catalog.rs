//! The models the user may download. Deliberately a static table: each
//! entry pins an exact file (repository revision + SHA-256), so a download
//! is reproducible and verifiable, and no code path ever "discovers" models
//! from the network.

/// One downloadable model file, as offered in Settings → Experimental.
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    /// Stable identifier stored in settings — never rename an existing one.
    pub id: &'static str,
    pub name: &'static str,
    /// One line for the picker: what this model is good for.
    pub description: &'static str,
    /// File name on disk; also the last path segment of `url`.
    pub file_name: &'static str,
    /// Exact byte size of the file — checked after download and used to
    /// tell a complete file from a leftover partial one.
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: &'static str,
    /// Download URL, pinned to a repository revision so the bytes behind it
    /// can never silently change.
    pub url: &'static str,
    /// The default pick, highlighted in the picker.
    pub recommended: bool,
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "qwen3.5-2b",
        name: "Qwen3.5 2B",
        description: "Fast and small, good at many languages. The best starting point.",
        file_name: "Qwen3.5-2B-Q4_K_M.gguf",
        size: 1_280_835_840,
        sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
        url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-Q4_K_M.gguf",
        recommended: true,
    },
    ModelSpec {
        id: "qwen3.5-4b",
        name: "Qwen3.5 4B",
        description: "Better summaries, about twice the memory and time of the 2B.",
        file_name: "Qwen3.5-4B-Q4_K_M.gguf",
        size: 2_740_937_888,
        sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
        url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/e87f176479d0855a907a41277aca2f8ee7a09523/Qwen3.5-4B-Q4_K_M.gguf",
        recommended: false,
    },
    ModelSpec {
        id: "gemma-4-e2b",
        name: "Gemma 4 E2B",
        description: "Google's small model, for comparison. Largest download.",
        file_name: "gemma-4-E2B_q4_0-it.gguf",
        size: 3_349_516_256,
        sha256: "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
        url: "https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/675cff42a74c774d6cb76f76d8eacb49b48c9b93/gemma-4-E2B_q4_0-it.gguf",
        recommended: false,
    },
];

/// Look a model up by its stored id; `None` for ids this build does not know
/// (a setting written by a newer or older version).
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_model_is_pinned_and_well_formed() {
        for spec in MODELS {
            assert!(spec.size > 0, "{}: size", spec.id);
            assert_eq!(spec.sha256.len(), 64, "{}: sha256 length", spec.id);
            assert!(
                spec.sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{}: sha256 must be lowercase hex",
                spec.id
            );
            // TLS-only, and the one sanctioned host — see CLAUDE.md.
            assert!(
                spec.url.starts_with("https://huggingface.co/"),
                "{}: url host",
                spec.id
            );
            // A revision pin looks like /resolve/<40 hex>/ — not /resolve/main/.
            let revision = spec
                .url
                .split("/resolve/")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or("");
            assert_eq!(revision.len(), 40, "{}: url must pin a revision", spec.id);
            assert!(spec.url.ends_with(spec.file_name), "{}: file name", spec.id);
            assert!(spec.file_name.ends_with(".gguf"), "{}: gguf", spec.id);
        }
    }

    #[test]
    fn ids_and_file_names_are_unique() {
        let ids: HashSet<_> = MODELS.iter().map(|m| m.id).collect();
        let files: HashSet<_> = MODELS.iter().map(|m| m.file_name).collect();
        assert_eq!(ids.len(), MODELS.len());
        assert_eq!(files.len(), MODELS.len());
    }

    #[test]
    fn exactly_one_model_is_recommended() {
        assert_eq!(MODELS.iter().filter(|m| m.recommended).count(), 1);
    }

    #[test]
    fn find_knows_catalog_ids_only() {
        assert_eq!(find("qwen3.5-2b").map(|m| m.name), Some("Qwen3.5 2B"));
        assert!(find("gpt-5").is_none());
        assert!(find("").is_none());
    }
}
