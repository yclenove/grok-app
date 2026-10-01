// Owned test process, NOT an NSIS/MSI installer. No external paths or settings.
fn main() {
    let exe = std::env::current_exe().unwrap();
    let root = exe.parent().unwrap();
    if std::fs::read(root.join("owned-dispatch-test-purpose"))
        .ok()
        .as_deref()
        != Some(b"grok updater owned process fixture v1")
    {
        std::process::exit(91);
    }
    let marker = root.join("installer-started");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(marker)
        .unwrap();
    writeln!(file, "{}", std::process::id()).unwrap();
    file.sync_all().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    while !root.join("installer-release").exists() {
        if std::time::Instant::now() >= deadline {
            std::process::exit(92);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let code: i32 = std::fs::read_to_string(root.join("installer-exit-code"))
        .unwrap()
        .parse()
        .unwrap();
    std::process::exit(code);
}
