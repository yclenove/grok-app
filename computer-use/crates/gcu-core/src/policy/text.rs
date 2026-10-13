//! Unicode folding shared by sensitive terms, chords, and execution-surface keywords.
//!
//! NFKC then Unicode lowercase. This is synchronous and allocates the folded string.
//! It does not read the filesystem or wait.

use unicode_normalization::UnicodeNormalization;

/// Compatibility-normalize and case-fold `input`.
pub(crate) fn fold(input: &str) -> String {
    input.nfkc().collect::<String>().to_lowercase()
}

/// Whether `needle` occurs in `haystack` as a whole token or a whole phrase.
///
/// Both strings must already be [`fold`]ed. Latin tokens require non-letter
/// boundaries so `send` does not match `sending`. Tokens with no ASCII letters
/// (CJK and similar) match by containment, because those scripts do not separate
/// words with spaces.
pub(crate) fn phrase_in(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() || haystack.is_empty() {
        return false;
    }
    if !needle.chars().any(|c| c.is_ascii_alphanumeric()) {
        return haystack.contains(needle);
    }
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(needle) {
        let index = start + offset;
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + needle.len()..].chars().next();
        if before.is_none_or(|c| !is_latin_word(c)) && after.is_none_or(|c| !is_latin_word(c)) {
            return true;
        }
        let step = haystack[index..].chars().next().map_or(1, char::len_utf8);
        start = index + step;
    }
    false
}

fn is_latin_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '\''
}

/// ASCII word boundary. Used for execution-host name hints, which are ASCII.
pub(crate) fn ascii_word(haystack: &str, word: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(word))
}
