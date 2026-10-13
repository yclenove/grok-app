//! CLI: prepare, check, or audit sources for an explicit runtime target.
const USAGE: &str = "usage: cu-prepare-runtime --prepare|--check|--verify-sources --target <target> [--repo <path>] [--seed <path>] [--cache <path>] [--lock <path>]";

fn argument_value(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    match args
        .next()
        .filter(|value| !value.starts_with("--") && !value.is_empty())
    {
        Some(value) => value,
        None => {
            eprintln!("{flag} requires a value\n{USAGE}");
            std::process::exit(2);
        }
    }
}

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
            "--prepare" | "--check" | "--verify-sources" => {
                if mode.is_some() {
                    eprintln!("exactly one mode is required\n{USAGE}");
                    std::process::exit(2);
                }
                mode = Some(match arg.as_str() {
                    "--prepare" => "prepare",
                    "--check" => "check",
                    _ => "sources",
                });
            }
            "--target" => target = Some(argument_value(&mut args, &arg)),
            "--repo" => repo = Some(std::path::PathBuf::from(argument_value(&mut args, &arg))),
            "--seed" => seed = Some(std::path::PathBuf::from(argument_value(&mut args, &arg))),
            "--cache" => cache = Some(std::path::PathBuf::from(argument_value(&mut args, &arg))),
            "--lock" => lock = Some(std::path::PathBuf::from(argument_value(&mut args, &arg))),
            "--help" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    let mode = match mode {
        Some(m) => m,
        None => {
            eprintln!("{USAGE}");
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
    if mode == "sources" {
        match grok_computer_use_core::runtime_prepare::verify_sources(&ctx) {
            Ok(report) => println!(
                "{}",
                serde_json::to_string(&report).expect("source report serialization")
            ),
            Err(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(1);
            }
        }
        return;
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
