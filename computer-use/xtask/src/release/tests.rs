use super::*;
use serde_json::json;

#[test]
fn version_tag_invalid_or_unsafe_rejected() {
    for tag in [
        "v1.2.3",
        "cu-v01.2.3",
        "cu-v1.2",
        "cu-v1.2.3\n",
        "cu-v1.2.3-01",
        "cu-v1.2.3/evil",
    ] {
        assert!(version(tag).is_err(), "{tag:?}");
    }
    for tag in ["cu-v1.2.3", "cu-v0.1.0-rc.1", "cu-v1.2.3+build.7"] {
        assert_eq!(version(tag).unwrap(), &tag[4..]);
    }
}

#[test]
fn checksum_unordered_files_sorted_and_standard_format() {
    let dir = temp_dir("sums");
    fs::write(dir.join("z.zip"), b"abc").unwrap();
    fs::write(dir.join("a.tgz"), b"").unwrap();
    let sums = checksums(&dir).unwrap();
    assert_eq!(sums, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a.tgz\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  z.zip\n");
    fs::write(dir.join("SHA256SUMS"), "old").unwrap();
    assert_eq!(checksums(&dir).unwrap(), sums);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn signing_missing_secrets_skips_partial_configuration_fails_closed() {
    assert_eq!(signing("macos", |_| false).unwrap(), "ad-hoc");
    assert_eq!(signing("windows", |_| false).unwrap(), "unsigned");
    assert!(signing("macos", |key| key == "APPLE_CERTIFICATE").is_err());
    assert!(signing("windows", |key| key == "WINDOWS_CERTIFICATE").is_err());
    assert_eq!(signing("macos", |_| true).unwrap(), "developer-id+notary");
    assert_eq!(signing("windows", |_| true).unwrap(), "authenticode");
}

#[test]
fn manifest_generation_five_targets_pin_version_hash_and_helper() {
    for target in TARGETS {
        let platform = platform_package("1.2.3", target, "RongleCat/grok-app");
        assert_eq!(
            platform["name"],
            format!("@grok-computer-use/{}", target.npm)
        );
        assert_eq!(platform["cpu"], json!([target.cpu]));
        assert_eq!(platform["version"], "1.2.3");
    }
    let launcher = launcher_package("1.2.3", "RongleCat/grok-app");
    assert_eq!(
        launcher["repository"]["url"],
        "git+https://github.com/RongleCat/grok-app.git"
    );
    assert_eq!(
        launcher["optionalDependencies"].as_object().unwrap().len(),
        5
    );
    let sums: BTreeMap<_, _> = TARGETS
        .iter()
        .map(|t| (archive_name("1.2.3", t), "a".repeat(64)))
        .collect();
    let brew = manifests::homebrew("1.2.3", "RongleCat/grok-app", &sums).unwrap();
    assert!(brew.contains("Grok Computer Use.app"));
    assert!(brew.contains(&"a".repeat(64)));
    let winget = manifests::winget("1.2.3", "RongleCat/grok-app", &sums).unwrap();
    assert!(winget.contains("NestedInstallerType: portable"));
    assert!(winget.contains("NestedInstallerFiles:"));
    assert!(manifests::homebrew("1.2.3", "RongleCat/grok-app", &BTreeMap::new()).is_err());
}

#[test]
fn dry_run_invalid_target_or_missing_explicit_mode_fails() {
    assert!(run(["--target", "evil"].map(String::from).into_iter()).is_err());
    assert!(run(std::iter::empty()).is_err());
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gcu-release-{label}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn collect_complete_artifacts_emits_manifests_and_sums_missing_target_fails() {
    let root = temp_dir("collect");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for target in TARGETS {
        fs::write(root.join(archive_name("1.2.3", target)), b"archive fixture").unwrap();
        for suffix in ["cdx.json", "lock.cdx.json"] {
            write_json(&root.join(format!("gcu-1.2.3-{}.{suffix}", target.triple)), &json!({"bomFormat":"CycloneDX", "metadata":{"component":{"version":"1.2.3"}}, "components":[{"name":"fixture"}]})).unwrap();
        }
        if target.os == "linux" {
            for suffix in ["deb", "rpm"] {
                fs::write(
                    root.join(format!("gcu-1.2.3-{}.{suffix}", target.triple)),
                    b"package fixture",
                )
                .unwrap();
            }
        }
        fs::write(
            root.join(format!("grok-computer-use-{}-1.2.3.tgz", target.npm)),
            b"npm fixture",
        )
        .unwrap();
        let npm = root.join("npm").join(target.npm);
        fs::create_dir_all(&npm).unwrap();
        write_json(
            &npm.join("package.json"),
            &platform_package("1.2.3", target, "RongleCat/grok-app"),
        )
        .unwrap();
    }
    collect(&workspace, "1.2.3", "RongleCat/grok-app", &root).unwrap();
    let sums = fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    assert!(sums.contains("  gcu.rb\n"));
    assert!(sums.contains("  RongleCat.GrokComputerUse.installer.yaml\n"));
    let launcher: Value =
        serde_json::from_slice(&fs::read(root.join("npm/launcher/package.json")).unwrap()).unwrap();
    assert_eq!(launcher["version"], "1.2.3");
    fs::remove_file(root.join(archive_name("1.2.3", &TARGETS[0]))).unwrap();
    assert!(collect(&workspace, "1.2.3", "RongleCat/grok-app", &root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn checksum_unsafe_asset_names_are_rejected() {
    let root = temp_dir("bad-name");
    fs::write(root.join("name with spaces.zip"), b"fixture").unwrap();
    assert!(checksums(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}
