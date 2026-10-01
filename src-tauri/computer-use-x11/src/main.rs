fn main() {
    #[cfg(target_os = "linux")]
    if let Some(code) = grok_computer_use_x11::clipboard::keeper_entrypoint() {
        std::process::exit(code);
    }
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--clipboard-keeper-parent-fixture") {
        if let Err(error) = grok_computer_use_x11::clipboard::run_keeper_parent_fixture(
            &std::env::args().nth(2).unwrap_or_default(),
        ) {
            eprintln!("keeper parent fixture failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    let result = if std::env::args().any(|arg| arg == "--clipboard") {
        grok_computer_use_x11::clipboard::run_clipboard_selftest()
    } else if std::env::args().any(|arg| arg == "--accessibility") {
        grok_computer_use_x11::run_semantic_selftest()
    } else {
        grok_computer_use_x11::run_native_selftest()
    };
    #[cfg(target_os = "linux")]
    match result {
        Ok(()) => println!(
            "native X11 adapter acceptance: PASS (owned Xvfb, not GNOME Wayland or installed App)"
        ),
        Err(error) => {
            eprintln!("native X11 adapter acceptance: FAIL: {error}");
            std::process::exit(1);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("native X11 adapter acceptance requires Linux and an owned X server");
        std::process::exit(2);
    }
}
