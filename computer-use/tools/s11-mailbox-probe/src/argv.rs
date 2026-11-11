//! Fixed argv templates from ADR 0019 §2 and §5.
//! Every program path is absolute. Nothing here searches `PATH` or invokes a shell.

use std::path::Path;

use crate::budget::{CONNECT_SEC, GOLDEN, GUEST_ROLE};

pub fn hdiutil_create(seed: &Path, image: &Path) -> Vec<String> {
    vec![
        "/usr/bin/hdiutil".into(),
        "create".into(),
        "-size".into(),
        "64m".into(),
        "-fs".into(),
        "HFSX".into(),
        "-type".into(),
        "SPARSE".into(),
        "-layout".into(),
        "NONE".into(),
        "-volname".into(),
        "GCU-MAILBOX".into(),
        "-srcfolder".into(),
        seed.display().to_string(),
        image.display().to_string(),
    ]
}

pub fn hdiutil_attach(mount: &Path, image: &Path) -> Vec<String> {
    vec![
        "/usr/bin/hdiutil".into(),
        "attach".into(),
        "-plist".into(),
        "-noautoopen".into(),
        "-nobrowse".into(),
        "-owners".into(),
        "on".into(),
        "-mountOptions".into(),
        "rw,nosuid,noexec,nodev".into(),
        "-mountpoint".into(),
        mount.display().to_string(),
        image.display().to_string(),
    ]
}

pub fn hdiutil_imageinfo(image: &Path) -> Vec<String> {
    vec![
        "/usr/bin/hdiutil".into(),
        "imageinfo".into(),
        "-plist".into(),
        image.display().to_string(),
    ]
}

pub fn hdiutil_info() -> Vec<String> {
    vec!["/usr/bin/hdiutil".into(), "info".into(), "-plist".into()]
}

pub fn diskutil_info(device: &str) -> Vec<String> {
    vec![
        "/usr/sbin/diskutil".into(),
        "info".into(),
        "-plist".into(),
        device.into(),
    ]
}

pub fn mount_readonly() -> Vec<String> {
    vec!["/sbin/mount".into()]
}

pub fn ls_acl(path: &Path) -> Vec<String> {
    vec!["/bin/ls".into(), "-lde".into(), path.display().to_string()]
}

pub fn mdutil_status(mount: &Path) -> Vec<String> {
    vec![
        "/usr/bin/mdutil".into(),
        "-s".into(),
        mount.display().to_string(),
    ]
}

pub fn tmutil_addexclusion(path: &Path) -> Vec<String> {
    vec![
        "/usr/bin/tmutil".into(),
        "addexclusion".into(),
        path.display().to_string(),
    ]
}

pub fn tmutil_isexcluded(path: &Path) -> Vec<String> {
    vec![
        "/usr/bin/tmutil".into(),
        "isexcluded".into(),
        path.display().to_string(),
    ]
}

pub fn hdiutil_detach(device: &str) -> Vec<String> {
    vec!["/usr/bin/hdiutil".into(), "detach".into(), device.into()]
}

