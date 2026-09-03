use super::*;
use serde_json::json;

fn gallery_item(id: usize) -> WallpaperGalleryItem {
    WallpaperGalleryItem {
        id: format!("item-{id}"),
        thumb_url: format!("https://images.example/{id}.jpg"),
        full_url: format!("https://images.example/{id}.jpg"),
        kind: "image".into(),
        width: Some(1920),
        height: Some(1080),
        source: "pexels".into(),
        username: None,
        post_url: None,
        text_preview: None,
        likes: None,
        local_path: None,
        prompt: None,
        provenance: crate::wallpaper_source::WallpaperProvenance::empty(),
        status_id: None,
        media_index: None,
        media_quality: None,
        media_fingerprint: None,
    }
}

#[test]
fn endpoints_and_page_sizes_are_fixed() {
    let openverse = provider_url(RemoteWallpaperSource::Openverse, "misty lake", 3, None).unwrap();
    assert_eq!(openverse.host_str(), Some("api.openverse.org"));
    assert_eq!(openverse.path(), "/v1/images/");
    assert!(openverse.as_str().contains("page_size=20"));
    assert!(openverse.as_str().contains("page=3"));
    assert_eq!(
        openverse
            .query_pairs()
            .find(|(key, _)| key == PEXELS_CACHE_BUST_PARAM),
        None
    );

    let pexels = provider_url(
        RemoteWallpaperSource::Pexels,
        "misty lake",
        2,
        Some("000000000000000000000000000000010000000000000001"),
    )
    .unwrap();
    assert_eq!(pexels.host_str(), Some("api.pexels.com"));
    assert_eq!(pexels.path(), "/v1/search");
    assert!(pexels.as_str().contains("per_page=40"));
    assert!(pexels.as_str().contains("orientation=landscape"));
    assert_eq!(
        pexels
            .query_pairs()
            .find(|(key, _)| key == "query")
            .map(|(_, value)| value.into_owned())
            .as_deref(),
        Some("misty lake")
    );
    assert!(!pexels.query_pairs().any(|(key, _)| key == "q"));
}

#[test]
fn pexels_requests_use_fresh_credential_free_cache_busters() {
    let first = provider_request_url(RemoteWallpaperSource::Pexels, "misty lake", 1).unwrap();
    let second = provider_request_url(RemoteWallpaperSource::Pexels, "misty lake", 1).unwrap();
    let nonce = |url: &Url| {
        url.query_pairs()
            .find(|(key, _)| key == PEXELS_CACHE_BUST_PARAM)
            .map(|(_, value)| value.into_owned())
            .unwrap()
    };
    let first_nonce = nonce(&first);
    let second_nonce = nonce(&second);

    assert_ne!(first_nonce, second_nonce);
    for value in [&first_nonce, &second_nonce] {
        assert_eq!(value.len(), 48);
        assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert!(provider_url(RemoteWallpaperSource::Pexels, "misty lake", 1, None).is_err());
    assert!(provider_url(
        RemoteWallpaperSource::Pexels,
        "misty lake",
        1,
        Some("pexels-api-key-sentinel"),
    )
    .is_err());
}

#[test]
fn library_queries_drop_wallpaper_and_resolution_modifiers() {
    assert_eq!(
        library_provider_query("aurora mountain lake wallpaper"),
        "aurora mountain lake"
    );
    assert_eq!(
        library_provider_query("极光雪山湖泊 4K 壁纸"),
        "极光雪山湖泊"
    );
    assert_eq!(library_provider_query("富士山 壁紙 8K"), "富士山");
    assert_eq!(library_provider_query("海岸 桌布 UHD"), "海岸");
    assert_eq!(library_provider_query("오로라 배경화면 4k"), "오로라");
    assert_eq!(library_provider_query("wallpaper"), "wallpaper");
    assert_eq!(library_provider_query("wallpaper !!!"), "wallpaper !!!");
}

#[test]
fn openverse_parser_requires_real_provenance_and_license() {
    let page = parse_openverse_page(
        &json!({
            "page_count": 3,
            "results": [{
                "id": "asset-1",
                "title": "Misty mountain",
                "creator": "Photographer",
                "creator_url": "https://creator.example/profile",
                "url": "https://images.example/mountain.jpg",
                "foreign_landing_url": "https://source.example/photo/1",
                "license": "by-sa",
                "license_version": "4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "mature": false
            }, {
                "id": "missing-license",
                "creator": "Unknown",
                "url": "https://images.example/no-license.jpg",
                "foreign_landing_url": "https://source.example/photo/2"
            }]
        }),
        1,
    )
    .unwrap();
    assert!(page.has_more);
    assert_eq!(page.candidates.len(), 1);
    let item = &page.candidates[0];
    assert_eq!(item.source_name, "Openverse");
    assert_eq!(item.license, "CC BY-SA 4.0");
    assert_eq!(item.author_name, "Photographer");
}

#[test]
fn pexels_parser_ignores_upstream_next_url_and_uses_fixed_license() {
    let page = parse_pexels_page(
        &json!({
            "page": 1,
            "per_page": 40,
            "total_results": 80,
            "next_page": "https://attacker.invalid/next",
            "photos": [{
                "id": 42,
                "url": "https://www.pexels.com/photo/misty-lake-42/",
                "photographer": "Photographer",
                "photographer_url": "https://www.pexels.com/@photographer/",
                "alt": "Misty lake",
                "src": {"original": "https://images.pexels.com/photos/42/image.jpeg"}
            }]
        }),
        1,
    )
    .unwrap();
    assert!(page.has_more);
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].license, "Pexels License");
    assert_eq!(page.candidates[0].license_url, PEXELS_LICENSE_URL);
}

