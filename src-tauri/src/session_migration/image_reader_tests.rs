use super::*;
use crate::session_migration::types::AssetRecord;
use crate::session_migration::{
    engine, job_control, scan, selection::AllowedSelection, test_home::IsolatedAppHome,
    types::Provider,
};
use serde_json::json;

struct Fixture {
    _home: IsolatedAppHome,
    scope: ImageScope,
    preview: String,
    ids: Vec<String>,
    source: PathBuf,
    source_bytes: Vec<u8>,
    png: Vec<u8>,
}

fn fixture(large: bool) -> Fixture {
    let home = IsolatedAppHome::acquire("import-image-reader");
    engine::reset_runtime();
    let root = home.path.join("synthetic Codex 来源");
    fs::create_dir_all(&root).unwrap();
    let edge = if large { 1024 } else { 16 };
    let mut random = 0x67ab321du32;
    let image = image::RgbImage::from_fn(edge, edge, |_, _| {
        let mut channels = [0; 3];
        for c in &mut channels {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            *c = random as u8;
        }
        image::Rgb(channels)
    });
    let mut buffer = Cursor::new(Vec::new());
    image.write_to(&mut buffer, ImageFormat::Png).unwrap();
    let png = buffer.into_inner();
    if large {
        assert!(png.len() > 2 * 1024 * 1024);
    }
    fs::write(root.join("same.png"), &png).unwrap();
    let rows = [
        json!({"type":"session_meta","payload":{"id":"synthetic-images"}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"图片前", "local_images":["same.png","same.png"]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","id":"remote","content":[
            {"type":"input_image","image_url":"https://example.invalid/private-image.png"},
            {"type":"input_image","file_id":"same.png"},
            {"type":"input_image","image_url":"missing.png"}]}}),
    ];
    let source = root.join("source.jsonl");
    let source_bytes = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>()
        .into_bytes();
    fs::write(&source, &source_bytes).unwrap();
    let state = scan::scan_start(vec![(Provider::Codex, root.to_string_lossy().into())]).unwrap();
    assert_eq!(state.candidates.len(), 1);
    let candidate = &state.candidates[0];
    let (preview, details) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let ids = details.assets.ids;
    assert_eq!(ids.len(), 5);
    Fixture {
        _home: home,
        scope: ImageScope::Preview {
            preview_id: preview.clone(),
        },
        preview,
        ids,
        source,
        source_bytes,
        png,
    }
}

impl Fixture {
    fn dir(&self) -> PathBuf {
        catalog::staging_dir().join("previews").join(&self.preview)
    }
    fn unchanged(&self) {
        assert_eq!(fs::read(&self.source).unwrap(), self.source_bytes);
    }
    fn resign_synthetic_preview(&self) {
        let dir = self.dir();
        let mut lease = jobs::read_preview_lease(&dir).unwrap().unwrap();
        lease.file_hashes = jobs::prepared_hashes(&dir).unwrap();
        jobs::write_preview_lease(&dir, &lease).unwrap();
    }
}

#[test]
fn imported_images_resolve_exact_occurrences_and_unresolved_references_without_fetching() {
    let fixture = fixture(false);
    let lease_before =
        serde_json::to_value(jobs::read_preview_lease(&fixture.dir()).unwrap()).unwrap();
    for id in &fixture.ids[..2] {
        let resolved = resolve(&fixture.scope, id);
        assert_eq!(resolved.state, ImageState::Stored);
        assert_eq!(resolved.image.unwrap().mime, "image/png");
        assert_eq!(read(&fixture.scope, id).unwrap().bytes, fixture.png);
    }
    assert_ne!(fixture.ids[0], fixture.ids[1]);
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[2]).state,
        ImageState::Unresolved
    );
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[3]).state,
        ImageState::Unresolved
    );
    assert_ne!(
        resolve(&fixture.scope, &fixture.ids[4]).state,
        ImageState::Stored
    );
    for id in [
        "../same.png",
        "file:///private.png",
        "https://example.invalid/private-image.png",
        "",
    ] {
        assert_eq!(resolve(&fixture.scope, id).state, ImageState::Invalid);
    }
    assert_eq!(
        serde_json::to_value(jobs::read_preview_lease(&fixture.dir()).unwrap()).unwrap(),
        lease_before
    );
    fixture.unchanged();
}

