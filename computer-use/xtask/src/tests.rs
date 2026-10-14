use super::deps::{check_metadata, check_tauri_manifest};
use serde_json::{json, Value};

fn metadata(edges: &[(&str, &[&str])]) -> String {
    let packages: Vec<Value> = edges
        .iter()
        .map(|(name, dependencies)| {
            json!({
                "id": format!("path+file:///fixture/{name}#0.0.0"),
                "name": name,
                "dependencies": dependencies.iter().map(|dependency| json!({
                    "name": dependency,
                    "kind": null,
                    "optional": false,
                    "target": null,
                    "rename": null
                })).collect::<Vec<_>>()
            })
        })
        .collect();
    serde_json::to_string(&json!({
        "workspace_members": packages.iter().map(|package| &package["id"]).collect::<Vec<_>>(),
        "packages": packages
    }))
    .unwrap()
}

#[test]
fn check_deps_compliant_metadata_passes() {
    let input = metadata(&[
        (
            "gcu-protocol",
            &["serde", "serde_json", "schemars", "thiserror"],
        ),
        ("gcu-host-api", &["gcu-protocol", "async-trait"]),
        (
            "gcu-core",
            &[
                "gcu-protocol",
                "gcu-host-api",
                "tokio",
                "parking_lot",
                "similar",
                "unicode-segmentation",
                "image",
                "tracing",
            ],
        ),
        (
            "gcu-host-protocol",
            &[
                "gcu-protocol",
                "serde",
                "serde_json",
                "schemars",
                "thiserror",
                "tokio",
                "interprocess",
                "hmac",
                "sha2",
                "rand",
                "libc",
                "windows",
            ],
        ),
        ("gcu-driver-macos", &["gcu-protocol", "gcu-core", "objc2"]),
        (
            "gcu-driver-windows",
            &["gcu-protocol", "gcu-core", "windows"],
        ),
        (
            "gcu-driver-linux",
            &["gcu-protocol", "gcu-core", "atspi", "x11rb"],
        ),
        ("gcu-driver-fake", &["gcu-protocol", "gcu-core"]),
        (
            "gcu-mcp",
            &[
                "gcu-protocol",
                "gcu-core",
                "gcu-host-api",
                "rmcp",
                "tokio",
                "axum",
            ],
        ),
        ("gcu-cli", &["gcu-mcp", "gcu-driver-fake"]),
        ("gcu-script", &["gcu-core"]),
        ("gcu-overlay", &["gcu-host-api"]),
        ("gcu-fixture", &["eframe", "serde", "serde_json"]),
        ("xtask", &["anyhow", "serde", "serde_json", "toml"]),
    ]);
    assert!(check_metadata(&input).unwrap().is_empty());
}

