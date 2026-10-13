//! Denylist and execution-host classification.
//!
//! The built-in table is compiled in from `data/denylist.toml`. This module does
//! not read the filesystem. The driver fills [`gcu_host_api::AppIdentity::image_path`]
//! with the canonical image after it has resolved symlinks, aliases, junctions,
//! and launch wrappers. A display name is only a hint.
//!
//! User and enterprise policy can append identifiers through
//! [`gcu_host_api::PolicyConfig::extra_denied_apps`]. An extra entry cannot
//! remove a built-in rule, including when it is prefixed with `!`.
//!
//! Matching is synchronous. It does not block, time out, or cancel.

use gcu_host_api::{AppIdentity, Platform};
use std::sync::OnceLock;
use thiserror::Error;

/// Why a built-in rule rejected an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyCategory {
    /// The Grok application itself.
    GrokSelf,
    /// A terminal or shell host.
    Terminal,
    /// A credential store or password manager.
    Credentials,
    /// An OS security prompt (UAC, polkit, SecurityAgent).
    SecurityPrompt,
    /// Endpoint security software.
    SecuritySoftware,
}

/// Whether the matching rule is built in or appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyKind {
    /// Shipped denylist category.
    Builtin(DenyCategory),
    /// User or enterprise addition. Additions cannot delete built-ins.
    Appended,
}

/// Result of comparing one OS identity with the denylist and execution-host hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityVerdict {
    /// The identity matches a deny rule.
    Denied {
        /// Built-in category, or an appended identifier.
        kind: DenyKind,
        /// Identifier of the rule that matched.
        rule_id: String,
    },
    /// The image looks like a shell or interpreter that is not on the denylist.
    ///
    /// The denylist is not a complete list of execution hosts. Callers hand off.
    UnknownExecutionHost,
    /// No deny rule and no execution-host evidence.
    Clear,
}

/// Identity fields the matcher is allowed to see.
///
/// `display_name` is not compared to deny rules. It only contributes to the
/// unknown-host hint, and only together with an empty or generic image path.
#[derive(Debug, Clone, Copy)]
pub struct IdentityEvidence<'a> {
    /// OS-verified identity. Callers pass the driver's canonical path.
    pub identity: &'a AppIdentity,
    /// OS display name. Not an authorization key.
    pub display_name: &'a str,
}

/// The embedded denylist could not be parsed. The message is not the file body.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDataError {
    /// `denylist.toml` is missing a field, has an unknown category, or a bad hash.
    #[error("denylist could not be parsed")]
    Denylist,
}

#[derive(Debug, Clone)]
struct DenyRule {
    platform: Platform,
    id: String,
    category: DenyCategory,
    sha256: Option<[u8; 32]>,
}

/// Compiled denylist. Built-in data is process-wide and immutable.
#[derive(Debug, Clone)]
pub struct DenyTable {
    rules: Vec<DenyRule>,
}

impl DenyTable {
    /// Embedded denylist. Parsing happens once.
    ///
    /// Blocking: none after the first call. The first call parses memory.
    /// Timeout: none. Cancellation: none. Thread safety: the returned table is
    /// shared and immutable.
    pub fn builtin() -> &'static Self {
        static TABLE: OnceLock<DenyTable> = OnceLock::new();
        TABLE.get_or_init(|| {
            #[allow(clippy::expect_used)]
            Self::parse(include_str!("../../data/denylist.toml"))
                .expect("embedded denylist is valid")
        })
    }

    /// Parse a denylist document. Intended for the embedded file and for tests.
    ///
    /// A rule matches later by SHA-256, then by exact ASCII case-insensitive
    /// equality on the key, the image path, the image basename, or the key
    /// basename. `match` must be `exact` when present. Optional `sha256` is
    /// 64 hex digits. This does not touch the filesystem.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
    pub fn parse(source: &str) -> Result<Self, PolicyDataError> {
        let raw: RawFile = toml::from_str(source).map_err(|_| PolicyDataError::Denylist)?;
        let mut rules = Vec::new();
        push_section(&mut rules, Platform::Macos, &raw.macos)?;
        push_section(&mut rules, Platform::Windows, &raw.windows)?;
        push_section(&mut rules, Platform::Linux, &raw.linux)?;
        Ok(Self { rules })
    }
}

