//! Lexical source-path operations: imported paths may belong to another OS.
//! These helpers neither probe a path nor authorize it for local execution.

use super::PathSyntax;

pub(super) fn file_name(path: &str, syntax: PathSyntax) -> Option<&str> {
    let separators: &[char] = if syntax == PathSyntax::Windows {
        &['/', '\\']
    } else {
        &['/']
    };
    path.trim_end_matches(separators)
        .rsplit(separators)
        .next()
        .filter(|name| !name.is_empty())
}

pub(super) fn file_stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

pub(super) fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

pub(super) fn is_network_path(path: &str) -> bool {
    path.starts_with("//")
        || path
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
        || (path.starts_with(r"\\") && !path.starts_with(r"\\?\") && !path.starts_with(r"\\.\"))
}

/// Lexical UNC identity only; this never enumerates shares or WSL distros.
pub(super) fn unc_parts(path: &str) -> Option<(&str, &str)> {
    let tail = if path
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        &path[8..]
    } else if is_network_path(path) {
        &path[2..]
    } else {
        return None;
    };
    let (server, rest) = tail.split_once(['/', '\\'])?;
    let share = rest.split(['/', '\\']).next()?;
    (!server.is_empty() && !share.is_empty()).then_some((server, share))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_source_paths_are_not_host_dependent() {
        for path in [r"C:\repo", "C:/repo", r"\\server\share", "/home/repo"] {
            assert!(is_absolute(path), "{path}");
        }
        for path in ["C:relative", "repo", r"repo\child", ""] {
            assert!(!is_absolute(path), "{path}");
        }
    }

    #[test]
    fn source_names_allow_both_separators_and_trailing_separators() {
        for (path, syntax) in [
            (r"C:\repo\", PathSyntax::Windows),
            ("C:/repo/", PathSyntax::Windows),
            ("/home/repo/", PathSyntax::Posix),
            ("repo", PathSyntax::Posix),
        ] {
            assert_eq!(file_name(path, syntax), Some("repo"));
        }
        assert_eq!(file_name("/", PathSyntax::Posix), None);
        assert_eq!(file_name("", PathSyntax::Posix), None);
    }

    #[test]
    fn source_stem_keeps_dotfiles_and_literal_backslashes() {
        assert_eq!(file_stem("app.code-workspace"), "app");
        assert_eq!(file_stem(".hidden"), ".hidden");
        assert_eq!(file_stem("no-extension"), "no-extension");
        assert_eq!(file_stem(r"repo\name.code-workspace"), r"repo\name");
    }
}
