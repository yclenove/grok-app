use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_s11-mailbox-probe"))
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("s11-bounded-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn c7ops_blocks_one_op_and_exits() {
    let dir = scratch("c7block");
    let fifo = dir.join("c7-r0");
    let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(made.success(), "mkfifo");
    for name in ["c7-r1", "c7-r2", "c7-r3", "c7-w0", "c7-w1", "c7-w2", "c7-w3"] {
        fs::write(dir.join(name), b"v=1 op=empty seq=0").unwrap();
    }
    let started = Instant::now();
    let out = Command::new(bin()).args(["c7ops", "--dir", dir.to_str().unwrap(), "--role", "guest", "--ms", "200"]).output().unwrap();
    let elapsed = started.elapsed();
    assert!(out.status.success(), "status {:?} stderr {}", out.status, String::from_utf8_lossy(&out.stderr));
    assert!(elapsed < Duration::from_secs(3), "elapsed {elapsed:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ops=1 "), "{stdout}");
    assert!(stdout.contains("ok=0"), "{stdout}");
    assert!(stdout.contains("hang=1"), "{stdout}");
    assert_eq!(fs::read(dir.join("c7-w0")).unwrap(), b"v=1 op=empty seq=0");
    assert!(!dir.join("c7-w0.tmp").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hold_nonzero_is_killable_after_held() {
    let dir = scratch("holdkill");
    fs::write(dir.join("c6-held"), b"slot").unwrap();
    let mut child = Command::new(bin()).args(["hold", "--dir", dir.to_str().unwrap(), "--role", "guest", "--kind", "file", "--secs", "30"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let started = Instant::now();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("held=1\n"), "{line}");
    child.kill().unwrap();
    let _ = child.wait();
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");
    let _ = fs::remove_dir_all(&dir);
}
