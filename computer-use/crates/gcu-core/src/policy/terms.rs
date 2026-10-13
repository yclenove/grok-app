//! Fifteen-locale sensitive-action vocabulary compiled into the binary.
//!
//! The files live in `data/sensitive_terms`. Loading does not touch the filesystem
//! at runtime. Matching is synchronous and does not wait or cancel.

use super::text::{fold, phrase_in};
use std::sync::OnceLock;

/// Folded terms from every shipped locale.
pub(crate) struct TermIndex {
    terms: Vec<String>,
}

impl TermIndex {
    pub(crate) fn builtin() -> &'static Self {
        static INDEX: OnceLock<TermIndex> = OnceLock::new();
        INDEX.get_or_init(|| Self {
            terms: load(EMBEDDED),
        })
    }

    /// Whether any label contains a sensitive term.
    pub(crate) fn hit(&self, labels: &[&str]) -> bool {
        let folded: Vec<String> = labels
            .iter()
            .filter(|label| !label.is_empty())
            .map(|label| fold(label))
            .collect();
        self.terms
            .iter()
            .any(|term| folded.iter().any(|label| phrase_in(label, term)))
    }
}

fn load(files: &[(&str, &str)]) -> Vec<String> {
    let mut terms = Vec::new();
    for (_, source) in files {
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let folded = fold(line);
            if !folded.is_empty() && !terms.contains(&folded) {
                terms.push(folded);
            }
        }
    }
    terms
}

const EMBEDDED: &[(&str, &str)] = &[
    ("de", include_str!("../../data/sensitive_terms/de.txt")),
    ("en", include_str!("../../data/sensitive_terms/en.txt")),
    ("es", include_str!("../../data/sensitive_terms/es.txt")),
    ("fil", include_str!("../../data/sensitive_terms/fil.txt")),
    ("fr", include_str!("../../data/sensitive_terms/fr.txt")),
    ("id", include_str!("../../data/sensitive_terms/id.txt")),
    ("it", include_str!("../../data/sensitive_terms/it.txt")),
    ("ja", include_str!("../../data/sensitive_terms/ja.txt")),
    ("ko", include_str!("../../data/sensitive_terms/ko.txt")),
    (
        "pt-BR",
        include_str!("../../data/sensitive_terms/pt-BR.txt"),
    ),
    ("ru", include_str!("../../data/sensitive_terms/ru.txt")),
    ("ta", include_str!("../../data/sensitive_terms/ta.txt")),
    ("uk", include_str!("../../data/sensitive_terms/uk.txt")),
    ("zh", include_str!("../../data/sensitive_terms/zh.txt")),
    (
        "zh-TW",
        include_str!("../../data/sensitive_terms/zh-TW.txt"),
    ),
];