#[test]
fn imported_images_fail_closed_for_expiry_wrong_owner_and_changed_bytes_or_metadata() {
    let fixture = fixture(false);
    let dir = fixture.dir();
    let original = fs::read(dir.join("assets.json")).unwrap();
    let wrong = ImageScope::Preview {
        preview_id: uuid::Uuid::new_v4().to_string(),
    };
    assert_eq!(resolve(&wrong, &fixture.ids[0]).state, ImageState::Invalid);
    let mut assets: Vec<AssetRecord> = serde_json::from_slice(&original).unwrap();
    let path = dir.join("blobs").join(assets[0].digest.as_ref().unwrap());
    let mut damaged = fixture.png.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    fs::write(&path, damaged).unwrap();
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[0]).state,
        ImageState::Invalid
    );
    fs::write(&path, &fixture.png).unwrap();
    for case in [
        "size",
        "digest",
        "mime",
        "duplicate",
        "oversize",
        "excluded",
        "missing",
    ] {
        assets = serde_json::from_slice(&original).unwrap();
        let expected = match case {
            "size" => {
                assets[0].size += 1;
                ImageState::Invalid
            }
            "digest" => {
                assets[0].digest = Some("../escape.png".into());
                ImageState::Invalid
            }
            "mime" => {
                assets[0].mime = Some("image/jpeg".into());
                ImageState::Invalid
            }
            "duplicate" => {
                assets.push(assets[0].clone());
                ImageState::Invalid
            }
            "oversize" => {
                assets[0].size = budget::IMAGE_MAX_BYTES + 1;
                ImageState::Oversize
            }
            "excluded" => {
                assets[0].state = AssetState::Excluded;
                ImageState::Excluded
            }
            "missing" => {
                assets[0].state = AssetState::Missing;
                ImageState::Missing
            }
            _ => unreachable!(),
        };
        fs::write(
            dir.join("assets.json"),
            serde_json::to_vec(&assets).unwrap(),
        )
        .unwrap();
        fixture.resign_synthetic_preview();
        assert_eq!(
            resolve(&fixture.scope, &fixture.ids[0]).state,
            expected,
            "{case}"
        );
    }
    fs::write(dir.join("assets.json"), original).unwrap();
    fixture.resign_synthetic_preview();
    let mut lease = jobs::read_preview_lease(&dir).unwrap().unwrap();
    lease.expires_unix = 1;
    jobs::write_preview_lease(&dir, &lease).unwrap();
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[0]).state,
        ImageState::Invalid
    );
    fixture.unchanged();
}

#[test]
fn imported_images_human_read_does_not_widen_published_model_selection() {
    let fixture = fixture(false);
    let (events, _) = engine::preview_events_page(&fixture.preview, None, Some(20), None).unwrap();
    let mut selection =
        AllowedSelection::for_events(events.iter().map(|row| row.event_id.clone()).collect());
    selection.asset_ids = vec![fixture.ids[0].clone()];
    let plan = engine::plan_with_selection(vec![fixture.preview.clone()], vec![], Some(&selection))
        .unwrap();
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, "human-image-reader").unwrap();
    job_control::run_pending().unwrap();
    let job = engine::job_items_page(&queued.job_id, None, Some(1)).unwrap();
    assert_eq!(job.state, jobs::JobState::CompletedWithIssues, "{job:?}");
    assert_eq!(job.items[0].phase, "committed");
    assert_eq!(job.items[0].outcome.as_deref(), Some("partial"));
    let item = &job.items[0];
    let import = item.import_id.clone().unwrap();
    let revision = item.revision_id.clone().unwrap();
    let dir = catalog::revision_dir(&import, &revision).unwrap();
    let before = jobs::prepared_hashes(&dir).unwrap();
    let scope = ImageScope::Published {
        import_id: import.clone(),
        revision_id: revision.clone(),
    };
    for id in &fixture.ids[..2] {
        assert_eq!(resolve(&scope, id).state, ImageState::Stored);
    }
    let auth = jobs::load_published_auth(&import, Some(&revision), None).unwrap();
    assert_eq!(auth.selection.asset_ids, selection.asset_ids);
    assert!(!auth.selection.permits_asset(&fixture.ids[1]));
    assert_eq!(jobs::prepared_hashes(&dir).unwrap(), before);
    let wrong = ImageScope::Published {
        import_id: import.clone(),
        revision_id: uuid::Uuid::new_v4().to_string(),
    };
    assert_eq!(resolve(&wrong, &fixture.ids[0]).state, ImageState::Invalid);
    catalog::hide_entry(&catalog::open_catalog().unwrap(), &import).unwrap();
    assert_eq!(resolve(&scope, &fixture.ids[0]).state, ImageState::Invalid);
    fixture.unchanged();
}

