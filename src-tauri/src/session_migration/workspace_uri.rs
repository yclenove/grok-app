//! Lexical URI handling. Remote authorities and foreign path syntax are source
//! facts; parsing them never opens a share, starts WSL or connects over SSH.

use percent_encoding::percent_decode_str;
use url::Url;

use super::{
    classify_root, classify_root_with_host, infer_syntax, root_id_for, sha256_bytes, HostKind,
    PathSyntax, RootAvailability, WorkspaceRoot,
};

fn decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && (bytes.get(i + 1).is_none_or(|b| !b.is_ascii_hexdigit())
                || bytes.get(i + 2).is_none_or(|b| !b.is_ascii_hexdigit()))
        {
            return Err("invalid URI percent escape".into());
        }
    }
    let decoded = percent_decode_str(value)
        .decode_utf8()
        .map_err(|_| "URI path is not UTF-8")?
        .into_owned();
    if decoded.chars().any(char::is_control) {
        return Err("URI contains control characters".into());
    }
    Ok(decoded)
}

fn parts(raw: &str) -> Result<(String, String), String> {
    let (_, rest) = raw.split_once(':').ok_or("URI has no scheme")?;
    let (authority, path) = if let Some(rest) = rest.strip_prefix("//") {
        let split = rest.find('/').ok_or("URI has no absolute folder path")?;
        (&rest[..split], &rest[split..])
    } else {
        ("", rest)
    };
    let authority = decode(authority)?;
    if authority.contains(['/', '\\', '?', '#']) {
        return Err("invalid URI authority".into());
    }
    let path = decode(path)?;
    if !path.starts_with('/') {
        return Err("URI must contain an absolute slash-separated path".into());
    }
    Ok((authority, path))
}

fn foreign_root(path: String, host: HostKind, identity: String) -> WorkspaceRoot {
    let mut root = WorkspaceRoot {
        root_id: String::new(),
        display_name: None,
        path_syntax: infer_syntax(&path),
        host,
        original_path: path,
        source_uri: None,
        host_identity: Some(identity),
        unsupported_uri: false,
        availability: RootAvailability::Offline,
        evidence: vec!["uri:remote-metadata-only".into()],
    };
    // Do not equate SSH aliases, WSL distro names, or hosts from text similarity.
    let identity = format!(
        "workspace-uri-v1\0{:?}\0{}\0{}",
        root.host,
        root.host_identity.as_deref().unwrap_or_default(),
        root.original_path
    );
    root.root_id = format!("root:{}", sha256_bytes(identity.as_bytes()));
    root
}

fn file_root(authority: String, path: String) -> Result<WorkspaceRoot, String> {
    if authority.contains(['@', ':']) {
        return Err("file URI cannot contain credentials or a port".into());
    }
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        if path.trim_matches('/').is_empty() || path.contains('\\') {
            return Err("file share URI needs a share name".into());
        }
        let target = format!("\\\\{}{}", authority, path.replace('/', "\\"));
        let mut root = classify_root(&target);
        root.evidence.push("uri:network-share-not-probed".into());
        return Ok(root);
    }
    let bytes = path.as_bytes();
    let target = if bytes.len() >= 4
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && bytes[3] == b'/'
    {
        if path.contains('\\') {
            return Err("Windows file URI must use slash separators".into());
        }
        path[1..].replace('/', "\\")
    } else {
        path
    };
    Ok(classify_root_with_host(&target, HostKind::Local))
}

pub(super) fn parse(raw: &str) -> Result<WorkspaceRoot, String> {
    if raw.is_empty()
        || raw.len() > 8192
        || raw.chars().any(char::is_control)
        || raw.contains('\\')
        || raw.trim() != raw
    {
        return Err("invalid workspace folder URI".into());
    }
    // Validate escapes even for unsupported schemes, without rewriting the raw
    // URI or using URL's host-dependent file-path conversion.
    decode(raw)?;
    let url = Url::parse(raw).map_err(|_| "invalid workspace folder URI")?;
    let known = matches!(url.scheme(), "file" | "vscode-remote" | "ssh");
    if known && (url.query().is_some() || url.fragment().is_some()) {
        return Err("workspace folder URI cannot contain query or fragment".into());
    }
    let mut root = if known {
        let (authority, path) = parts(raw)?;
        match url.scheme() {
            "file" => file_root(authority, path)?,
            "vscode-remote" if authority.starts_with("ssh-remote+") && authority.len() > 11 => {
                foreign_root(path, HostKind::Ssh, authority)
            }
            "vscode-remote" if authority.starts_with("wsl+") && authority.len() > 4 => {
                foreign_root(path, HostKind::Wsl, authority)
            }
            "ssh" => {
                if url.host_str().is_none() || url.password().is_some() {
                    return Err("SSH URI needs a host and cannot include a password".into());
                }
                foreign_root(path, HostKind::Ssh, format!("ssh:{authority}"))
            }
            _ => unsupported(raw),
        }
    } else {
        unsupported(raw)
    };
    root.source_uri = Some(raw.to_string());
    root.evidence.push(format!("source-uri:{raw}"));
    Ok(root)
}

fn unsupported(raw: &str) -> WorkspaceRoot {
    WorkspaceRoot {
        root_id: root_id_for(HostKind::Unknown, raw),
        display_name: None,
        path_syntax: PathSyntax::Posix,
        host: HostKind::Unknown,
        original_path: raw.to_string(),
        source_uri: None,
        host_identity: None,
        unsupported_uri: true,
        availability: RootAvailability::Unresolved,
        evidence: vec!["uri:unsupported".into()],
    }
}
