//! Prepare-time materializer for the pinned playwright-core tarball.
fn main() {
    let mut seed = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => seed = args.next().map(std::path::PathBuf::from),
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    let seed = seed.unwrap_or_else(|| {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("resources")
            .join("computer-use")
            .join("seed")
    });
    match grok_computer_use_core::playwright_materialize::prepare_seed(&seed) {
        Ok(tree) => {
            println!(
                "playwright-runtime files={} bytes={} digest={} version={}",
                tree.file_count, tree.total_bytes, tree.listing_sha256, tree.version
            );
        }
        Err(error) => {
            eprintln!("materialize failed {} {}", error.code, error.message);
            std::process::exit(1);
        }
    }
}