/// Classify `evidence` against `table` and appended identifiers.
///
/// Built-in rules win over appends. An appended id is matched with the same
/// exact comparison and cannot withdraw a built-in hit. After a miss, a known
/// interpreter basename, or a terminal-like display name paired with an empty
/// or generic launcher path, yields [`IdentityVerdict::UnknownExecutionHost`].
/// A terminal-like display name on a specific non-exec image is [`IdentityVerdict::Clear`].
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
pub fn classify(
    table: &DenyTable,
    evidence: &IdentityEvidence<'_>,
    extra: &[String],
) -> IdentityVerdict {
    let identity = evidence.identity;
    for rule in &table.rules {
        if rule.platform == identity.platform && rule_matches(rule, identity) {
            return IdentityVerdict::Denied {
                kind: DenyKind::Builtin(rule.category),
                rule_id: rule.id.clone(),
            };
        }
    }
    for appended in extra {
        let id = appended.trim();
        if !id.is_empty() && id_matches(id, identity) {
            return IdentityVerdict::Denied {
                kind: DenyKind::Appended,
                rule_id: id.to_string(),
            };
        }
    }
    if execution_host(identity, evidence.display_name) {
        IdentityVerdict::UnknownExecutionHost
    } else {
        IdentityVerdict::Clear
    }
}

fn rule_matches(rule: &DenyRule, identity: &AppIdentity) -> bool {
    hash_matches(rule.sha256, identity.sha256) || id_matches(&rule.id, identity)
}

fn hash_matches(rule: Option<[u8; 32]>, actual: Option<[u8; 32]>) -> bool {
    matches!((rule, actual), (Some(expected), Some(got)) if expected == got)
}

fn id_matches(rule_id: &str, identity: &AppIdentity) -> bool {
    eq(rule_id, &identity.key)
        || eq(rule_id, &identity.image_path)
        || eq(rule_id, file_name(&identity.image_path))
        || eq(rule_id, file_name(&identity.key))
}

fn eq(rule_id: &str, candidate: &str) -> bool {
    !candidate.is_empty() && candidate.eq_ignore_ascii_case(rule_id)
}

fn file_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("")
}

fn execution_host(identity: &AppIdentity, display_name: &str) -> bool {
    let image = file_name(&identity.image_path);
    let key_base = file_name(&identity.key);
    if token_eq(image, EXEC_HOSTS)
        || token_eq(key_base, EXEC_HOSTS)
        || token_eq(&identity.key, EXEC_HOSTS)
    {
        return true;
    }
    let hinted = NAME_HINTS
        .iter()
        .any(|word| super::text::ascii_word(display_name, word));
    let generic = identity.image_path.is_empty()
        || identity.key.is_empty()
        || token_eq(image, GENERIC_LAUNCHERS)
        || token_eq(key_base, GENERIC_LAUNCHERS);
    hinted && generic
}

fn token_eq(value: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| value.eq_ignore_ascii_case(token))
}

const EXEC_HOSTS: &[&str] = &[
    "bash",
    "zsh",
    "sh",
    "fish",
    "dash",
    "tmux",
    "screen",
    "ssh",
    "python",
    "python3",
    "node",
    "ruby",
    "perl",
    "osascript",
    "busybox",
    "x-terminal-emulator",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "wsl",
    "wsl.exe",
    "conhost",
    "conhost.exe",
];

const GENERIC_LAUNCHERS: &[&str] = &[
    "open",
    "python",
    "python3",
    "node",
    "electron",
    "bash",
    "sh",
    "zsh",
    "xdg-open",
    "start",
    "cmd",
    "powershell",
    "pwsh",
];

const NAME_HINTS: &[&str] = &[
    "terminal",
    "console",
    "shell",
    "ssh",
    "powershell",
    "cmd",
    "repl",
];

fn push_section(
    rules: &mut Vec<DenyRule>,
    platform: Platform,
    entries: &[RawEntry],
) -> Result<(), PolicyDataError> {
    for entry in entries {
        if entry.id.trim().is_empty() {
            return Err(PolicyDataError::Denylist);
        }
        if entry
            .match_kind
            .as_deref()
            .is_some_and(|kind| kind != "exact")
        {
            return Err(PolicyDataError::Denylist);
        }
        let sha256 = match entry.sha256.as_deref() {
            Some(value) => Some(parse_hash(value)?),
            None => None,
        };
        rules.push(DenyRule {
            platform,
            id: entry.id.clone(),
            category: parse_category(&entry.category)?,
            sha256,
        });
    }
    Ok(())
}

