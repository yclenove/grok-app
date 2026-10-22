use super::*;

crate::session_migration::test_home::fixture_env_contract_tests!(["GROK_APP_VOICE"], mock_stt);

#[test]
fn extract_transcript_text_field() {
    assert_eq!(
        extract_transcript(r#"{"text":"hello world"}"#),
        "hello world"
    );
}

#[test]
fn extract_transcript_segments() {
    let body = r#"{"segments":[{"text":"foo"},{"text":"bar"}]}"#;
    assert_eq!(extract_transcript(body), "foo bar");
}

#[test]
fn extract_empty_object() {
    assert!(extract_transcript("{}").is_empty());
}

#[test]
fn guess_mime_webm() {
    assert_eq!(guess_mime("a.webm"), "audio/webm");
    assert_eq!(guess_mime("a.mp3"), "audio/mpeg");
}

#[tokio::test]
async fn transcribe_without_auth_is_not_available() {
    // Always take the production no-auth branch before decode/client/network.
    // Never inspect this machine's token, secrets or persisted STT route.
    for audio in ["AAAA", "", "not base64!"] {
        let r =
            official_transcribe_with_auth(audio.into(), Some("t.webm".into()), None, None).await;
        assert!(!r.ok);
        assert!(r.text.is_none());
        assert_eq!(r.error_class.as_deref(), Some("not_available"));
        assert_eq!(
            r.error.as_deref(),
            Some("no speech auth (official login or API key required)")
        );
    }
}

#[tokio::test]
async fn authenticated_preflight_rejects_invalid_and_empty_audio_without_network() {
    // Non-secret fixture token; both inputs return before creating an HTTP client.
    for (audio, class, message) in [
        ("not base64!", "unknown", "invalid audio encoding:"),
        ("", "no_speech", "empty audio"),
        ("  ", "no_speech", "empty audio"),
    ] {
        let r = official_transcribe_with_auth(
            audio.into(),
            None,
            None,
            Some(("fixture-token-not-real".into(), "api_key")),
        )
        .await;
        assert!(!r.ok);
        assert!(r.text.is_none());
        assert_eq!(r.error_class.as_deref(), Some(class));
        assert!(r.error.as_deref().unwrap().starts_with(message));
    }
}

#[tokio::test]
async fn mock_stt() {
    let _guard = crate::session_migration::test_home::TestEnvGuard::acquire(&["GROK_APP_VOICE"]);
    std::env::set_var("GROK_APP_VOICE", "mock");
    let r = transcribe_base64("AAAA", Some("audio/wav"), Some("en"))
        .await
        .unwrap();
    assert_eq!(r.text, "mock transcript from voice dictation");
    assert_eq!(r.duration, Some(1.0));
    assert_eq!(r.language.as_deref(), Some("en"));
}

#[test]
fn mentions_param_recognizes_common_gateway_wording() {
    assert!(mentions_param(
        r#"{"error":{"message":"unknown param `prompt`","type":"invalid_request_error"}}"#,
        "prompt"
    ));
    assert!(mentions_param(
        "unexpected field `language` is not supported",
        "language"
    ));
    assert!(mentions_param("invalid parameter `model`", "model"));
    assert!(!mentions_param(
        r#"{"error":{"message":"model too slow"}}"#,
        "prompt"
    ));
    assert!(!mentions_param("file too large", "language"));
}

#[test]
fn stt_provider_for_base_url_matches_presets() {
    assert_eq!(
        stt_provider_for_base_url("http://127.0.0.1:8000/v1"),
        "local"
    );
    assert_eq!(
        stt_provider_for_base_url("https://api.groq.com/openai/v1"),
        "groq"
    );
    assert_eq!(
        stt_provider_for_base_url("https://api.openai.com/v1/"),
        "openai"
    );
    assert_eq!(
        stt_provider_for_base_url("https://api.mistral.ai/v1"),
        "mistral"
    );
    assert_eq!(stt_provider_for_base_url(""), "custom");
    assert_eq!(
        stt_provider_for_base_url("https://api.example.com/v1"),
        "custom"
    );
}

#[test]
fn is_local_stt_url_detects_loopback() {
    assert!(is_local_stt_url("http://127.0.0.1:8000/v1"));
    assert!(is_local_stt_url("http://localhost:8000/v1"));
    assert!(is_local_stt_url("http://[::1]:8000/v1"));
    assert!(!is_local_stt_url("https://api.groq.com/openai/v1"));
    assert!(!is_local_stt_url(""));
}

#[test]
fn custom_401_classifies_as_stt_key_not_auth() {
    let r = finish_http_error(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error":{"message":"invalid api key"}}"#,
        SttErrorCtx::Custom { local: false },
    );
    assert_eq!(r.error_class.as_deref(), Some("stt_key"));
    assert!(r.error.unwrap_or_default().contains("API Key"));
}

#[test]
fn official_401_classifies_as_auth() {
    let r = finish_http_error(
        reqwest::StatusCode::UNAUTHORIZED,
        "{}",
        SttErrorCtx::Official,
    );
    assert_eq!(r.error_class.as_deref(), Some("auth"));
}

#[test]
fn custom_stt_url_joins_base() {
    assert_eq!(
        custom_stt_url("https://api.groq.com/openai/v1"),
        "https://api.groq.com/openai/v1/audio/transcriptions"
    );
}

#[test]
fn custom_stt_url_trims_trailing_slash() {
    assert_eq!(
        custom_stt_url("https://api.example.com/v1/"),
        "https://api.example.com/v1/audio/transcriptions"
    );
}

#[test]
fn custom_stt_url_empty_base_is_empty() {
    assert_eq!(custom_stt_url(""), "");
    assert_eq!(custom_stt_url("   "), "");
}

#[test]
fn resolve_zh_script_explicit_prefs_win() {
    assert_eq!(
        resolve_zh_script("simplified", "zh-TW"),
        ZhScript::Simplified
    );
    assert_eq!(
        resolve_zh_script("traditional", "en"),
        ZhScript::Traditional
    );
    assert_eq!(
        resolve_zh_script("Simplified", "zh-TW"),
        ZhScript::Simplified
    );
}

#[test]
fn resolve_zh_script_auto_follows_locale() {
    assert_eq!(resolve_zh_script("auto", "zh-TW"), ZhScript::Traditional);
    assert_eq!(resolve_zh_script("auto", "zh_Hant"), ZhScript::Traditional);
    assert_eq!(resolve_zh_script("auto", "zh"), ZhScript::Simplified);
    assert_eq!(resolve_zh_script("auto", "en"), ZhScript::Simplified);
    assert_eq!(resolve_zh_script("", "en"), ZhScript::Simplified);
}

#[test]
fn stt_script_prompt_zh_language_uses_locale_script() {
    let p = stt_script_prompt("zh", "auto", "zh-TW").unwrap();
    assert!(p.contains("繁體中文"));
    let p = stt_script_prompt("zh", "auto", "en").unwrap();
    assert!(p.contains("简体中文"));
}

#[test]
fn stt_script_prompt_explicit_pref_without_language_hint() {
    let p = stt_script_prompt("", "traditional", "en").unwrap();
    assert!(p.contains("繁體中文"));
    let p = stt_script_prompt("en", "simplified", "zh-TW").unwrap();
    assert!(p.contains("简体中文"));
}

#[test]
fn stt_script_prompt_non_zh_auto_is_none() {
    assert!(stt_script_prompt("en", "auto", "en").is_none());
    assert!(stt_script_prompt("", "auto", "zh").is_none());
    assert!(stt_script_prompt("ja", "auto", "zh-TW").is_none());
}

/// Local end-to-end check against a real OpenAI-compatible endpoint.
/// Run explicitly with `--ignored` and `GROK_APP_E2E_STT=1`. The port defaults to 8799 (the
/// fake strict server) but can point at any local server:
/// `GROK_APP_E2E_STT_PORT=8000`. When `GROK_APP_E2E_STT_AUDIO` is a file
/// path, that audio is transcribed and only `ok` is asserted (real
/// transcripts are content-dependent); otherwise the fake server's canned
/// text is asserted. Never runs in CI.
#[tokio::test]
#[ignore = "requires explicitly authorized local STT endpoint and audio fixture"]
async fn custom_transcribe_e2e_against_local_server() {
    if std::env::var("GROK_APP_E2E_STT")
        .map(|v| v != "1")
        .unwrap_or(true)
    {
        return;
    }
    let port = std::env::var("GROK_APP_E2E_STT_PORT").unwrap_or_else(|_| "8799".into());
    let audio_path = std::env::var("GROK_APP_E2E_STT_AUDIO").ok();
    let settings = crate::store::AppSettings {
        stt_engine: "custom".into(),
        stt_custom_base_url: Some(format!("http://127.0.0.1:{port}/v1")),
        stt_custom_model: Some("whisper-test".into()),
        stt_custom_language: Some("zh".into()),
        ..Default::default()
    };
    let audio = match &audio_path {
        Some(p) => {
            let bytes = std::fs::read(p).expect("GROK_APP_E2E_STT_AUDIO must be a readable file");
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
        None => base64::engine::general_purpose::STANDARD.encode(vec![7u8; 2048]),
    };
    let r = custom_transcribe(
        &settings,
        audio,
        Some("audio.webm".into()),
        None,
        Some("zh-TW".into()),
    )
    .await;
    assert!(r.ok, "expected ok, got error={:?}", r.error);
    assert_eq!(r.error_class, None);
    if audio_path.is_none() {
        assert!(r.text.unwrap_or_default().contains("本地模拟"));
    }
}
