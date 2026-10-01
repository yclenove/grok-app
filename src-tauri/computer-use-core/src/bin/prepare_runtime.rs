//! CLI: prepare or check an explicitly selected Computer Use runtime target.
fn main() {
    let mut mode = None;
    let mut target = None;
    let mut repo = None;
    let mut seed = None;
    let mut cache = None;
    let mut lock = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--prepare" => mode = Some("prepare"),
            "--check" => mode = Some("check"),
            "--target" => target = args.next(),
            "--repo" => repo = args.next().map(std::path::PathBuf::from),
            "--seed" => seed = args.next().map(std::path::PathBuf::from),
            "--cache" => cache = args.next().map(std::path::PathBuf::from),
            "--lock" => lock = args.next().map(std::path::PathBuf::from),
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    let mode = match mode {
        Some(m) => m,
        None => {
            eprintln!(
                "usage: cu-prepare-runtime --prepare|--check --target <target> [--repo <path>]"
            );
            std::process::exit(2);
        }
    };
    let target = match target.or_else(|| std::env::var("GROK_CU_TARGET").ok()) {
        Some(t) => t,
        None => {
            eprintln!("--target is required (do not guess host equals target)");
            std::process::exit(2);
        }
    };
    let repo_root = repo.unwrap_or_else(|| {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    });
    let mut ctx =
        grok_computer_use_core::runtime_prepare::PrepareContext::shipped(repo_root, target);
    if let Some(seed) = seed.or_else(|| {
        std::env::var("GROK_CU_SEED")
            .ok()
            .map(std::path::PathBuf::from)
    }) {
        ctx.seed_dir = seed;
    }
    if let Some(cache) = cache.or_else(|| {
        std::env::var("GROK_CU_CACHE")
            .ok()
            .map(std::path::PathBuf::from)
    }) {
        ctx.cache_dir = cache;
    }
    if let Some(lock) = lock {
        ctx.lock_path = lock;
    }
    let result = match mode {
        "prepare" => grok_computer_use_core::runtime_prepare::prepare(&ctx),
        _ => grok_computer_use_core::runtime_prepare::check(&ctx),
    };
    match result {
        Ok(report) => {
            println!(
                "ok computer-use runtime seed target={} manifest={} tree={} chromium={} importProbe={}",
                report.target, report.manifest_sha256, report.tree_sha256, report.chromium_tree_sha256, report.import_probe
            );
        }
        Err(error) => {
            eprintln!("{}: {}", error.code, error.message);
            std::process::exit(1);
        }
    }
}