fn parse_category(value: &str) -> Result<DenyCategory, PolicyDataError> {
    match value {
        "grok_self" => Ok(DenyCategory::GrokSelf),
        "terminal" => Ok(DenyCategory::Terminal),
        "credentials" => Ok(DenyCategory::Credentials),
        "security_prompt" => Ok(DenyCategory::SecurityPrompt),
        "security_software" => Ok(DenyCategory::SecuritySoftware),
        _ => Err(PolicyDataError::Denylist),
    }
}

fn parse_hash(value: &str) -> Result<[u8; 32], PolicyDataError> {
    let bytes = value.trim().as_bytes();
    if bytes.len() != 64 {
        return Err(PolicyDataError::Denylist);
    }
    let mut out = [0; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let high = nibble(bytes[index * 2])?;
        let low = nibble(bytes[index * 2 + 1])?;
        *byte = (high << 4) | low;
    }
    Ok(out)
}

fn nibble(byte: u8) -> Result<u8, PolicyDataError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(PolicyDataError::Denylist),
    }
}

#[derive(serde::Deserialize)]
struct RawFile {
    #[serde(default)]
    macos: Vec<RawEntry>,
    #[serde(default)]
    windows: Vec<RawEntry>,
    #[serde(default)]
    linux: Vec<RawEntry>,
}

#[derive(serde::Deserialize)]
struct RawEntry {
    id: String,
    category: String,
    #[serde(default, rename = "match")]
    match_kind: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_host_api::SignatureState;

    fn linux(key: &str, image: &str, sha: Option<[u8; 32]>) -> AppIdentity {
        AppIdentity {
            platform: Platform::Linux,
            key: key.into(),
            team_id: None,
            publisher: None,
            sha256: sha,
            signature: SignatureState::Unsigned,
            image_path: image.into(),
        }
    }

    #[test]
    fn denylist_hash_rule_matches_digest_not_name() {
        let table = DenyTable::parse(
            r#"
            [[linux]]
            id = "evil-hash"
            category = "terminal"
            match = "exact"
            sha256 = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
            "#,
        )
        .unwrap();
        let sha =
            parse_hash("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff").unwrap();
        let hit = classify(
            &table,
            &IdentityEvidence {
                identity: &linux("org.example.Notes", "/usr/bin/notes", Some(sha)),
                display_name: "Notes",
            },
            &[],
        );
        assert!(matches!(
            hit,
            IdentityVerdict::Denied {
                kind: DenyKind::Builtin(DenyCategory::Terminal),
                ..
            }
        ));
        let miss = classify(
            &table,
            &IdentityEvidence {
                identity: &linux("football", "/usr/bin/football", Some([9; 32])),
                display_name: "football",
            },
            &[],
        );
        assert_eq!(miss, IdentityVerdict::Clear);
    }

    #[test]
    fn denylist_substring_and_foreign_platform_do_not_match() {
        let table = DenyTable::parse(
            r#"
            [[linux]]
            id = "foot"
            category = "terminal"
            match = "exact"
            [[macos]]
            id = "com.apple.Terminal"
            category = "terminal"
            match = "exact"
            "#,
        )
        .unwrap();
        let football = classify(
            &table,
            &IdentityEvidence {
                identity: &linux("football", "/usr/bin/football", None),
                display_name: "football",
            },
            &[],
        );
        assert_eq!(football, IdentityVerdict::Clear);
        let windows = AppIdentity {
            platform: Platform::Windows,
            key: "com.apple.Terminal".into(),
            team_id: None,
            publisher: None,
            sha256: None,
            signature: SignatureState::Unsigned,
            image_path: "C:\\Notes\\Notes.exe".into(),
        };
        let foreign = classify(
            &table,
            &IdentityEvidence {
                identity: &windows,
                display_name: "Notes",
            },
            &[],
        );
        assert_eq!(foreign, IdentityVerdict::Clear);
    }
}
