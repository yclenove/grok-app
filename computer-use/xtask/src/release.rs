//! Offline release staging. Network publication belongs exclusively to the tag workflow.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

mod manifests;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

pub(super) struct Target {
    triple: &'static str,
    npm: &'static str,
    os: &'static str,
    cpu: &'static str,
}
const TARGETS: &[Target] = &[
    Target {
        triple: "aarch64-apple-darwin",
        npm: "darwin-arm64",
        os: "darwin",
        cpu: "arm64",
    },
    Target {
        triple: "x86_64-apple-darwin",
        npm: "darwin-x64",
        os: "darwin",
        cpu: "x64",
    },
    Target {
        triple: "x86_64-pc-windows-msvc",
        npm: "win32-x64",
        os: "win32",
        cpu: "x64",
    },
    Target {
        triple: "x86_64-unknown-linux-gnu",
        npm: "linux-x64",
        os: "linux",
        cpu: "x64",
    },
    Target {
        triple: "aarch64-unknown-linux-gnu",
        npm: "linux-arm64",
        os: "linux",
        cpu: "arm64",
    },
];

fn version(tag: &str) -> Result<&str> {
    let value = tag.strip_prefix("cu-v").context("expected cu-v<semver>")?;
    let (core_pre, build) = value
        .split_once('+')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let (core, pre) = core_pre
        .split_once('-')
        .map_or((core_pre, None), |(a, b)| (a, Some(b)));
    let numeric = |s: &str| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    if core.split('.').count() != 3 || !core.split('.').all(numeric) {
        bail!("invalid semver core");
    }
    for (ids, prerelease) in [(pre, true), (build, false)] {
        if let Some(ids) = ids {
            if !ids.split('.').all(|id| {
                !id.is_empty()
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && (!prerelease || !id.bytes().all(|b| b.is_ascii_digit()) || numeric(id))
            }) {
                bail!("invalid semver identifiers");
            }
        }
    }
    Ok(value)
}

fn signing(os: &str, present: impl Fn(&str) -> bool) -> Result<&'static str> {
    let keys: &[&str] = match os {
        "macos" => &[
            "APPLE_CERTIFICATE",
            "APPLE_CERTIFICATE_PASSWORD",
            "APPLE_SIGNING_IDENTITY",
            "APPLE_TEAM_ID",
            "APPLE_API_ISSUER",
            "APPLE_API_KEY",
            "APPLE_API_KEY_P8",
        ],
        "windows" => &["WINDOWS_CERTIFICATE", "WINDOWS_CERTIFICATE_PASSWORD"],
        _ => return Ok("unsigned"),
    };
    if !present(keys[0]) {
        return Ok(if os == "macos" { "ad-hoc" } else { "unsigned" });
    }
    if !keys.iter().all(|k| present(k)) {
        bail!("partial signing configuration for {os}; configure all required secrets or omit the certificate");
    }
    Ok(if os == "macos" {
        "developer-id+notary"
    } else {
        "authenticode"
    })
}

fn archive_name(v: &str, target: &Target) -> String {
    format!(
        "gcu-{v}-{}.{}",
        target.triple,
        if target.os == "win32" {
            "zip"
        } else {
            "tar.gz"
        }
    )
}

fn launcher_package(v: &str, repository: &str) -> Value {
    let optional: BTreeMap<_, _> = TARGETS
        .iter()
        .map(|t| (format!("@grok-computer-use/{}", t.npm), v))
        .collect();
    json!({"name":"gcu", "version":v, "description":"Grok Computer Use native CLI launcher", "license":"MIT", "repository":{"type":"git", "url":format!("git+https://github.com/{repository}.git"), "directory":"computer-use"}, "bin":{"gcu":"bin/gcu.cjs"}, "engines":{"node":">=18"}, "files":["bin", "README.md", "LICENSE"], "optionalDependencies": optional, "publishConfig":{"access":"public"}})
}

