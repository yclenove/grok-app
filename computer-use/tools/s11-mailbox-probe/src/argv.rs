//! Allow-listed argv. The program set is closed: tart, ssh, ssh-keygen, dscl.

use std::path::Path;

use crate::budget::{CONNECT_SEC, GOLDEN, GUEST_ROLE};

pub fn closed_program(argv: &[String]) -> Result<(), &'static str> {
    let Some(first) = argv.first() else {
        return Err("argv");
    };
    if !first.starts_with('/') || first.contains("..") || first.as_bytes().contains(&0) {
        return Err("program");
    }
    let base = first.rsplit('/').next().unwrap_or(first);
    let ok = first == "/usr/bin/ssh"
        || first == "/usr/bin/ssh-keygen"
        || first == "/usr/bin/dscl"
        || base == "tart";
    if ok {
        Ok(())
    } else {
        Err("program")
    }
}

pub fn tart_list(tart: &Path) -> Vec<String> {
    vec![
        tart.display().to_string(),
        "list".into(),
        "--format".into(),
        "json".into(),
    ]
}

pub fn tart_clone(tart: &Path, name: &str) -> Vec<String> {
    vec![
        tart.display().to_string(),
        "clone".into(),
        GOLDEN.into(),
        name.into(),
    ]
}

pub fn tart_run(tart: &Path, name: &str, share: &str, mailbox: &Path) -> Vec<String> {
    vec![
        tart.display().to_string(),
        "run".into(),
        name.into(),
        "--no-graphics".into(),
        "--net-softnet".into(),
        "--dir".into(),
        format!("{share}:{}", mailbox.display()),
    ]
}

pub fn tart_stop(tart: &Path, name: &str) -> Vec<String> {
    vec![tart.display().to_string(), "stop".into(), name.into()]
}

pub fn tart_delete(tart: &Path, name: &str) -> Vec<String> {
    vec![tart.display().to_string(), "delete".into(), name.into()]
}

pub fn tart_ip(tart: &Path, name: &str) -> Vec<String> {
    vec![tart.display().to_string(), "ip".into(), name.into()]
}

pub fn ssh_keygen(key: &Path) -> Vec<String> {
    vec![
        "/usr/bin/ssh-keygen".into(),
        "-t".into(),
        "ed25519".into(),
        "-f".into(),
        key.display().to_string(),
        "-N".into(),
        String::new(),
        "-q".into(),
    ]
}

pub fn ssh(key: &Path, known_hosts: &Path, ip: &str, remote: &str) -> Vec<String> {
    vec![
        "/usr/bin/ssh".into(),
        "-F".into(),
        "/dev/null".into(),
        "-T".into(),
        "-i".into(),
        key.display().to_string(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
        "-o".into(),
        "GlobalKnownHostsFile=/dev/null".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        "HostKeyAlgorithms=ssh-ed25519".into(),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
        "-o".into(),
        "IdentityAgent=none".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ClearAllForwardings=yes".into(),
        "-o".into(),
        "PreferredAuthentications=publickey".into(),
        "-o".into(),
        "PubkeyAuthentication=yes".into(),
        "-o".into(),
        "PasswordAuthentication=no".into(),
        "-o".into(),
        "KbdInteractiveAuthentication=no".into(),
        "-o".into(),
        "NumberOfPasswordPrompts=0".into(),
        "-o".into(),
        "RequestTTY=no".into(),
        "-o".into(),
        "ForwardAgent=no".into(),
        "-o".into(),
        "ForwardX11=no".into(),
        "-o".into(),
        "ForwardX11Trusted=no".into(),
        "-o".into(),
        "GatewayPorts=no".into(),
        "-o".into(),
        "PermitLocalCommand=no".into(),
        "-o".into(),
        "ProxyCommand=none".into(),
        "-o".into(),
        "ProxyJump=none".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_SEC}"),
        "-o".into(),
        format!("User={GUEST_ROLE}"),
        "--".into(),
        ip.into(),
        remote.into(),
    ]
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn dscl_users() -> Vec<String> {
    vec![
        "/usr/bin/dscl".into(),
        ".".into(),
        "-list".into(),
        "/Users".into(),
        "UniqueID".into(),
    ]
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn dscl_admin_group() -> Vec<String> {
    vec![
        "/usr/bin/dscl".into(),
        ".".into(),
        "-read".into(),
        "/Groups/admin".into(),
        "GroupMembership".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_set_and_ssh_checklist() {
        let tart = Path::new("/home/claude/bin/tart");
        assert!(closed_program(&tart_list(tart)).is_ok());
        assert!(closed_program(&ssh_keygen(Path::new("/k"))).is_ok());
        assert!(closed_program(&dscl_users()).is_ok());
        assert!(closed_program(&["/usr/bin/hdiutil".into(), "info".into()]).is_err());
        assert!(closed_program(&["/bin/sh".into(), "-c".into()]).is_err());
        assert!(closed_program(&["/usr/bin/ssh-keyscan".into()]).is_err());
        assert!(closed_program(&["/usr/bin/sudo".into()]).is_err());
        assert!(closed_program(&["tart".into(), "list".into()]).is_err());
        assert!(closed_program(&["/tmp/../bin/tart".into()]).is_err());
        let run = tart_run(tart, "s11-0c-aa", "bbbb", Path::new("/m"));
        assert!(run.iter().any(|a| a == "--no-graphics"));
        assert!(run.iter().any(|a| a == "--net-softnet"));
        assert!(!run.iter().any(|a| a.contains("softnet-block")));
        assert_eq!(run.last().map(String::as_str), Some("bbbb:/m"));
        let argv = ssh(
            Path::new("/priv/id"),
            Path::new("/priv/known_hosts"),
            "203.0.113.10",
            "upload C1",
        );
        let joined = argv.join("\n");
        for needle in [
            "-F",
            "/dev/null",
            "UserKnownHostsFile=/priv/known_hosts",
            "GlobalKnownHostsFile=/dev/null",
            "StrictHostKeyChecking=yes",
            "HostKeyAlgorithms=ssh-ed25519",
            "IdentitiesOnly=yes",
            "IdentityAgent=none",
            "BatchMode=yes",
            "ClearAllForwardings=yes",
            "PreferredAuthentications=publickey",
            "PubkeyAuthentication=yes",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "NumberOfPasswordPrompts=0",
            "RequestTTY=no",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ForwardX11Trusted=no",
            "GatewayPorts=no",
            "PermitLocalCommand=no",
            "ProxyCommand=none",
            "ProxyJump=none",
            "ConnectTimeout=5",
            "User=eval",
            "-T",
        ] {
            assert!(joined.contains(needle), "{needle}");
        }
        assert_eq!(argv.last().map(String::as_str), Some("upload C1"));
        assert!(closed_program(&argv).is_ok());
    }
}
