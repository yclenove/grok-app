//! Fixed guest dispatcher. SSH_ORIGINAL_COMMAND must be one whitelist entry.
//! The script is generated from the same table the host uses to send commands.

use crate::budget::{DISPATCHER_CAP, GUEST_DIR, UPLOAD_CAP};

pub const CASES: &[&str] = &["C1", "C2", "C4", "C8"];

pub fn whitelisted_remote(cmd: &str) -> bool {
    if let Some(rest) = cmd.strip_prefix("upload ") {
        return CASES.contains(&rest);
    }
    if let Some(rest) = cmd.strip_prefix("run ") {
        return CASES.contains(&rest);
    }
    if cmd == "fetch-result" {
        return true;
    }
    if let Some(rest) = cmd.strip_prefix("verify ") {
        return crate::sha256::is_hex64(rest) && cmd == format!("verify {rest}");
    }
    false
}

pub fn dispatcher_script(dir: &str) -> Result<String, &'static str> {
    if !dir.starts_with('/') || dir.contains('\'') || dir.contains('\n') || dir.contains(' ') {
        return Err("dispatcher-dir");
    }
    // One case word: a quoted prefix plus a 64-hex glob. A bare space is a syntax error.
    let verify_arm = format!("\"verify \"{}", "[0-9a-f]".repeat(64));
    let script = format!(
        r#"#!/bin/sh
set -eu
set -f
umask 077
DIR='{dir}'
MAX={upload}
PROBE="$DIR/probe"
STAMP="$DIR/verified"
cmd=${{SSH_ORIGINAL_COMMAND-}}
case "$cmd" in
  "upload C1"|"upload C2"|"upload C4"|"upload C8")
    tmp="$DIR/probe.upload"
    if [ -L "$DIR" ] || [ -L "$tmp" ] || [ -e "$tmp" ] || [ -e "$PROBE" ] || [ -L "$PROBE" ]; then
      exit 1
    fi
    /bin/dd bs=4096 count=2049 of="$tmp" 2>/dev/null || exit 1
    size=$(/usr/bin/wc -c < "$tmp")
    if [ "$size" -gt "$MAX" ] || [ "$size" -lt 1 ]; then
      /bin/rm -f "$tmp"
      exit 1
    fi
    /bin/mv "$tmp" "$PROBE"
    /bin/chmod 0700 "$PROBE"
    exit 0
    ;;
  "run C1"|"run C2"|"run C4"|"run C8")
    case_id=${{cmd#run }}
    if [ -L "$STAMP" ] || [ ! -f "$STAMP" ] || [ -L "$PROBE" ] || [ ! -f "$PROBE" ]; then
      exit 1
    fi
    stamp=$(/bin/cat "$STAMP")
    got=$(/usr/bin/shasum -a 256 "$PROBE")
    set -- $got
    if [ "$1" != "$stamp" ]; then
      exit 1
    fi
    exec "$PROBE" guest "$case_id"
    ;;
  "fetch-result")
    status="$DIR/guest-status"
    if [ -L "$status" ] || [ ! -f "$status" ]; then
      echo absent
      exit 0
    fi
    /bin/dd if="$status" bs=256 count=1 2>/dev/null
    exit 0
    ;;
  {verify_arm})
    hash=${{cmd#verify }}
    case "$hash" in
      *[!0-9a-f]*) exit 1 ;;
    esac
    if [ -L "$PROBE" ] || [ ! -f "$PROBE" ]; then
      exit 1
    fi
    got=$(/usr/bin/shasum -a 256 "$PROBE")
    set -- $got
    if [ "$1" != "$hash" ]; then
      exit 1
    fi
    printf '%s\n' "$hash" > "$STAMP"
    /bin/chmod 0600 "$STAMP"
    exit 0
    ;;
  *)
    echo rejected >&2
    exit 1
    ;;
esac
exit 1
"#,
        dir = dir,
        upload = UPLOAD_CAP,
    );
    if script.len() > DISPATCHER_CAP {
        return Err("dispatcher-cap");
    }
    static_check(&script)?;
    Ok(script)
}

pub fn production_script() -> Result<String, &'static str> {
    dispatcher_script(GUEST_DIR)
}

