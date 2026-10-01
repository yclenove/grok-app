fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // pipewire-sys only checks >= 0.3, but our SPA bindings enable APIs
        // introduced by 0.3.65. Diagnose the real SDK boundary before bindgen.
        pkg_config::Config::new()
            .atleast_version("0.3.65")
            .cargo_metadata(false)
            .probe("libpipewire-0.3")
            .expect("native Wayland requires PipeWire/SPA >= 0.3.65; see docs/BUILD.md");
        // Older libei can dereference a null connection when retiring a peer
        // that never completed its handshake. Bind the verified static library,
        // not an older runtime libei.so with the same SONAME.
        let mut config = pkg_config::Config::new();
        config
            .atleast_version("1.5")
            .statik(true)
            .cargo_metadata(false);
        let library = config.probe("libei-1.0").expect(
            "native Wayland requires static libei >= 1.5; run scripts/build-computer-use-libei.sh",
        );
        assert!(
            library
                .link_paths
                .iter()
                .any(|p| p.join("libei.a").is_file()),
            "static libei.a is required; dynamic system fallback is not safe"
        );
        config.cargo_metadata(true).probe("libei-1.0").unwrap();
    }
}