fn platform_package(v: &str, target: &Target, repository: &str) -> Value {
    let mut value = json!({"name":format!("@grok-computer-use/{}", target.npm), "version":v, "description":"Grok Computer Use native platform binary", "license":"MIT", "repository":{"type":"git", "url":format!("git+https://github.com/{repository}.git"), "directory":"computer-use"}, "os":[target.os], "cpu":[target.cpu], "files":["bin", "LICENSE"], "publishConfig":{"access":"public"}});
    if target.os == "linux" {
        value["libc"] = json!(["glibc"]);
    }
    value
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn checksums(root: &Path) -> Result<String> {
    let mut hashes = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF8 asset name"))?;
        if name == "SHA256SUMS" || name.starts_with("SHA256SUMS.") {
            continue;
        }
        if !kind.is_file()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        {
            bail!("unsafe release asset name: {name:?}");
        }
        hashes.insert(name, hash(&entry.path())?);
    }
    if hashes.is_empty() {
        bail!("no release assets");
    }
    Ok(hashes
        .iter()
        .map(|(name, sha)| format!("{sha}  {name}\n"))
        .collect())
}

fn stage(
    workspace: &Path,
    v: &str,
    target: &Target,
    repository: &str,
    output: &Path,
) -> Result<()> {
    if output.exists() {
        bail!("stage output already exists; use a fresh directory");
    }
    if v != env!("CARGO_PKG_VERSION") {
        bail!("staging version must match the compiled workspace version");
    }
    let binary = workspace
        .join("target")
        .join(target.triple)
        .join("release")
        .join(if target.os == "win32" {
            "gcu.exe"
        } else {
            "gcu"
        });
    if !binary.is_file() {
        bail!("release binary missing: {}", binary.display());
    }
    let payload = output.join("payload");
    fs::create_dir_all(&payload)?;
    fs::copy(
        &binary,
        payload.join(binary.file_name().context("binary name")?),
    )?;
    if target.os == "darwin" {
        crate::bundle::run(
            [
                "--release".into(),
                "--binary".into(),
                binary.display().to_string(),
                "--output".into(),
                payload.display().to_string(),
            ]
            .into_iter(),
        )?;
        let app = payload.join("Grok Computer Use.app");
        // Apple bundle version fields accept only the numeric SemVer core.
        let bundle_version = v.split(['-', '+']).next().context("missing version core")?;
        fs::write(
            app.join("Contents/Info.plist"),
            crate::bundle::info_plist(true).replace(
                "<string>0.0.0</string>",
                &format!("<string>{bundle_version}</string>"),
            ),
        )?;
        // Updating the plist invalidates the first seal; restore identifier-only ad-hoc signing.
        let argv = crate::bundle::codesign_argv(crate::bundle::RELEASE_BUNDLE_ID, &app)?;
        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .status()?;
        if !status.success() {
            bail!("ad-hoc signing failed");
        }
        fs::copy(app.join("Contents/MacOS/gcu"), payload.join("gcu"))?;
    }
    fs::copy(workspace.join("../LICENSE"), payload.join("LICENSE"))?;
    fs::copy(
        workspace.join("packages/README.md"),
        payload.join("README.md"),
    )?;
    write_json(
        &payload.join("release.json"),
        &json!({"version":v,"target":target.triple}),
    )?;
    write_json(
        &output.join("sbom.cdx.json"),
        &crate::sbom::generate(&fs::read_to_string(workspace.join("Cargo.lock"))?, v)?,
    )?;
    let npm = output.join("npm").join(target.npm);
    fs::create_dir_all(&npm)?;
    write_json(
        &npm.join("package.json"),
        &platform_package(v, target, repository),
    )?;
    fs::copy(workspace.join("../LICENSE"), npm.join("LICENSE"))?;
    Ok(())
}

