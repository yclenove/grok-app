use super::*;
use serde_json::json;

#[test]
fn endpoints_and_page_sizes_are_fixed() {
    let openverse = provider_url(RemoteWallpaperSource::Openverse, "misty lake", 3).unwrap();
    assert_eq!(openverse.host_str(), Some("api.openverse.org"));
    assert_eq!(openverse.path(), "/v1/images/");
    assert!(openverse.as_str().contains("page_size=20"));
    assert!(openverse.as_str().contains("page=3"));

    let pexels = provider_url(RemoteWallpaperSource::Pexels, "misty lake", 2).unwrap();
    assert_eq!(pexels.host_str(), Some("api.pexels.com"));
    assert_eq!(pexels.path(), "/v1/search");
    assert!(pexels.as_str().contains("per_page=40"));
    assert!(pexels.as_str().contains("orientation=landscape"));
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
    cache.insert(key.clone(), result, 3, now);
    assert_eq!(cache.get(&key, now).map(|(_, page)| page), Some(3));
    assert!(cache
        .get(&key, now + CACHE_TTL + Duration::from_secs(1))
        .is_none());
}

#[test]
fn replacement_provider_request_cancels_the_previous_generation() {
    let (_first_token, first) = begin_request("first-provider");
    let (second_token, second) = begin_request("second-provider");
    assert!(first.is_cancelled());
    assert!(!second.is_cancelled());
    finish_request("second-provider", second_token);
}
