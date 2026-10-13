use super::*;
use std::collections::BTreeMap;
use std::fs;

fn tiny() -> TreeLimits {
    TreeLimits {
        max_entries: 4,
        max_files: 2,
        max_file_bytes: 4,
        max_total_bytes: 8,
        max_path_chars: 8,
        max_archive_bytes: 4096,
        max_compression_ratio: 20.0,
    }
}

#[test]
fn compiled_archive_and_tree_limits_use_the_same_contract() {
    for (zip, tree) in [
        (crate::archive_zip::ZipPolicy::chromium(), chromium()),
        (crate::archive_zip::ZipPolicy::node_win_x64(), node()),
    ] {
        assert_eq!(zip.max_entries, tree.max_entries);
        assert_eq!(zip.max_files, tree.max_files);
        assert_eq!(zip.max_file_bytes, tree.max_file_bytes);
        assert_eq!(zip.max_total_bytes, tree.max_total_bytes);
        assert_eq!(zip.max_archive_bytes, tree.max_archive_bytes);
        assert_eq!(zip.max_path_chars, tree.max_path_chars);
        assert_eq!(zip.max_compression_ratio, tree.max_compression_ratio);
    }
}

#[test]
fn exactly_at_each_resource_boundary_is_accepted() {
    let mut budget = TreeBudget::default();
    budget.add(tiny(), None).unwrap();
    budget.add(tiny(), None).unwrap();
    budget.add(tiny(), Some(4)).unwrap();
    budget.add(tiny(), Some(4)).unwrap();
    assert_eq!((budget.entries, budget.files, budget.bytes), (4, 2, 8));
    validate_path("12345678", 8).unwrap();
}

#[test]
fn exceeding_any_independent_ceiling_is_rejected() {
    for (limits, entries) in [
        (tiny(), vec![Some(5)]),
        (tiny(), vec![None; 5]),
        (tiny(), vec![Some(0); 3]),
        (
            TreeLimits {
                max_total_bytes: 7,
                ..tiny()
            },
            vec![Some(4), Some(4)],
        ),
    ] {
        let mut budget = TreeBudget::default();
        let result = entries
            .into_iter()
            .try_for_each(|size| budget.add(limits, size));
        assert_eq!(result.unwrap_err().code, "oversize");
    }
}

#[test]
fn counters_fail_closed_instead_of_saturating_or_wrapping() {
    let limits = TreeLimits {
        max_entries: u32::MAX,
        max_files: u32::MAX,
        max_file_bytes: u64::MAX,
        max_total_bytes: u64::MAX,
        ..tiny()
    };
    for mut budget in [
        TreeBudget {
            entries: u32::MAX,
            ..Default::default()
        },
        TreeBudget {
            files: u32::MAX,
            ..Default::default()
        },
        TreeBudget {
            bytes: u64::MAX,
            ..Default::default()
        },
    ] {
        assert_eq!(budget.add(limits, Some(1)).unwrap_err().code, "oversize");
    }
}

#[test]
fn ambiguous_listing_and_overlong_paths_are_rejected() {
    for name in [
        "123456789",
        "../file",
        "a//b",
        "a\tb",
        "a\nb",
        "a\rb",
        "C:/file",
        "/file",
    ] {
        assert_eq!(validate_path(name, 8).unwrap_err().code, "path", "{name:?}");
    }
}