#[test]
fn provider_links_reject_insecure_userinfo_and_non_default_ports() {
    assert!(safe_url("http://example.test/image.jpg").is_none());
    assert!(safe_url("https://user:pass@example.test/image.jpg").is_none());
    assert!(safe_url("https://example.test:444/image.jpg").is_none());
    assert_eq!(
        safe_url("https://example.test:443/image.jpg#fragment").as_deref(),
        Some("https://example.test/image.jpg")
    );
}

#[test]
fn cache_expires_and_keeps_the_host_owned_cursor() {
    let search = SearchKey {
        source: RemoteWallpaperSource::Openverse,
        query: "misty lake".into(),
        credential_revision: None,
        contract_version: CONTRACT_VERSION,
    };
    let key = PageKey {
        search,
        first_page: 1,
    };
    let result = RemoteSearchResult::success("openverse", Vec::new(), true, 5);
    let now = Instant::now();
    let mut cache = SearchCache::default();
    cache.insert(
        key.clone(),
        result,
        Some(ProviderContinuation {
            next_page: 3,
            buffered_items: vec![gallery_item(1)],
            upstream_has_more: true,
        }),
        now,
    );
    let continuation = cache
        .get(&key, now)
        .and_then(|(_, continuation)| continuation)
        .expect("cached continuation");
    assert_eq!(continuation.next_page, 3);
    assert_eq!(continuation.buffered_items.len(), 1);
    assert!(continuation.upstream_has_more);
    assert!(cache
        .get(&key, now + CACHE_TTL + Duration::from_secs(1))
        .is_none());
}

#[test]
fn overfetched_results_are_paged_before_advancing_upstream() {
    let continuation = ProviderContinuation {
        next_page: 3,
        buffered_items: (0..25).map(gallery_item).collect(),
        upstream_has_more: false,
    };

    let (first, continuation) = take_buffered_page(continuation).expect("first buffered page");
    assert_eq!(first.len(), RESULT_LIMIT);
    assert_eq!(first.first().map(|item| item.id.as_str()), Some("item-0"));
    assert_eq!(first.last().map(|item| item.id.as_str()), Some("item-19"));

    let (second, continuation) = take_buffered_page(continuation.expect("remaining buffered page"))
        .expect("second buffered page");
    assert_eq!(second.len(), 5);
    assert_eq!(second.first().map(|item| item.id.as_str()), Some("item-20"));
    assert!(continuation.is_none());
}

#[test]
fn continuation_cache_is_ttl_bound_lru_and_credential_scoped() {
    let now = Instant::now();
    let mut cache = SearchCache::default();
    for index in 0..=CACHE_CAPACITY {
        cache.update_continuation(
            &SearchKey {
                source: RemoteWallpaperSource::Pexels,
                query: format!("query-{index}"),
                credential_revision: Some(format!("revision-{index}")),
                contract_version: CONTRACT_VERSION,
            },
            Some(ProviderContinuation {
                next_page: 2,
                buffered_items: Vec::new(),
                upstream_has_more: true,
            }),
            now,
        );
    }
    assert_eq!(cache.continuations.len(), CACHE_CAPACITY);
    let evicted = SearchKey {
        source: RemoteWallpaperSource::Pexels,
        query: "query-0".into(),
        credential_revision: Some("revision-0".into()),
        contract_version: CONTRACT_VERSION,
    };
    assert!(cache.continuation(&evicted, now).is_none());

    let newest = SearchKey {
        source: RemoteWallpaperSource::Pexels,
        query: format!("query-{CACHE_CAPACITY}"),
        credential_revision: Some(format!("revision-{CACHE_CAPACITY}")),
        contract_version: CONTRACT_VERSION,
    };
    let changed_credential = SearchKey {
        credential_revision: Some("rotated".into()),
        ..newest.clone()
    };
    assert!(cache.continuation(&changed_credential, now).is_none());
    assert_eq!(
        cache
            .continuation(&newest, now)
            .map(|continuation| continuation.next_page),
        Some(2)
    );
    assert!(cache
        .continuation(&newest, now + CACHE_TTL + Duration::from_secs(1))
        .is_none());
}

#[test]
fn provider_cancel_before_begin_remains_sticky_and_bounded() {
    let now = Instant::now();
    let mut registry = RequestRegistry::default();
    registry.record_pre_cancel("late-provider", now);
    assert!(registry.take_pre_cancel("late-provider", now));
    assert!(!registry.take_pre_cancel("late-provider", now));

    for index in 0..=PRE_CANCEL_CAPACITY {
        registry.record_pre_cancel(&format!("request-{index}"), now);
    }
    assert_eq!(registry.pre_cancelled.len(), PRE_CANCEL_CAPACITY);
    assert!(!registry.take_pre_cancel("request-0", now));
    assert!(!registry.take_pre_cancel("request-1", now + PRE_CANCEL_TTL + Duration::from_secs(1),));
}

#[tokio::test]
async fn concurrent_provider_probes_preserve_upstream_relevance_order() {
    use futures_util::StreamExt as _;

    let output = buffered_in_input_order(
        [("first", 30_u64), ("second", 0_u64), ("third", 5_u64)],
        3,
        |(name, delay_ms)| async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            name
        },
    )
    .collect::<Vec<_>>()
    .await;

    assert_eq!(output, ["first", "second", "third"]);
}

#[test]
fn replacement_provider_request_cancels_the_previous_generation() {
    let (_first_token, first) = begin_request("first-provider");
    let (second_token, second) = begin_request("second-provider");
    assert!(first.is_cancelled());
    assert!(!second.is_cancelled());
    finish_request("second-provider", second_token);
}
