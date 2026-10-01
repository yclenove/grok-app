//! Worker process environment and Bearer token rules.

pub fn csprng_bearer_token() -> String {
    let mut raw = [0u8; 32];
    getrandom::getrandom(&mut raw).expect("os csprng");
    raw.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn keep_worker_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "OPENAI_API_KEY"
            | "GROK_API_KEY"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "ALL_PROXY"
            | "NODE_OPTIONS"
    ) {
        return false;
    }
    if key.starts_with("GROK_CU_") {
        return true;
    }
    const ALLOW: &[&str] = &[
        "SYSTEMROOT",
        "WINDIR",
        "TEMP",
        "TMP",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "COMMONPROGRAMFILES",
        "COMSPEC",
        "PATHEXT",
        "PATH",
        "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE",
        "HOME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "USER",
        "LOGNAME",
    ];
    ALLOW
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looks_like_uuid_v4(bytes: &[u8]) -> bool {
        bytes.len() == 16 && (bytes[6] >> 4) == 4 && (bytes[8] & 0xc0) == 0x80
    }

    #[test]
    fn bearer_is_32_csprng_bytes_not_a_uuid_string() {
        let token = csprng_bearer_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!token.contains('-'));
        assert_ne!(token, csprng_bearer_token());
        let mut uuidish = 0;
        for _ in 0..24 {
            let hex = csprng_bearer_token();
            let raw = (0..32)
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
                .collect::<Vec<_>>();
            if looks_like_uuid_v4(&raw[..16]) && looks_like_uuid_v4(&raw[16..]) {
                uuidish += 1;
            }
        }
        assert!(
            uuidish <= 2,
            "bearer still looks like concatenated UUID v4 blobs ({uuidish}/24)"
        );
    }

    #[test]
    fn worker_env_drops_api_keys_and_proxies() {
        assert!(!keep_worker_env_key("OPENAI_API_KEY"));
        assert!(!keep_worker_env_key("GROK_API_KEY"));
        assert!(!keep_worker_env_key("HTTP_PROXY"));
        assert!(!keep_worker_env_key("NODE_OPTIONS"));
        assert!(keep_worker_env_key("GROK_CU_BROWSER_TOKEN"));
        assert!(keep_worker_env_key("SystemRoot") || keep_worker_env_key("HOME"));
        assert!(keep_worker_env_key("PATH"));
    }
}