pub fn hdiutil_detach_force(device: &str) -> Vec<String> {
    vec![
        "/usr/bin/hdiutil".into(),
        "detach".into(),
        "-force".into(),
        device.into(),
    ]
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

/// §5 DEFAULT network laboratory exception, plus `--no-graphics` from the probe task.
/// No block and no allow.
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

pub fn ssh_keyscan(ip: &str) -> Vec<String> {
    vec![
        "/usr/bin/ssh-keyscan".into(),
        "-T".into(),
        CONNECT_SEC.to_string(),
        "-t".into(),
        "ed25519".into(),
        ip.into(),
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

pub fn dscl_users() -> Vec<String> {
    vec![
        "/usr/bin/dscl".into(),
        ".".into(),
        "-list".into(),
        "/Users".into(),
        "UniqueID".into(),
    ]
}

pub fn dscl_admin_group() -> Vec<String> {
    vec![
        "/usr/bin/dscl".into(),
        ".".into(),
        "-read".into(),
        "/Groups/admin".into(),
        "GroupMembership".into(),
    ]
}

/// Reject host argv that would elevate, search PATH, or open a local shell.
pub fn validate_host_argv(argv: &[String]) -> Result<(), &'static str> {
    if argv.is_empty() {
        return Err("empty-argv");
    }
    if argv.iter().any(|a| {
        let base = a.rsplit('/').next().unwrap_or(a);
        matches!(base, "sudo" | "su" | "osascript" | "sysadminctl")
    }) {
        return Err("elevation");
    }
    let first = &argv[0];
    if !first.starts_with('/') {
        return Err("argv-not-absolute");
    }
    if first.ends_with("/tmutil") && argv.iter().any(|a| a == "-p" || a == "-v") {
        return Err("tmutil-flag");
    }
    if first.ends_with("/mdutil") && argv.iter().any(|a| a == "-i") {
        return Err("mdutil-mutate");
    }
    if first.ends_with("/hdiutil")
        && argv
            .iter()
            .any(|a| a.contains("softnet-block") || a.contains("softnet-allow"))
    {
        return Err("unexpected-net");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn create_attach_and_run_match_adr_templates() {
        let seed = Path::new("/vol/seed");
        let image = Path::new("/vol/mailbox.sparseimage");
        let mount = Path::new("/vol/mnt");
        assert_eq!(
            hdiutil_create(seed, image),
            vec![
                "/usr/bin/hdiutil",
                "create",
                "-size",
                "64m",
                "-fs",
                "HFSX",
                "-type",
                "SPARSE",
                "-layout",
                "NONE",
                "-volname",
                "GCU-MAILBOX",
                "-srcfolder",
                "/vol/seed",
                "/vol/mailbox.sparseimage",
            ]
        );
        assert_eq!(
            hdiutil_attach(mount, image),
            vec![
                "/usr/bin/hdiutil",
                "attach",
                "-plist",
                "-noautoopen",
                "-nobrowse",
                "-owners",
                "on",
                "-mountOptions",
                "rw,nosuid,noexec,nodev",
                "-mountpoint",
                "/vol/mnt",
                "/vol/mailbox.sparseimage",
            ]
        );
        let tart = Path::new("/Users/eval/bin/tart");
        assert_eq!(
            tart_run(tart, "s11-0c-aa", "bbbb", Path::new("/vol/mnt/mailbox")),
            vec![
                "/Users/eval/bin/tart",
                "run",
                "s11-0c-aa",
                "--no-graphics",
                "--net-softnet",
                "--dir",
                "bbbb:/vol/mnt/mailbox",
            ]
        );
        let run = tart_run(tart, "n", "s", Path::new("/m"));
        assert!(!run
            .iter()
            .any(|a| a.contains("softnet-block") || a.contains("softnet-allow")));
        assert!(!tmutil_addexclusion(Path::new("/a"))
            .iter()
            .any(|a| a == "-p" || a == "-v"));
        assert_eq!(
            mdutil_status(Path::new("/m")),
            vec!["/usr/bin/mdutil", "-s", "/m"]
        );
    }

    #[test]
    fn ssh_options_cover_the_lab_checklist() {
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
            "-i",
            "/priv/id",
            "UserKnownHostsFile=/priv/known_hosts",
            "GlobalKnownHostsFile=/dev/null",
            "StrictHostKeyChecking=yes",
            "IdentitiesOnly=yes",
            "IdentityAgent=none",
            "BatchMode=yes",
            "ClearAllForwardings=yes",
            "PreferredAuthentications=publickey",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "RequestTTY=no",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ProxyCommand=none",
            "ProxyJump=none",
            "ConnectTimeout=5",
            "User=eval",
            "-T",
        ] {
            assert!(joined.contains(needle), "missing {needle}");
        }
        assert_eq!(argv.last().map(String::as_str), Some("upload C1"));
        assert!(validate_host_argv(&argv).is_ok());
        assert_eq!(
            validate_host_argv(&["sudo".into(), "id".into()]),
            Err("elevation")
        );
        assert_eq!(
            validate_host_argv(&["tart".into(), "list".into()]),
            Err("argv-not-absolute")
        );
        let mut bad = tmutil_addexclusion(Path::new("/a"));
        bad.push("-p".into());
        assert_eq!(validate_host_argv(&bad), Err("tmutil-flag"));
    }
}