/// Shellcheck-style scan of the generated script. This is not a full shell parser.
pub fn static_check(script: &str) -> Result<(), &'static str> {
    if !script.starts_with("#!/bin/sh\n") {
        return Err("dispatcher-shebang");
    }
    for needle in ["set -eu\n", "set -f\n", "umask 077\n"] {
        if !script.contains(needle) {
            return Err("dispatcher-flags");
        }
    }
    let lower = script.to_ascii_lowercase();
    for banned in [
        "eval ",
        "eval\t",
        "`",
        "sudo",
        "osascript",
        "sysadminctl",
        "dscl",
        "curl",
        "wget",
        "hdiutil",
        " tart",
        "ssh ",
        "rm -rf",
        "chmod 777",
        "chmod 666",
    ] {
        if lower.contains(banned) {
            return Err("dispatcher-banned");
        }
    }
    let reject_arms = script.matches("\n  *)\n").count();
    if reject_arms != 1 {
        return Err("dispatcher-default");
    }
    for case_id in CASES {
        if !script.contains(&format!("\"upload {case_id}\""))
            || !script.contains(&format!("\"run {case_id}\""))
        {
            return Err("dispatcher-whitelist");
        }
    }
    if !script.contains("\"fetch-result\"") || !script.contains("[0-9a-f]".repeat(64).as_str()) {
        return Err("dispatcher-whitelist");
    }
    if !script.contains(&format!("MAX={UPLOAD_CAP}")) {
        return Err("dispatcher-cap");
    }
    let mut rest = script;
    while let Some(at) = rest.find("$(") {
        let after = &rest[at + 2..];
        let ok = after.starts_with("/usr/bin/shasum")
            || after.starts_with("/usr/bin/wc")
            || after.starts_with("/bin/cat");
        if !ok {
            return Err("dispatcher-subst");
        }
        rest = after;
    }
    let mut quotes = 0;
    for ch in script.chars() {
        if ch == '\'' {
            quotes ^= 1;
        }
    }
    if quotes != 0 {
        return Err("dispatcher-quote");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn whitelist_and_static_check() {
        let script = production_script().unwrap();
        static_check(&script).unwrap();
        assert!(script.contains("DIR='/Users/eval/.s11-0c'"));
        assert!(whitelisted_remote("upload C1"));
        assert!(whitelisted_remote("run C8"));
        assert!(whitelisted_remote("fetch-result"));
        assert!(whitelisted_remote(&format!("verify {}", "ab".repeat(32))));
        assert!(!whitelisted_remote("upload C3"));
        assert!(!whitelisted_remote("upload C1; id"));
        assert!(!whitelisted_remote("run C1 extra"));
        assert!(!whitelisted_remote("verify zz"));
        assert!(!whitelisted_remote("sh -c id"));
        assert!(static_check("#!/bin/sh\neval $cmd\n").is_err());
    }

    fn run_script(script: &str, cmd: &str, stdin: &[u8]) -> (i32, Vec<u8>) {
        let dir = std::env::temp_dir().join(format!("s11-disp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("dispatcher");
        std::fs::write(&path, script).unwrap();
        let mut child = Command::new("/bin/sh")
            .arg(&path)
            .env_clear()
            .env("SSH_ORIGINAL_COMMAND", cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A rejected command exits before reading stdin. EPIPE is that rejection, not a harness bug.
        {
            let mut input = child.stdin.take().unwrap();
            match input.write_all(stdin) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => {}
                Err(err) => panic!("dispatcher stdin: {err}"),
            }
        }
        let out = child.wait_with_output().unwrap();
        let code = out.status.code().unwrap_or(127);
        let _ = std::fs::remove_dir_all(&dir);
        (code, out.stdout)
    }

    #[test]
    fn shell_stub_accepts_only_the_whitelist() {
        let dir = std::env::temp_dir().join(format!("s11-disp-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let script = dispatcher_script(&dir.to_string_lossy()).unwrap();
        let (code, _) = run_script(&script, "id", b"");
        assert_ne!(code, 0);
        let (code, _) = run_script(&script, "upload C1; rm -rf /", b"abc");
        assert_ne!(code, 0);
        let (code, _) = run_script(&script, "run C1", b"");
        assert_ne!(code, 0);
        let payload = b"probe-bytes";
        let (code, _) = run_script(&script, "upload C1", payload);
        assert_eq!(code, 0, "upload");
        assert_eq!(std::fs::read(dir.join("probe")).unwrap(), payload);
        let (code, _) = run_script(&script, "upload C9", b"nope");
        assert_ne!(code, 0);
        let (code, stdout) = run_script(&script, "fetch-result", b"");
        assert_eq!(code, 0);
        assert_eq!(stdout, b"absent\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
