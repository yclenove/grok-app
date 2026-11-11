//! Fixed init command and the literal-match dispatcher. No eval.

use crate::budget::{DISPATCHER_CAP, GUEST_DIR, UPLOAD_CAP};
use crate::sha256::{is_hex32, is_hex64};

const INIT: &str = "\
set -eu
set -f
umask 077
d='__DIR__'
digest='__DIGEST__'
auth='__AUTH__'
share='__SHARE__'
sub='__SUB__'
if [ -e \"$d\" ] || [ -L \"$d\" ]; then printf '%s\\n' fail-exists; exit 11; fi
/bin/mkdir -m 0700 \"$d\" || { printf '%s\\n' fail-mkdir; exit 12; }
/bin/dd if=/dev/stdin of=\"$d/dispatcher.upload\" bs=4096 count=64 >/dev/null 2>&1 || { printf '%s\\n' fail-dd; exit 13; }
/bin/mv \"$d/dispatcher.upload\" \"$d/dispatcher\" || { printf '%s\\n' fail-mv; exit 14; }
/bin/chmod 0700 \"$d/dispatcher\" || { printf '%s\\n' fail-chmod; exit 15; }
got=$(/usr/bin/shasum -a 256 \"$d/dispatcher\") || { printf '%s\\n' fail-shasum; exit 16; }
set -- $got
[ \"$1\" = \"$digest\" ] || { printf '%s\\n' fail-digest; exit 17; }
sshdir=\"$HOME/.ssh\"
if [ -L \"$sshdir\" ]; then printf '%s\\n' fail-ssh; exit 18; fi
if [ -e \"$sshdir\" ] && [ ! -d \"$sshdir\" ]; then printf '%s\\n' fail-ssh; exit 18; fi
if [ ! -d \"$sshdir\" ]; then /bin/mkdir -m 0700 \"$sshdir\" || { printf '%s\\n' fail-ssh; exit 18; }; fi
ak=\"$sshdir/authorized_keys\"
if [ -L \"$ak\" ]; then printf '%s\\n' fail-auth; exit 19; fi
if [ ! -e \"$ak\" ]; then : > \"$ak\" || { printf '%s\\n' fail-auth; exit 19; }; fi
/bin/chmod 0600 \"$ak\" || { printf '%s\\n' fail-auth; exit 19; }
printf '%s\\n' \"$auth\" >> \"$ak\" || { printf '%s\\n' fail-auth; exit 19; }
printf '%s\\n' \"$share\" > \"$d/share-name\" || { printf '%s\\n' fail-share; exit 20; }
/bin/chmod 0600 \"$d/share-name\" || { printf '%s\\n' fail-share; exit 20; }
printf '%s\\n' \"$sub\" > \"$d/subcase\" || { printf '%s\\n' fail-sub; exit 21; }
/bin/chmod 0600 \"$d/subcase\" || { printf '%s\\n' fail-sub; exit 21; }
printf 'init-ok\\n'
";

pub fn subcase_ok(s: &str) -> bool {
    matches!(
        s,
        "rename-latency" | "file-sync" | "dir-sync" | "rename-boundary" | "subjects"
    )
}

pub fn path_token(s: &str) -> bool {
    s.starts_with('/')
        && s.len() <= 200
        && !s.contains("..")
        && !s.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '\'' | '"' | '\\' | '$' | '`' | ';' | '&' | '|' | '<' | '>'
                )
        })
}

pub fn build_init(
    dir: &str,
    digest: &str,
    auth: &str,
    share: &str,
    sub: &str,
) -> Result<String, &'static str> {
    if !path_token(dir) || !is_hex64(digest) || !is_hex32(share) || !subcase_ok(sub) {
        return Err("init");
    }
    if auth.contains('\'') || auth.contains('\n') || !auth.contains("restrict,command=") {
        return Err("init");
    }
    let script = INIT
        .replace("__DIR__", dir)
        .replace("__DIGEST__", digest)
        .replace("__AUTH__", auth)
        .replace("__SHARE__", share)
        .replace("__SUB__", sub);
    if script.contains("__DIR__")
        || script.contains("__DIGEST__")
        || script.len() > crate::budget::REMOTE_CAP
    {
        return Err("init");
    }
    Ok(script)
}

pub fn is_init_script(script: &str) -> bool {
    let Some(dir) = assign(script, "d") else {
        return false;
    };
    let Some(digest) = assign(script, "digest") else {
        return false;
    };
    let Some(auth) = assign(script, "auth") else {
        return false;
    };
    let Some(share) = assign(script, "share") else {
        return false;
    };
    let Some(sub) = assign(script, "sub") else {
        return false;
    };
    build_init(&dir, &digest, &auth, &share, &sub)
        .ok()
        .as_deref()
        == Some(script)
}

fn assign(script: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}='");
    let line = script.lines().find(|l| l.starts_with(&prefix))?;
    let rest = line.strip_prefix(&prefix)?;
    let value = rest.strip_suffix('\'')?;
    if value.contains('\'') {
        return None;
    }
    Some(value.to_string())
}

pub fn production_init(
    digest: &str,
    auth: &str,
    share: &str,
    sub: &str,
) -> Result<String, &'static str> {
    build_init(GUEST_DIR, digest, auth, share, sub)
}

pub fn whitelisted_remote(remote: &str) -> bool {
    matches!(
        remote,
        "upload C1" | "upload C2" | "upload C8" | "run C1" | "run C2" | "run C8"
    ) || verify_arg(remote).is_some()
}

pub fn verify_arg(remote: &str) -> Option<&str> {
    let hex = remote.strip_prefix("verify ")?;
    if is_hex64(hex) {
        Some(hex)
    } else {
        None
    }
}

pub fn remote_allowed(remote: &str, init: &str) -> bool {
    crate::ssh::remote_fits(remote) && (remote == init || whitelisted_remote(remote))
}

const DISPATCHER: &str = "\
#!/bin/sh
set -eu
set -f
umask 077
cmd=${SSH_ORIGINAL_COMMAND-}
action=''
case_name=''
hex=''
case \"$cmd\" in
\"upload C1\") action=upload; case_name=C1 ;;
\"upload C2\") action=upload; case_name=C2 ;;
\"upload C8\") action=upload; case_name=C8 ;;
\"run C1\") action=run; case_name=C1 ;;
\"run C2\") action=run; case_name=C2 ;;
\"run C8\") action=run; case_name=C8 ;;
\"verify \"*)
  action=verify
  hex=${cmd#\"verify \"}
  case \"$hex\" in
    *[!0-9a-f]*) printf '%s\\n' reject; exit 2 ;;
  esac
  [ \"${#hex}\" -eq 64 ] || { printf '%s\\n' reject; exit 2; }
  ;;
*)
  printf '%s\\n' reject
  exit 2
  ;;
esac
dir='__DIR__'
probe=\"$dir/probe\"
stamp=\"$dir/probe.sha256\"
tmp=\"$dir/probe.upload\"
if [ \"$action\" = upload ]; then
  if [ -L \"$dir\" ] || [ ! -d \"$dir\" ]; then printf '%s\\n' fail-dir; exit 3; fi
  if [ -L \"$probe\" ] || [ -e \"$probe\" ]; then printf '%s\\n' fail-exists; exit 3; fi
  if [ -L \"$tmp\" ] || [ -e \"$tmp\" ]; then printf '%s\\n' fail-exists; exit 3; fi
  /bin/dd if=/dev/stdin of=\"$tmp\" bs=4096 count=2049 >/dev/null 2>&1 || { printf '%s\\n' fail-dd; exit 4; }
  size=$(/usr/bin/wc -c < \"$tmp\") || { printf '%s\\n' fail-size; exit 4; }
  if [ \"$size\" -lt 1 ] || [ \"$size\" -gt __CAP__ ]; then /bin/rm -f \"$tmp\"; printf '%s\\n' fail-size; exit 4; fi
  /bin/mv \"$tmp\" \"$probe\" || { printf '%s\\n' fail-mv; exit 4; }
  /bin/chmod 0700 \"$probe\" || { printf '%s\\n' fail-chmod; exit 4; }
  printf '%s\\n' upload-ok
  exit 0
fi
if [ \"$action\" = verify ]; then
  if [ -L \"$probe\" ] || [ ! -f \"$probe\" ]; then printf '%s\\n' fail-probe; exit 5; fi
  got=$(/usr/bin/shasum -a 256 \"$probe\") || { printf '%s\\n' fail-shasum; exit 5; }
  set -- $got
  [ \"$1\" = \"$hex\" ] || { printf '%s\\n' fail-digest; exit 5; }
  printf '%s\\n' \"$hex\" > \"$stamp\" || { printf '%s\\n' fail-stamp; exit 5; }
  /bin/chmod 0600 \"$stamp\" || { printf '%s\\n' fail-stamp; exit 5; }
  printf '%s\\n' verify-ok
  exit 0
fi
if [ \"$action\" = run ]; then
  if [ -L \"$probe\" ] || [ ! -f \"$probe\" ]; then printf '%s\\n' fail-probe; exit 6; fi
  if [ -L \"$stamp\" ] || [ ! -f \"$stamp\" ]; then printf '%s\\n' fail-stamp; exit 6; fi
  live=$(/usr/bin/shasum -a 256 \"$probe\") || { printf '%s\\n' fail-shasum; exit 6; }
  set -- $live
  want=$(/bin/cat \"$stamp\") || { printf '%s\\n' fail-stamp; exit 6; }
  [ \"$1\" = \"$want\" ] || { printf '%s\\n' fail-digest; exit 6; }
  exec \"$probe\" guest \"$case_name\"
fi
printf '%s\\n' reject
exit 2
";

pub fn dispatcher_script(dir: &str) -> Result<String, &'static str> {
    let raw = DISPATCHER.replace("__CAP__", &UPLOAD_CAP.to_string());
    if !path_token(dir) {
        return Err("dispatcher");
    }
    let script = raw.replace("__DIR__", dir);
    if script.contains("__DIR__") || script.contains("__CAP__") || script.len() > DISPATCHER_CAP {
        return Err("dispatcher");
    }
    if script.contains("eval ") || script.contains("sh -c") || script.contains("fetch-result") {
        return Err("dispatcher");
    }
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256::hex_digest;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let home = std::env::var("HOME").unwrap();
        let p = std::path::PathBuf::from(format!("{home}/.s11m2a-disp-{}-{n}", std::process::id()));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }

    fn run_disp(
        script: &std::path::Path,
        dir: &std::path::Path,
        cmd: &str,
        stdin: &[u8],
    ) -> (i32, String) {
        let mut child = Command::new("/bin/sh");
        child
            .arg(script)
            .env_clear()
            .env("SSH_ORIGINAL_COMMAND", cmd)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut proc = child.spawn().unwrap();
        use std::io::Write;
        if let Some(mut input) = proc.stdin.take() {
            input.write_all(stdin).unwrap();
        }
        let out = proc.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(99),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    #[test]
    fn rejects_shell_payloads_and_completes_upload_verify_run() {
        let dir = scratch();
        let src = dispatcher_script(dir.to_str().unwrap()).unwrap();
        assert!(!src.contains("sh -c"));
        assert!(!src.contains("fetch-result"));
        assert!(!src.contains("guest-status"));
        assert!(!src.contains("sudo"));
        let script = dir.join("dispatcher");
        std::fs::write(&script, &src).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        for bad in [
            "run C1; id",
            "run C1\nid",
            "run C1$(id)",
            "run C1`id`",
            &"A".repeat(9000),
            "nope",
            "diag echo PWNED",
            "fetch-result C1",
            "upload C4",
            "upload C9",
            "verify xyz",
            "verify AABB",
        ] {
            let (code, out) = run_disp(&script, &dir, bad, b"");
            assert_ne!(code, 0, "{bad}");
            assert!(!out.contains("PWNED"), "{bad}: {out}");
            assert!(!out.contains("uid="), "{bad}: {out}");
        }
        let marker = dir.join("marker");
        let probe = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$2\" > '{}'\n",
            marker.display()
        );
        let digest = hex_digest(probe.as_bytes());
        let (code, out) = run_disp(&script, &dir, "upload C1", probe.as_bytes());
        assert_eq!((code, out.as_str()), (0, "upload-ok\n"));
        let (code, out) = run_disp(&script, &dir, &format!("verify {digest}"), b"");
        assert_eq!((code, out.as_str()), (0, "verify-ok\n"), "{out}");
        let (code, out) = run_disp(&script, &dir, "run C1", b"");
        assert_eq!(code, 0, "{out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "C1\n");
        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::remove_file(dir.join("probe"));
        let _ = std::fs::remove_file(dir.join("probe.sha256"));
        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn init_script_is_exact_and_fails_closed() {
        let body = "B".repeat(64);
        let auth = crate::ssh::auth_line(&body).unwrap();
        let share = "a".repeat(32);
        let built = build_init(
            "/Users/eval/.s11-0c",
            &"ab".repeat(32),
            &auth,
            &share,
            "file-sync",
        )
        .unwrap();
        assert!(is_init_script(&built));
        assert!(!is_init_script(&format!("{built}\necho extra\n")));
        assert!(build_init("/tmp/a b", &"ab".repeat(32), &auth, &share, "file-sync").is_err());
        let dir = scratch();
        let home = dir.join("home");
        std::fs::create_dir(&home).unwrap();
        let guest = dir.join("guest");
        let disp = dispatcher_script("/Users/eval/.s11-0c").unwrap();
        let digest = hex_digest(disp.as_bytes());
        let script =
            build_init(guest.to_str().unwrap(), &digest, &auth, &share, "subjects").unwrap();
        let path = dir.join("init.sh");
        std::fs::write(&path, &script).unwrap();
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(&path)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(disp.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(
            out.stdout,
            b"init-ok\n",
            "stderr {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success());
        let again = Command::new("/bin/sh")
            .arg(&path)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert_eq!(again.stdout, b"fail-exists\n");
        assert!(!again.status.success());
        let _ = std::fs::remove_file(home.join(".ssh/authorized_keys"));
        let _ = std::fs::remove_dir(home.join(".ssh"));
        let _ = std::fs::remove_dir(&home);
        for name in ["dispatcher", "share-name", "subcase", "init.sh"] {
            let _ = std::fs::remove_file(guest.join(name));
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&guest);
        let _ = std::fs::remove_dir(&dir);
    }
}
