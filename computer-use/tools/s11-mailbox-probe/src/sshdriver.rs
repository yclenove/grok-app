//! Laboratory SSH. The eval key is used once to install the dispatcher.
//! Later commands use the one-time key and a whitelisted remote command.

use std::path::Path;

use crate::argv;
use crate::budget::{DISPATCHER_PATH, GUEST_DIR};
use crate::dispatcher::{self, whitelisted_remote};
use crate::runner::Cmd;

pub fn eval_key(home: &Path) -> Result<std::path::PathBuf, &'static str> {
    if !home.is_absolute() {
        return Err("home");
    }
    Ok(home.join(".ssh").join("id_ed25519_tart"))
}

pub fn validate_pubkey(line: &str) -> Result<String, &'static str> {
    let mut parts = line.split_whitespace();
    let kind = parts.next().ok_or("pubkey")?;
    let body = parts.next().ok_or("pubkey")?;
    if kind != "ssh-ed25519" {
        return Err("pubkey");
    }
    if !(40..=128).contains(&body.len())
        || !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return Err("pubkey");
    }
    Ok(format!("ssh-ed25519 {body}"))
}

pub fn auth_line(pubkey: &str) -> Result<String, &'static str> {
    let key = validate_pubkey(pubkey)?;
    Ok(format!("restrict,command=\"{DISPATCHER_PATH}\" {key}"))
}

pub fn subcase_ok(sub: &str) -> bool {
    matches!(
        sub,
        "rename-latency"
            | "file-sync"
            | "dir-sync"
            | "rename-boundary"
            | "create"
            | "attach"
            | "subjects"
    )
}

pub fn install_remote(share: &str, subcase: &str, pubkey: &str) -> Result<String, &'static str> {
    if !crate::sha256::is_hex32(share) || !subcase_ok(subcase) {
        return Err("install-token");
    }
    let line = auth_line(pubkey)?;
    if line.contains('\'') {
        return Err("install-token");
    }
    Ok(format!(
        r#"umask 077
if [ -L {dir} ]; then exit 1; fi
/bin/mkdir -p -m 0700 {dir}
if [ -L {dir}/.ssh-parent ]; then exit 1; fi
tmp={dir}/dispatcher.upload
if [ -e "$tmp" ] || [ -L "$tmp" ]; then exit 1; fi
/bin/dd bs=4096 count=64 of="$tmp" 2>/dev/null || exit 1
size=$(/usr/bin/wc -c < "$tmp")
if [ "$size" -gt 262144 ] || [ "$size" -lt 32 ]; then /bin/rm -f "$tmp"; exit 1; fi
/bin/mv "$tmp" {dispatcher}
/bin/chmod 0700 {dispatcher}
printf '%s\n' '{share}' > {dir}/share-name
printf '%s\n' '{sub}' > {dir}/subcase
/bin/chmod 0600 {dir}/share-name {dir}/subcase
if [ -L /Users/eval/.ssh ]; then exit 1; fi
if [ ! -d /Users/eval/.ssh ]; then /bin/mkdir -m 0700 /Users/eval/.ssh || exit 1; fi
ak=/Users/eval/.ssh/authorized_keys
if [ -L "$ak" ]; then exit 1; fi
/usr/bin/touch "$ak"
/bin/chmod 0600 "$ak"
line='{line}'
if ! /usr/bin/grep -F -x -q "$line" "$ak"; then printf '%s\n' "$line" >> "$ak"; fi
exit 0
"#,
        dir = GUEST_DIR,
        dispatcher = DISPATCHER_PATH,
        share = share,
        sub = subcase,
        line = line,
    ))
}

pub fn parse_ipv4(text: &str) -> Result<String, &'static str> {
    let mut found = None;
    for tok in text.split_whitespace() {
        if is_ipv4(tok) {
            if found.is_some() {
                return Err("ip-ambiguous");
            }
            found = Some(tok.to_string());
        }
    }
    found.ok_or("ip-missing")
}

fn is_ipv4(tok: &str) -> bool {
    let parts: Vec<&str> = tok.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in parts {
        if part.is_empty()
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|b| b.is_ascii_digit())
        {
            return false;
        }
        let n: u32 = match part.parse() {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n > 255 {
            return false;
        }
    }
    tok != "0.0.0.0" && !tok.starts_with("255.")
}

pub fn parse_keyscan(stdout: &str, ip: &str) -> Result<String, &'static str> {
    let mut found = None;
    for line in stdout.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let host = parts.next().ok_or("host-key-untrusted")?;
        let kind = parts.next().ok_or("host-key-untrusted")?;
        let key = parts.next().ok_or("host-key-untrusted")?;
        if parts.next().is_some() || host != ip || kind != "ssh-ed25519" {
            return Err("host-key-untrusted");
        }
        validate_pubkey(&format!("ssh-ed25519 {key}"))?;
        if found.is_some() {
            return Err("host-key-ambiguous");
        }
        found = Some(format!("{ip} ssh-ed25519 {key}"));
    }
    found.ok_or("host-key-missing")
}

pub fn ssh_cmd(
    key: &Path,
    known: &Path,
    ip: &str,
    remote: &str,
    stdin: Vec<u8>,
    timeout_ms: u64,
) -> Result<Cmd, &'static str> {
    if !whitelisted_remote(remote) && !remote.starts_with("umask 077\n") {
        return Err("remote-rejected");
    }
    if remote.starts_with("umask 077\n") && remote.contains("sudo") {
        return Err("remote-rejected");
    }
    Ok(Cmd::new(argv::ssh(key, known, ip, remote), timeout_ms).with_stdin(stdin))
}

pub fn production_dispatcher() -> Result<Vec<u8>, &'static str> {
    Ok(dispatcher::production_script()?.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pubkey_and_install_reject_injection() {
        let good = format!("ssh-ed25519 {}", "A".repeat(68));
        let line = auth_line(&good).unwrap();
        assert!(
            line.starts_with("restrict,command=\"/Users/eval/.s11-0c/dispatcher\" ssh-ed25519 ")
        );
        assert!(auth_line("ssh-ed25519 $(id)").is_err());
        assert!(auth_line("ssh-rsa AAAA").is_err());
        let remote =
            install_remote("0123456789abcdef0123456789abcdef", "file-sync", &good).unwrap();
        assert!(remote.contains("restrict,command="));
        assert!(!remote.contains("sudo"));
        assert!(install_remote(
            "0123456789abcdef0123456789abcdef",
            "file-sync",
            "ssh-ed25519 bad;id"
        )
        .is_err());
        assert_eq!(parse_ipv4("203.0.113.10\n").unwrap(), "203.0.113.10");
        assert!(parse_ipv4("203.0.113.10 198.51.100.2").is_err());
        assert!(parse_ipv4("not-an-ip").is_err());
        let scan = parse_keyscan(
            &format!("203.0.113.10 ssh-ed25519 {}\n", "A".repeat(68)),
            "203.0.113.10",
        )
        .unwrap();
        assert!(scan.contains("ssh-ed25519"));
        assert!(parse_keyscan(
            "# comment\n1.2.3.4 ssh-ed25519 AAAA\n5.6.7.8 ssh-ed25519 AAAA\n",
            "1.2.3.4"
        )
        .is_err());
    }
}