#[test]
fn disk_inventory_counts_empty_directories_and_hashes_unchanged_format() {
    let root = std::env::temp_dir().join(format!("grok-cu-tree-limits-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("dir")).unwrap();
    fs::write(root.join("a"), b"aaaa").unwrap();
    fs::write(root.join("b"), b"bbbb").unwrap();
    let result = crate::runtime_tree::listing(&root, tiny(), &BTreeMap::new()).unwrap();
    assert_eq!(
        result,
        format!(
            "a\t4\t{}\nb\t4\t{}\n",
            crate::playwright_materialize::sha256_hex(b"aaaa"),
            crate::playwright_materialize::sha256_hex(b"bbbb")
        )
    );
    assert_eq!(crate::runtime_tree::manifest(&result).1, 8);
    fs::create_dir(root.join("empty")).unwrap();
    assert_eq!(
        crate::runtime_tree::listing(&root, tiny(), &BTreeMap::new())
            .unwrap_err()
            .code,
        "oversize"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn disk_inventory_rejects_single_large_file_before_content_read() {
    let root = std::env::temp_dir().join(format!("grok-cu-tree-limits-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let file = fs::File::create(root.join("big")).unwrap();
    file.set_len(chromium().max_file_bytes + 1).unwrap();
    drop(file);
    assert_eq!(
        crate::runtime_chromium::tree_manifest(&root, "x86_64-windows")
            .unwrap_err()
            .code,
        "oversize"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn chromium_copy_refuses_oversize_source_without_creating_destination() {
    let root = std::env::temp_dir().join(format!("grok-cu-copy-limits-{}", uuid::Uuid::new_v4()));
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    let file = fs::File::create(src.join("large")).unwrap();
    file.set_len(chromium().max_file_bytes + 1).unwrap();
    drop(file);
    let dest = root.join("dest");
    assert_eq!(
        crate::runtime_chromium::copy_tree(&src, &dest, "x86_64-windows")
            .unwrap_err()
            .code,
        "oversize"
    );
    assert!(!dest.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounded_copy_preserves_content_and_refuses_one_over_before_file_write() {
    let root = std::env::temp_dir().join(format!("grok-cu-copy-limits-{}", uuid::Uuid::new_v4()));
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a"), b"aaaa").unwrap();
    let dest = root.join("dest");
    crate::playwright_materialize::copy_tree_with_limits(&src, &dest, tiny()).unwrap();
    assert_eq!(fs::read(dest.join("a")).unwrap(), b"aaaa");
    fs::write(src.join("a"), b"aaaaa").unwrap();
    let invalid = root.join("invalid");
    assert_eq!(
        crate::playwright_materialize::copy_tree_with_limits(&src, &invalid, tiny())
            .unwrap_err()
            .code,
        "oversize"
    );
    assert!(!invalid.join("a").exists());
    assert_eq!(fs::read(dest.join("a")).unwrap(), b"aaaa");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn streaming_transfer_checks_exact_length_including_zero_length_entries() {
    for (data, size) in [(&b"abc"[..], 3), (&b""[..], 0)] {
        crate::runtime_tree::copy_exact(&mut &data[..], &mut std::io::sink(), size).unwrap();
    }
    for (data, size) in [(&b"abc"[..], 2), (&b"abc"[..], 4), (&b"a"[..], 0)] {
        assert_eq!(
            crate::runtime_tree::copy_exact(&mut &data[..], &mut std::io::sink(), size)
                .unwrap_err()
                .code,
            "truncated"
        );
    }
    assert_eq!(
        crate::runtime_tree::copy_exact(&mut &b""[..], &mut std::io::sink(), u64::MAX)
            .unwrap_err()
            .code,
        "oversize"
    );
}

#[test]
fn locks_cannot_enlarge_compiled_budgets() {
    let original =
        crate::runtime_lock::load_lock(&crate::runtime_lock::shipped_lock_path()).unwrap();
    for field in 0..5 {
        let mut lock = original.clone();
        match field {
            0 => lock.chromium.tree_files = chromium().max_files + 1,
            1 => lock.chromium.tree_bytes = chromium().max_total_bytes + 1,
            2 => lock.chromium.archive_bytes = chromium().max_archive_bytes + 1,
            3 => lock.playwright.tree_bytes = playwright().max_total_bytes + 1,
            _ => lock.js_runtime.archive_bytes = node().max_archive_bytes + 1,
        }
        assert_eq!(
            crate::runtime_lock::validate_lock(&lock).unwrap_err().code,
            "oversize"
        );
    }
}