#[test]
fn imported_images_preview_projection_keeps_order_and_bounds_payloads() {
    let event = serde_json::from_value(json!({"kind":"message", "id":"mixed", "ordinal":0,
        "spans":[], "timeOrigin":"unknown", "role":"user", "complete":true,
        "blocks":[{"kind":"text","text":"before"}, {"kind":"asset","asset_id":"picture"},
            {"kind":"text","text":"after"}, {"kind":"opaque","source_type":"future",
                "payload":{"blobId":"SECRET","mediaType":"application/json","byteLength":99}},
            {"kind":"text","text":"界".repeat(5000)}]}))
    .unwrap();
    let (parts, limited) = super::super::preview_display::ordered(&event);
    let wire = serde_json::to_value(parts).unwrap();
    assert_eq!(wire[0]["text"], "before");
    assert_eq!(wire[1]["asset_id"], "picture");
    assert_eq!(wire[2]["text"], "after");
    assert!(wire[3].get("payload").is_none());
    assert!(!wire.to_string().contains("SECRET"));
    assert!(limited);
    assert_eq!(
        wire.as_array()
            .unwrap()
            .iter()
            .filter_map(|part| part["text"].as_str())
            .map(|text| text.chars().count())
            .sum::<usize>(),
        1024
    );
}

#[test]
fn imported_images_extensionless_original_http_is_full_200_and_thumbnail_is_bounded() {
    let fixture = fixture(true);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let handle = crate::media_server::start().await.unwrap();
        let endpoint = handle.endpoint();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("{}/v1/import-image", endpoint.base_url);
        let scope = serde_json::to_string(&fixture.scope).unwrap();
        let query = |variant| {
            [
                ("t", endpoint.token.as_str()),
                ("s", scope.as_str()),
                ("a", fixture.ids[0].as_str()),
                ("variant", variant),
            ]
        };
        let response = client
            .get(&url)
            .query(&query("original"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.bytes().await.unwrap().as_ref(), fixture.png);
        let response = client
            .head(&url)
            .query(&query("original"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers()["content-length"],
            fixture.png.len().to_string()
        );
        assert!(response.bytes().await.unwrap().is_empty());
        let response = client
            .get(&url)
            .query(&query("thumbnail"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/jpeg");
        let bytes = response.bytes().await.unwrap();
        assert!(bytes.len() < 256 * 1024);
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (480, 480));
        let response = client
            .get(&url)
            .query(&query("original"))
            .header("Origin", "https://example.invalid")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
        let mut bad = query("original");
        bad[0].1 = "bad";
        assert_eq!(
            client.get(&url).query(&bad).send().await.unwrap().status(),
            401
        );
        let mut assets: Vec<AssetRecord> =
            serde_json::from_slice(&fs::read(fixture.dir().join("assets.json")).unwrap()).unwrap();
        let blob = fixture
            .dir()
            .join("blobs")
            .join(assets[0].digest.as_ref().unwrap());
        fs::OpenOptions::new()
            .write(true)
            .open(&blob)
            .unwrap()
            .set_len(budget::IMAGE_MAX_BYTES + 1)
            .unwrap();
        assert!(fs::metadata(&blob).unwrap().len() > budget::IMAGE_MAX_BYTES);
        assert_eq!(
            read_bounded(&blob, budget::IMAGE_MAX_BYTES).unwrap_err(),
            ImageState::Oversize
        );
        assets[0].size = budget::IMAGE_MAX_BYTES + 1;
        fs::write(
            fixture.dir().join("assets.json"),
            serde_json::to_vec(&assets).unwrap(),
        )
        .unwrap();
        fixture.resign_synthetic_preview();
        assert_eq!(
            client
                .get(&url)
                .query(&query("original"))
                .send()
                .await
                .unwrap()
                .status(),
            413
        );
    });
    fixture.unchanged();
}

#[cfg(windows)]
#[test]
fn imported_images_reject_windows_junction_without_reading_or_changing_its_target() {
    use std::os::windows::process::CommandExt;
    let fixture = fixture(false);
    let blobs = fixture.dir().join("blobs");
    let outside = fixture._home.path.join("outside-imports-blobs");
    fs::rename(&blobs, &outside).unwrap();
    struct Junction(PathBuf);
    impl Drop for Junction {
        fn drop(&mut self) {
            let _ = fs::remove_dir(&self.0);
        }
    }
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&blobs)
        .arg(&outside)
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "synthetic junction creation must execute");
    let link = Junction(blobs.clone());
    assert!(is_reparse(&fs::symlink_metadata(&blobs).unwrap()));
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[0]).state,
        ImageState::Invalid
    );
    let digest = hash::sha256_bytes(&fixture.png);
    assert_eq!(fs::read(outside.join(digest)).unwrap(), fixture.png);
    drop(link);
    fs::rename(&outside, &blobs).unwrap();
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[0]).state,
        ImageState::Stored
    );
    fixture.unchanged();
}

