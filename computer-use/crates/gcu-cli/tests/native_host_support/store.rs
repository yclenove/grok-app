//! Offline store-copy checks, included in the native_host acceptance target.
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};

const EXTENSION: &str = "../../tools/browser-extension";
const MATERIALS: &[&str] = &[
    "README.md",
    "store/README.md",
    "store/chrome-web-store.md",
    "store/edge-add-ons.md",
    "store/permissions.md",
    "store/data-use.md",
    "store/privacy-policy.md",
    "../../docs/browser-extension.md",
];

fn permission_rows(copy: &str) -> Vec<String> {
    copy.lines()
        .filter_map(|line| {
            let row = line.strip_prefix("| `")?;
            let (permission, reason) = row.split_once("` | ")?;
            assert!(!reason.trim_matches([' ', '|']).is_empty());
            Some(permission.to_owned())
        })
        .collect()
}

fn matches_permissions(manifest: &Value, copy: &str) -> bool {
    let declared: Vec<_> = manifest["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap().to_owned())
        .collect();
    let documented = permission_rows(copy);
    let declared_set: BTreeSet<_> = declared.iter().collect();
    let documented_set: BTreeSet<_> = documented.iter().collect();
    declared.len() == declared_set.len()
        && documented.len() == documented_set.len()
        && declared_set == documented_set
        && [
            "host_permissions",
            "optional_host_permissions",
            "optional_permissions",
            "content_scripts",
        ]
        .iter()
        .all(|field| {
            manifest
                .get(field)
                .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
        })
}

#[test]
fn store_manifest_permissions_match_reviewed_permission_reasons() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(EXTENSION);
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
    let copy = fs::read_to_string(root.join("store/permissions.md")).unwrap();
    assert!(matches_permissions(&manifest, &copy));
}

#[test]
fn store_permissions_added_removed_or_broadened_fail_copy_check() {
    let manifest = json!({"permissions":["activeTab", "scripting"]});
    let copy = "| `activeTab` | Explicit tab consent. |\n| `scripting` | Bundled injection. |";
    assert!(matches_permissions(&manifest, copy));
    let mut added = manifest.clone();
    added["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("tabs"));
    assert!(!matches_permissions(&added, copy));
    let mut removed = manifest.clone();
    let _ = removed["permissions"].as_array_mut().unwrap().pop();
    assert!(!matches_permissions(&removed, copy));
    assert!(!matches_permissions(&manifest, &format!("{copy}\n{copy}")));
    for field in [
        "host_permissions",
        "optional_host_permissions",
        "optional_permissions",
        "content_scripts",
    ] {
        let mut broadened = manifest.clone();
        broadened[field] = json!(["<all_urls>"]);
        assert!(!matches_permissions(&broadened, copy));
    }
}

fn links(markdown: &str) -> Vec<&str> {
    // Materials use inline Markdown links; code URLs and explicit placeholders
    // are intentionally not links. No network or account access is needed.
    markdown
        .split("](")
        .skip(1)
        .map(|part| part.split_once(')').unwrap().0)
        .collect()
}

fn has_anchor(markdown: &str, fragment: &str) -> bool {
    markdown.contains(&format!("<a id=\"{fragment}\"></a>"))
        || markdown.lines().any(|line| {
            let Some(heading) = line.strip_prefix('#') else {
                return false;
            };
            let heading = heading.trim_start_matches('#').trim().to_lowercase();
            let anchor: String = heading
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(*c, ' ' | '-' | '_'))
                .map(|c| if c == ' ' { '-' } else { c })
                .collect();
            anchor == fragment
        })
}

fn local_link_resolves(source: &Path, href: &str) -> bool {
    let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
    let target = if path.is_empty() {
        source.to_path_buf()
    } else {
        source.parent().unwrap().join(path)
    };
    target.is_file()
        && (fragment.is_empty() || has_anchor(&fs::read_to_string(target).unwrap(), fragment))
}

#[test]
fn store_install_share_revoke_and_uninstall_links_resolve_offline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(EXTENSION);
    for material in MATERIALS {
        let source = root.join(material);
        let markdown = fs::read_to_string(&source).unwrap();
        for href in links(&markdown) {
            assert!(
                local_link_resolves(&source, href),
                "broken local link in {material}: {href}"
            );
        }
    }
    let guide = root.join("../../docs/browser-extension.md");
    for anchor in ["install", "share", "revoke", "uninstall"] {
        assert!(local_link_resolves(&guide, &format!("#{anchor}")));
    }
    assert!(!local_link_resolves(&guide, "missing-guide.md"));
    assert!(!local_link_resolves(&guide, "#missing-section"));
    for listing in ["store/chrome-web-store.md", "store/edge-add-ons.md"] {
        let markdown = fs::read_to_string(root.join(listing)).unwrap();
        for anchor in ["install", "share", "revoke", "uninstall"] {
            let href = format!("../../../docs/browser-extension.md#{anchor}");
            assert!(links(&markdown).contains(&href.as_str()));
        }
    }
}