fn collect(workspace: &Path, v: &str, repository: &str, root: &Path) -> Result<()> {
    let mut hashes = BTreeMap::new();
    for target in TARGETS {
        let name = archive_name(v, target);
        hashes.insert(
            name.clone(),
            hash(&root.join(name)).context("missing platform archive")?,
        );
        for suffix in ["cdx.json", "lock.cdx.json"] {
            let sbom = root.join(format!("gcu-{v}-{}.{suffix}", target.triple));
            let value: Value = serde_json::from_slice(&fs::read(sbom)?)?;
            if value["bomFormat"] != "CycloneDX"
                || value["metadata"]["component"]["version"] != v
                || value["components"].as_array().is_none_or(Vec::is_empty)
            {
                bail!("invalid platform SBOM or version");
            }
        }
        if !root
            .join(format!("grok-computer-use-{}-{v}.tgz", target.npm))
            .is_file()
        {
            bail!("missing packed platform npm package");
        }
        if target.os == "linux" {
            for extension in ["deb", "rpm"] {
                if !root
                    .join(format!("gcu-{v}-{}.{extension}", target.triple))
                    .is_file()
                {
                    bail!("missing Linux package");
                }
            }
        }
        let npm = root.join("npm").join(target.npm);
        let manifest: Value = serde_json::from_slice(&fs::read(npm.join("package.json"))?)?;
        if manifest != platform_package(v, target, repository) {
            bail!("platform npm manifest mismatch");
        }
    }
    let npm = root.join("npm/launcher");
    fs::create_dir_all(npm.join("bin"))?;
    fs::copy(
        workspace.join("packages/npm/gcu.cjs"),
        npm.join("bin/gcu.cjs"),
    )?;
    fs::copy(workspace.join("../LICENSE"), npm.join("LICENSE"))?;
    fs::copy(workspace.join("packages/README.md"), npm.join("README.md"))?;
    write_json(&npm.join("package.json"), &launcher_package(v, repository))?;
    fs::write(
        root.join("gcu.rb"),
        manifests::homebrew(v, repository, &hashes)?,
    )?;
    // winget supports stable numeric versions only; prereleases remain GitHub/npm artifacts.
    if !v.contains(['-', '+']) {
        fs::write(
            root.join("RongleCat.GrokComputerUse.installer.yaml"),
            manifests::winget(v, repository, &hashes)?,
        )?;
        fs::write(
            root.join("RongleCat.GrokComputerUse.locale.en-US.yaml"),
            manifests::winget_locale(v, repository),
        )?;
        fs::write(root.join("RongleCat.GrokComputerUse.yaml"), format!("PackageIdentifier: RongleCat.GrokComputerUse\nPackageVersion: {v}\nDefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.9.0\n"))?;
    }
    fs::write(root.join("SHA256SUMS"), checksums(root)?)?;
    Ok(())
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut args = args;
    let mut mode = String::new();
    let mut tag = format!("cu-v{}", env!("CARGO_PKG_VERSION"));
    let mut target = None;
    let mut output = PathBuf::from("target/cu-release");
    let mut repository = "RongleCat/grok-app".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" | "--stage" | "--collect" | "--signing-status" => {
                if !mode.is_empty() {
                    bail!("choose one release mode");
                }
                mode = arg;
            }
            "--tag" => tag = args.next().context("missing tag")?,
            "--target" => target = Some(args.next().context("missing target")?),
            "--output" => output = PathBuf::from(args.next().context("missing output")?),
            "--repository" => repository = args.next().context("missing repository")?,
            _ => bail!("unknown release argument: {arg}"),
        }
    }
    let v = version(&tag)?;
    if repository.split('/').count() != 2
        || repository.split('/').any(|s| {
            s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        bail!("invalid repository");
    }
    let target = target
        .map(|t| {
            TARGETS
                .iter()
                .find(|x| x.triple == t)
                .context("unsupported target")
        })
        .transpose()?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    match mode.as_str() {
        "--dry-run" => {
            // No filesystem writes, processes, credentials, or publication clients on this branch.
            println!("{}", serde_json::to_string_pretty(&json!({"dry_run":true, "version":v, "publish":false, "targets":TARGETS.iter().map(|t|t.triple).collect::<Vec<_>>(), "macos":signing("macos", |_| false)?, "windows":signing("windows", |_| false)?, "cosign":"keyless on tag publication only; skipped offline"}))?);
            Ok(())
        }
        "--signing-status" => {
            let target = target.context("--signing-status requires --target")?;
            let os = match target.os { "darwin" => "macos", "win32" => "windows", _ => "linux" };
            println!("{}", signing(os, |key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))?);
            Ok(())
        }
        "--stage" => stage(&workspace, v, target.context("--stage requires --target")?, &repository, &output),
        "--collect" => collect(&workspace, v, &repository, &output),
        _ => bail!("usage: cargo xtask release <--dry-run|--stage|--collect> [--tag cu-v<semver>] [--target TRIPLE] [--output DIR] [--repository OWNER/REPO]"),
    }
}