/// The symlink boundary is the app-data root: a link at or below it is
/// refused, while the operating system's own links above it (macOS `/var`,
/// ostree `/home`) are not the reader's to judge.
#[cfg(unix)]
#[test]
fn imported_images_refuse_links_below_the_app_root_but_not_above_it() {
    let fixture = fixture(false);
    let blobs = fixture.dir().join("blobs");
    let outside = fixture._home.path.join("outside-imports-blobs");
    fs::rename(&blobs, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &blobs).unwrap();
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[0]).state,
        ImageState::Invalid
    );
    fs::remove_file(&blobs).unwrap();
    fs::rename(&outside, &blobs).unwrap();
    assert_eq!(
        resolve(&fixture.scope, &fixture.ids[0]).state,
        ImageState::Stored
    );
    // The same app root reached through a link above it keeps working.
    let root = crate::paths::app_data_root();
    let alias = root.with_file_name(format!(
        "{}-alias",
        root.file_name().unwrap().to_string_lossy()
    ));
    std::os::unix::fs::symlink(root.parent().unwrap(), &alias).unwrap();
    let through = alias.join(root.file_name().unwrap());
    let previous = std::env::var_os("GROK_APP_HOME");
    std::env::set_var("GROK_APP_HOME", &through);
    let via_link = resolve(&fixture.scope, &fixture.ids[0]).state;
    match previous {
        Some(value) => std::env::set_var("GROK_APP_HOME", value),
        None => std::env::remove_var("GROK_APP_HOME"),
    }
    fs::remove_file(&alias).unwrap();
    assert_eq!(via_link, ImageState::Stored);
    fixture.unchanged();
}

#[test]
fn imported_images_twenty_dense_mixed_rows_fit_the_fixed_transfer_byte_budget() {
    use base64::Engine;
    let fixture = fixture(false);
    let root = fixture.source.parent().unwrap();
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&fixture.png)
    );
    let mut rows = vec![json!({"type":"session_meta","payload":{"id":"dense-images"}})];
    for index in 0..20 {
        let mut content = vec![json!({"type":"input_text","text":"界".repeat(1024)})];
        content.extend((0..64).map(|_| json!({"type":"input_image","image_url":data_url})));
        rows.push(
            json!({"type":"response_item","payload":{"type":"message","role":"user",
            "id":format!("dense-{index}"),"content":content}}),
        );
    }
    let source = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    fs::write(&fixture.source, &source).unwrap();
    let scan = scan::scan_start(vec![(Provider::Codex, root.to_string_lossy().into())]).unwrap();
    let candidate = &scan.candidates[0];
    let (preview, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let (events, next) = engine::preview_events_page(&preview, None, Some(20), None).unwrap();
    assert_eq!(events.len(), 20);
    assert!(next.is_none());
    assert!(events.iter().all(|event| event.asset_ids.len() == 64));
    assert!(events.iter().all(|event| event.display_limited));
    assert!(events.iter().all(|event| event
        .display_parts
        .iter()
        .any(|part| matches!(part, super::super::types::ContentBlock::Asset { .. }))));
    let dir = catalog::staging_dir().join("previews").join(&preview);
    let full = super::super::search::indexed_event_by_id(&dir, &events[0].event_id)
        .unwrap()
        .unwrap();
    assert!(
        matches!(full, super::super::types::ImportedEvent::Message { blocks, .. } if blocks.len() == 65)
    );
    let wire = serde_json::to_vec(&json!({"events":events,"nextCursor":next})).unwrap();
    assert!(
        wire.len() <= 256 * 1024,
        "fixed preview page is {} bytes",
        wire.len()
    );
    assert_eq!(fs::read_to_string(&fixture.source).unwrap(), source);
}