#[test]
fn check_deps_core_os_dependency_fails() {
    assert_eq!(
        check_metadata(&metadata(&[("gcu-core", &["windows"])]))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn check_deps_driver_driver_dependency_fails() {
    assert_eq!(
        check_metadata(&metadata(&[("gcu-driver-linux", &["gcu-driver-fake"])]))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn check_deps_mcp_platform_dependency_fails() {
    for dependency in ["windows", "objc2", "atspi", "x11rb", "gcu-driver-macos"] {
        assert_eq!(
            check_metadata(&metadata(&[("gcu-mcp", &[dependency])]))
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn check_deps_tauri_core_dependency_fails() {
    assert_eq!(
        check_tauri_manifest(
            "[dependencies]\ngcu-core = { path = '../computer-use/crates/gcu-core' }\n"
        )
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn check_deps_forbidden_edges_each_rule_fails() {
    for (owner, dependency) in [
        ("gcu-protocol", "tokio"),
        ("gcu-protocol", "libc"),
        ("gcu-host-api", "windows"),
        ("gcu-core", "reqwest"),
        ("gcu-core", "fs2"),
        ("gcu-driver-fake", "gcu-driver-linux"),
        ("gcu-driver-macos", "gcu-mcp"),
        ("gcu-host-protocol", "gcu-core"),
        ("gcu-host-protocol", "gcu-driver-windows"),
    ] {
        let violations = check_metadata(&metadata(&[(owner, &[dependency])])).unwrap();
        assert_eq!(violations.len(), 1, "{owner} -> {dependency}");
        assert!(violations[0].contains(&format!("{owner} -> {dependency}")));
    }
}

#[test]
fn check_deps_optional_target_dev_build_edges_fail() {
    for kind in [json!(null), json!("dev"), json!("build")] {
        let mut input: Value =
            serde_json::from_str(&metadata(&[("gcu-core", &["windows"])])).unwrap();
        let dependency = &mut input["packages"][0]["dependencies"][0];
        dependency["kind"] = kind;
        dependency["optional"] = json!(true);
        dependency["target"] = json!("cfg(windows)");
        dependency["rename"] = json!("innocent_alias");
        assert_eq!(check_metadata(&input.to_string()).unwrap().len(), 1);
    }
}

#[test]
fn check_deps_external_metadata_packages_ignored() {
    let mut input: Value = serde_json::from_str(&metadata(&[("gcu-cli", &["windows"])])).unwrap();
    input["packages"].as_array_mut().unwrap().push(json!({
        "id": "registry+https://github.com/rust-lang/crates.io-index#windows@0.61.0",
        "name": "windows",
        "dependencies": [{"name": "windows-core"}]
    }));
    assert!(check_metadata(&input.to_string()).unwrap().is_empty());
}

#[test]
fn check_deps_unknown_workspace_crate_returns_error() {
    assert!(check_metadata(&metadata(&[("gcu-unreviewed", &[])])).is_err());
}

#[test]
fn check_deps_missing_member_returns_error() {
    let input = json!({"workspace_members": ["missing"], "packages": []});
    assert!(check_metadata(&input.to_string()).is_err());
}

#[test]
fn check_deps_invalid_metadata_returns_error() {
    assert!(check_metadata("not JSON").is_err());
    assert!(check_metadata("{}").is_err());
}

#[test]
fn check_deps_tauri_without_computer_use_passes() {
    assert!(
        check_tauri_manifest("[dependencies]\ntauri = '2'\nserde = '1'\n")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn check_deps_tauri_protocols_and_aliases_pass() {
    let input = r#"
[dependencies]
gcu-protocol = { path = "../computer-use/crates/gcu-protocol" }
host = { package = "gcu-host-protocol", path = "../computer-use/crates/gcu-host-protocol" }
[target.'cfg(windows)'.build-dependencies]
gcu-host-protocol = "0.0.0"
"#;
    assert!(check_tauri_manifest(input).unwrap().is_empty());
}

#[test]
fn check_deps_tauri_all_forbidden_computer_use_crates_fail() {
    for dependency in [
        "gcu-core",
        "gcu-host-api",
        "gcu-driver-macos",
        "gcu-driver-windows",
        "gcu-driver-linux",
        "gcu-driver-fake",
        "gcu-mcp",
        "gcu-cli",
        "gcu-overlay",
        "gcu-script",
        "gcu-fixture",
        "gcu-unreviewed",
    ] {
        let input = format!("[dependencies]\n{dependency} = '0.0.0'\n");
        assert_eq!(
            check_tauri_manifest(&input).unwrap().len(),
            1,
            "{dependency}"
        );
    }
}

#[test]
fn check_deps_tauri_renamed_optional_target_edges_fail() {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let input = format!(
            "[target.'cfg(windows)'.{section}]\nhidden = {{ package = 'gcu-core', version = '0.0.0', optional = true }}\n"
        );
        let violations = check_tauri_manifest(&input).unwrap();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("src-tauri -> gcu-core"));
    }
}

#[test]
fn check_deps_tauri_top_level_dev_build_edges_fail() {
    for section in ["dev-dependencies", "build-dependencies"] {
        let input =
            format!("[{section}]\nhidden = {{ package = 'gcu-core', version = '0.0.0' }}\n");
        assert_eq!(check_tauri_manifest(&input).unwrap().len(), 1);
    }
}

#[test]
fn check_deps_tauri_inherited_alias_fails() {
    let input = "[workspace.dependencies]\nhidden = { package = 'gcu-core', version = '0.0.0' }\n[dependencies]\nhidden.workspace = true\n";
    assert_eq!(check_tauri_manifest(input).unwrap().len(), 1);
}

#[test]
fn check_deps_tauri_inherited_protocol_passes() {
    let input = "[workspace.dependencies]\nhost = { package = 'gcu-host-protocol', version = '0.0.0' }\n[dependencies]\nhost.workspace = true\n";
    assert!(check_tauri_manifest(input).unwrap().is_empty());
}

#[test]
fn check_deps_tauri_unresolved_inheritance_returns_error() {
    let input = "[dependencies]\nhidden.workspace = true\n";
    assert!(check_tauri_manifest(input).is_err());
}

#[test]
fn check_deps_invalid_tauri_manifest_returns_error() {
    for input in [
        "[dependencies",
        "dependencies = false",
        "[dependencies]\ngcu-core = 1",
        "[dependencies]\nhidden = { package = false }",
        "target = false",
        "[target]\nwindows = false",
    ] {
        assert!(check_tauri_manifest(input).is_err(), "{input}");
    }
}

#[test]
fn check_deps_model_and_fake_support_libraries_pass() {
    let input = metadata(&[
        (
            "gcu-core",
            &[
                "smol_str",
                "smallvec",
                "bitflags",
                "thiserror",
                "async-trait",
                "serde",
                "serde_json",
                "toml",
                "unicode-normalization",
                "futures-core",
            ],
        ),
        (
            "gcu-driver-fake",
            &[
                "gcu-core",
                "gcu-protocol",
                "async-trait",
                "tokio",
                "futures-core",
                "serde",
                "thiserror",
                "ron",
                "png",
            ],
        ),
        ("gcu-script", &["gcu-core", "gcu-protocol", "rquickjs"]),
    ]);
    assert!(check_metadata(&input).unwrap().is_empty());
}

#[test]
fn check_deps_script_io_and_model_os_edges_fail() {
    for (owner, dependency) in [
        ("gcu-core", "ron"),
        ("gcu-core", "png"),
        ("gcu-script", "tokio"),
        ("gcu-script", "reqwest"),
        ("gcu-script", "windows"),
    ] {
        assert_eq!(
            check_metadata(&metadata(&[(owner, &[dependency])]))
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn check_deps_core_snapshot_and_bench_libraries_dev_only() {
    for name in ["insta", "criterion", "gcu-driver-fake"] {
        for (kind, allowed) in [
            (json!("dev"), true),
            (json!(null), false),
            (json!("build"), false),
        ] {
            let mut input: Value =
                serde_json::from_str(&metadata(&[("gcu-core", &[name])])).unwrap();
            input["packages"][0]["dependencies"][0]["kind"] = kind;
            assert_eq!(
                check_metadata(&input.to_string()).unwrap().is_empty(),
                allowed
            );
        }
    }
}

#[test]
fn check_deps_proptest_dev_only_and_image_runtime_allowed() {
    for kind in ["dev", "build", "normal"] {
        let input = json!({
            "workspace_members": ["core"],
            "packages": [{"id": "core", "name": "gcu-core", "dependencies": [
                {"name": "proptest", "kind": kind}, {"name": "image", "kind": null}
            ]}]
        });
        let violations = check_metadata(&input.to_string()).unwrap();
        assert_eq!(violations.len(), usize::from(kind != "dev"));
    }
}

#[test]
fn host_protocol_minimum_auth_schema_platform_dependencies_only() {
    for name in [
        "gcu-core",
        "gcu-host-api",
        "gcu-driver-fake",
        "reqwest",
        "axum",
        "tempfile",
    ] {
        assert_eq!(
            check_metadata(&metadata(&[("gcu-host-protocol", &[name])]))
                .unwrap()
                .len(),
            1
        );
    }
}
