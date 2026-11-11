//! Pinned host key and address checks. No keyscan and no trust-on-first-use.

use std::path::Path;

use crate::budget::{DISPATCHER_PATH, REMOTE_CAP};
use crate::osutil::{self, Id};

pub fn parse_pubkey_line(text: &str) -> Result<String, &'static str> {
    let mut nonempty = text.lines().filter(|l| !l.is_empty());
    let line = nonempty.next().ok_or("pinned-host-key")?;
    if nonempty.next().is_some() {
        return Err("pinned-host-key");
    }
    let mut parts = line.split_whitespace();
    let kind = parts.next().ok_or("pinned-host-key")?;
    let body = parts.next().ok_or("pinned-host-key")?;
    let extra = parts.next();
    if extra.is_some() && parts.next().is_some() {
        return Err("pinned-host-key");
    }
    validate_body(kind, body)?;
    Ok(body.to_string())
}

fn validate_body(kind: &str, body: &str) -> Result<(), &'static str> {
    if kind != "ssh-ed25519" {
        return Err("pinned-host-key");
    }
    if !(40..=128).contains(&body.len()) {
        return Err("pinned-host-key");
    }
    if !body
        .bytes()
        .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='))
    {
        return Err("pinned-host-key");
    }
    Ok(())
}

pub fn load_pinned(path: &Path) -> Result<String, &'static str> {
    let bytes = read_regular_nofollow(path, 4096)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "pinned-host-key")?;
    parse_pubkey_line(text)
}

pub fn auth_line(body: &str) -> Result<String, &'static str> {
    validate_body("ssh-ed25519", body)?;
    if body.contains('"') || body.contains('\'') {
        return Err("pinned-host-key");
    }
    Ok(format!(
        "restrict,command=\"{DISPATCHER_PATH}\" ssh-ed25519 {body}"
    ))
}

pub fn known_hosts_line(ip: &str, body: &str) -> Result<String, &'static str> {
    let ip = parse_single_ipv4(ip)?;
    validate_body("ssh-ed25519", body)?;
    Ok(format!("{ip} ssh-ed25519 {body}\n"))
}

pub fn parse_single_ipv4(text: &str) -> Result<String, &'static str> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() || text.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err("ip");
    }
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return Err("ip");
    }
    let mut nums = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) || part.len() > 3 {
            return Err("ip");
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err("ip");
        }
        let n: u16 = part.parse().map_err(|_| "ip")?;
        if n > 255 {
            return Err("ip");
        }
        nums[i] = n as u8;
    }
    if nums == [0, 0, 0, 0] || nums[0] == 255 {
        return Err("ip");
    }
    Ok(format!("{}.{}.{}.{}", nums[0], nums[1], nums[2], nums[3]))
}

pub fn remote_fits(remote: &str) -> bool {
    !remote.is_empty() && remote.len() <= REMOTE_CAP && !remote.as_bytes().contains(&0)
}

fn read_regular_nofollow(path: &Path, cap: usize) -> Result<Vec<u8>, &'static str> {
    let parent = path.parent().ok_or("pinned-host-key")?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("pinned-host-key")?;
    let (dir, _) = osutil::walk_dir(parent).map_err(|_| "pinned-host-key")?;
    let mut file = osutil::open_child(&dir, name).map_err(|_| "pinned-host-key")?;
    let id = osutil::id_of(&file).map_err(|_| "pinned-host-key")?;
    if !regular_key(&id, cap) {
        return Err("pinned-host-key");
    }
    osutil::read_fd_capped(&mut file, cap).map_err(|_| "pinned-host-key")
}

fn regular_key(id: &Id, cap: usize) -> bool {
    id.is_reg && id.nlink == 1 && id.size > 0 && id.size as usize <= cap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_one_key_and_one_address() {
        let body = "A".repeat(64);
        let line = format!("ssh-ed25519 {body} comment");
        assert_eq!(parse_pubkey_line(&line).unwrap(), body);
        assert!(parse_pubkey_line("ssh-rsa AAAA").is_err());
        assert!(parse_pubkey_line(&format!("ssh-ed25519 {}", "A".repeat(10))).is_err());
        assert!(parse_pubkey_line("ssh-ed25519 not base64!!!!").is_err());
        let auth = auth_line(&body).unwrap();
        assert!(auth.starts_with("restrict,command=\"/Users/eval/.s11-0c/dispatcher\" "));
        assert_eq!(
            known_hosts_line("203.0.113.10\n", &body).unwrap(),
            format!("203.0.113.10 ssh-ed25519 {body}\n")
        );
        assert!(parse_single_ipv4("203.0.113.10").is_ok());
        assert!(parse_single_ipv4("01.2.3.4").is_err());
        assert!(parse_single_ipv4("0.0.0.0").is_err());
        assert!(parse_single_ipv4("255.1.2.3").is_err());
        assert!(parse_single_ipv4("203.0.113.10\n10.0.0.2\n").is_err());
        assert!(parse_single_ipv4("203.0.113").is_err());
    }
}
