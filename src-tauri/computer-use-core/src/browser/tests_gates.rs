use super::*;
use crate::error::BrokerError;
use crate::pairing::ExtensionPairing;

#[cfg(any(test, feature = "test-support"))]
fn gate_browser_borrow_return_disconnect() -> Result<(), String> {
    let host = ExistingTabHost::new();
    host.set_installed_extension_id("pw-ext-installed");
    let token = host.handshake_pairing().map_err(|e| e.to_string())?;
    let origin = "chrome-extension://pw-ext-installed/";
    let ext = Some("pw-ext-installed");
    let root = std::env::temp_dir().join(format!("cu-s83-{}", uuid::Uuid::new_v4()));
    let worker = std::sync::Arc::new(RecordingBrowserWorker::new(root.clone()));
    host.set_worker(worker.clone());
    let grant = |tab_id: &str, url: &str, index: u32| {
        host.share_and_grant(TabAttachment {
            pairing_token: Some(&token),
            origin,
            extension_id: ext,
            session: "sess-a",
            run_id: "run-a",
            tab_id,
            title: tab_id,
            url,
            home_index: Some(index),
            document_generation: Some(1),
            connection_generation: Some(1),
            focused: true,
        })
    };

    let home_url = "https://example.test/home";
    let granted = grant("tab-home", home_url, 2).map_err(|e| e.to_string())?;
    if !granted.user_owned
        || !granted.borrowed
        || granted.home_index != Some(2)
        || granted.url != home_url
        || !granted.focused
        || granted.preview_generation == 0
    {
        return Err(format!(
            "borrow must record home index/url/focus/preview: {granted:?}"
        ));
    }

    match host.navigate(
        "run-a",
        "tab-home",
        "https://example.test/agent",
        "nav-home",
    ) {
        Err(BrokerError::Schema(message)) if message.contains("extension transport") => {}
        other => {
            return Err(format!(
                "existing tab navigation must fail closed until its transport owns the action: {other:?}"
            ));
        }
    }
    if !worker.gotos.lock().is_empty() {
        return Err("existing tab navigation must never fall through to the managed worker".into());
    }
    let restored = host
        .return_borrowed("run-a", "tab-home")
        .map_err(|e| e.to_string())?;
    if restored.closed
        || restored.borrowed
        || restored.url != home_url
        || restored.home_index != Some(2)
        || !restored.focused
    {
        return Err(format!(
            "return must restore home without closing: {restored:?}"
        ));
    }
    if host
        .list_for_run("run-a")
        .iter()
        .any(|t| t.tab_id == "tab-home")
    {
        return Err("returned tab must leave the model list".into());
    }
    if host.observe("run-a", "tab-home").is_ok() {
        return Err("observe after return must fail; grant ended".into());
    }
    if host.is_closed("tab-home") {
        return Err("return closed the user tab".into());
    }
    if host.return_borrowed("run-a", "tab-home").is_ok() {
        return Err("second return of an already-returned tab must fail".into());
    }
    let regranted = grant("tab-home", home_url, 2).map_err(|e| e.to_string())?;
    if !regranted.borrowed || regranted.closed || regranted.url != home_url {
        return Err(format!(
            "re-grant after return must borrow again: {regranted:?}"
        ));
    }
    host.observe("run-a", "tab-home")
        .map_err(|e| format!("observe after re-grant must succeed: {e}"))?;
    host.return_borrowed("run-a", "tab-home")
        .map_err(|e| e.to_string())?;

    let unfocused = host
        .share_and_grant(TabAttachment {
            pairing_token: Some(&token),
            origin,
            extension_id: ext,
            session: "sess-a",
            run_id: "run-a",
            tab_id: "tab-unfocus",
            title: "tab-unfocus",
            url: "https://example.test/unfocus",
            home_index: Some(8),
            document_generation: Some(1),
            connection_generation: Some(1),
            focused: false,
        })
        .map_err(|e| e.to_string())?;
    if unfocused.focused || !unfocused.borrowed {
        return Err(format!(
            "grant must record original unfocused state: {unfocused:?}"
        ));
    }
    let restored_unfocused = host
        .return_borrowed("run-a", "tab-unfocus")
        .map_err(|e| e.to_string())?;
    if restored_unfocused.focused || restored_unfocused.closed || restored_unfocused.borrowed {
        return Err(format!(
            "return must restore unfocused home: {restored_unfocused:?}"
        ));
    }

    grant("tab-user-nav", "https://example.test/start", 5).map_err(|e| e.to_string())?;
    host.note_user_navigation("tab-user-nav", "https://example.test/user-kept")
        .map_err(|e| e.to_string())?;
    let kept = host
        .return_borrowed("run-a", "tab-user-nav")
        .map_err(|e| e.to_string())?;
    if kept.closed || kept.borrowed || kept.url != "https://example.test/user-kept" {
        return Err(format!("return must keep user navigation: {kept:?}"));
    }

    let stop = grant("tab-stop", "https://example.test/stop", 1).map_err(|e| e.to_string())?;
    let disconnected = host
        .disconnect_tab("tab-stop", TabDisconnect::Stop)
        .map_err(|e| e.to_string())?;
    if disconnected.closed
        || disconnected.disconnect_reason.as_deref() != Some("stop")
        || disconnected.preview_generation != 0
    {
        return Err(format!("stop state {disconnected:?}"));
    }
    if host.observe("run-a", "tab-stop").is_ok() || host.act("run-a", "tab-stop").is_ok() {
        return Err("disconnected tab must refuse observe/act".into());
    }
    if host
        .navigate("run-a", "tab-stop", "https://example.test/x", "nav-stop")
        .is_ok()
    {
        return Err("disconnected tab must refuse navigate".into());
    }
    match host.reconnect_tab(
        "run-a",
        "tab-stop",
        stop.document_generation.saturating_add(1),
        stop.connection_generation.saturating_add(1),
    ) {
        Err(e) if e.to_string().contains("identity mismatch") => {}
        other => return Err(format!("stale document reconnect: {other:?}")),
    }
    match host.reconnect_tab(
        "run-a",
        "tab-stop",
        stop.document_generation,
        stop.connection_generation,
    ) {
        Err(e) if e.to_string().contains("identity mismatch") => {}
        other => return Err(format!("stale connection reconnect: {other:?}")),
    }
    if host
        .reconnect_tab(
            "run-b",
            "tab-stop",
            stop.document_generation,
            stop.connection_generation.saturating_add(1),
        )
        .is_ok()
    {
        return Err("run-b must not reconnect run-a tab".into());
    }
    let live = host
        .reconnect_tab(
            "run-a",
            "tab-stop",
            stop.document_generation,
            stop.connection_generation.saturating_add(1),
        )
        .map_err(|e| e.to_string())?;
    if !live.borrowed
        || live.closed
        || live.preview_generation != 0
        || live.disconnect_reason.is_some()
    {
        return Err(format!(
            "reconnect must restore grant and drop preview: {live:?}"
        ));
    }
    host.observe("run-a", "tab-stop")
        .map_err(|e| e.to_string())?;

    let reasons = [
        ("tab-disc", TabDisconnect::Disconnect, "disconnect", false),
        (
            "tab-ext",
            TabDisconnect::ExtensionUpdate,
            "extension_update",
            false,
        ),
        (
            "tab-bexit",
            TabDisconnect::BrowserExit,
            "browser_exit",
            false,
        ),
        ("tab-aexit", TabDisconnect::AppExit, "app_exit", false),
        ("tab-close", TabDisconnect::TabClose, "tab_close", true),
    ];
    for (id, reason, reason_str, expect_closed) in reasons {
        grant(id, "https://example.test/d", 1).map_err(|e| e.to_string())?;
        let info = host.disconnect_tab(id, reason).map_err(|e| e.to_string())?;
        if info.closed != expect_closed || info.disconnect_reason.as_deref() != Some(reason_str) {
            return Err(format!("{reason_str} state {info:?}"));
        }
        if expect_closed {
            match host.reconnect_tab("run-a", id, 1, 2) {
                Err(e)
                    if matches!(e, BrokerError::DeadTarget)
                        || e.to_string().contains("directed target is gone") => {}
                other => return Err(format!("closed tab reconnect: {other:?}")),
            }
            if !host.is_closed(id) {
                return Err("tab close must stay closed".into());
            }
            if host.return_borrowed("run-a", id).is_ok() {
                return Err("return must not resurrect a closed user tab".into());
            }
        } else {
            if host.observe("run-a", id).is_ok() {
                return Err(format!("{reason_str} must refuse observe"));
            }
            if host.is_closed(id) {
                return Err(format!("{reason_str} closed the user tab"));
            }
        }
    }

    if !host.list_for_run("run-b").is_empty() {
        return Err("run-b must not see run-a existing tabs".into());
    }
    if host.observe("run-b", "tab-stop").is_ok() {
        return Err("run-b must not observe run-a tab".into());
    }
    if host.return_borrowed("run-b", "tab-stop").is_ok() {
        return Err("run-b must not return run-a tab".into());
    }

    grant("tab-cancel", "https://example.test/cancel", 4).map_err(|e| e.to_string())?;
    let after = host.cancel_run("run-a").map_err(|e| e.to_string())?;
    let user = after
        .iter()
        .find(|t| t.tab_id == "tab-cancel")
        .ok_or("missing user tab after cancel")?;
    if user.closed {
        return Err("cancel must not close the user's original tab".into());
    }
    if user.disconnect_reason.as_deref() != Some("stop") {
        return Err(format!("cancel disconnect {user:?}"));
    }
    if host.is_closed("tab-cancel") {
        return Err("cancel flipped the user tab closed flag".into());
    }
    if host.observe("run-a", "tab-cancel").is_ok() {
        return Err("cancel leftover observe must fail".into());
    }

    let _ = std::fs::remove_dir_all(&root);
    println!("gate: browser_borrow_return_disconnect");
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
pub fn run_browser_gates() -> Result<(), String> {
    let host = ExistingTabHost::new();
    host.set_installed_extension_id("pw-ext-installed");

    let fake = host.attach_existing(TabAttachment {
        session: "sess-a",
        run_id: "run-a",
        tab_id: "tab-user",
        title: "Docs",
        url: "https://example.test/",
        origin: "chrome-extension://not-the-real-id/",
        extension_id: Some("not-the-real-id"),
        pairing_token: None,
        home_index: Some(3),
        document_generation: None,
        connection_generation: None,
        focused: true,
    });
    if fake.is_ok() {
        return Err("extension-shaped Origin must not pair".into());
    }
    let id_only = host.attach_existing(TabAttachment {
        session: "sess-a",
        run_id: "run-a",
        tab_id: "tab-user",
        title: "Docs",
        url: "https://example.test/",
        origin: "chrome-extension://pw-ext-installed/",
        extension_id: Some("pw-ext-installed"),
        pairing_token: None,
        home_index: Some(3),
        document_generation: None,
        connection_generation: None,
        focused: true,
    });
    if id_only.is_ok() {
        return Err("extension id without challenge must not pair".into());
    }
    let ch = host.begin_pairing_challenge();
    host.confirm_pairing_app().map_err(|e| e.to_string())?;
    if host.complete_pairing("wrong-proof").is_ok() {
        return Err("invalid extension proof must fail".into());
    }
    let token = host
        .complete_pairing(&ExtensionPairing::extension_response(&ch))
        .map_err(|e| e.to_string())?;
    let a = host
        .share_and_grant(TabAttachment {
            pairing_token: Some(&token),
            origin: "chrome-extension://pw-ext-installed/",
            extension_id: Some("pw-ext-installed"),
            session: "sess-a",
            run_id: "run-a",
            tab_id: "tab-user",
            title: "Docs",
            url: "https://example.test/",
            home_index: Some(3),
            document_generation: Some(1),
            connection_generation: Some(1),
            focused: true,
        })
        .map_err(|e| e.to_string())?;
    if !a.user_owned || !a.borrowed || a.home_index != Some(3) {
        return Err(format!("borrow record missing: {:?}", a));
    }

    let b = host.share_and_grant(TabAttachment {
        pairing_token: Some(&token),
        origin: "chrome-extension://pw-ext-installed/",
        extension_id: Some("pw-ext-installed"),
        session: "sess-b",
        run_id: "run-b",
        tab_id: "tab-user",
        title: "Docs",
        url: "https://example.test/",
        home_index: Some(3),
        document_generation: Some(1),
        connection_generation: Some(1),
        focused: true,
    });
    if b.is_ok() {
        return Err("second session must not share the user tab".into());
    }
    println!("gate: browser_two_session_isolation");

    let listed_b = host.list_for_run("run-b");
    if !listed_b.is_empty() {
        return Err("run-b must not see run-a tabs".into());
    }
    if host.observe("run-b", "tab-user").is_ok() {
        return Err("run-b must not observe run-a tab".into());
    }

    let returned = host
        .return_borrowed("run-a", "tab-user")
        .map_err(|e| e.to_string())?;
    if returned.borrowed || returned.closed || returned.home_index != Some(3) {
        return Err(format!("return must keep user tab: {:?}", returned));
    }
    println!("gate: browser_borrow_return");

    let profile_runtime_root =
        std::env::temp_dir().join(format!("grok-cu-profiles-gate-{}", uuid::Uuid::new_v4()));
    let profile_worker =
        std::sync::Arc::new(RecordingBrowserWorker::new(profile_runtime_root.clone()));
    host.set_profile_root(profile_runtime_root.join("profiles"));
    host.set_worker(profile_worker);
    let managed = host
        .open_managed_profile("sess-a", "run-a", "profile-1")
        .map_err(|e| e.to_string())?;
    let steal = host.open_managed_profile("sess-b", "run-b", "profile-1");
    if steal.is_ok() {
        return Err("managed profile must have one owner".into());
    }
    host.open_managed_profile("sess-a", "run-a", "alice")
        .map_err(|e| e.to_string())?;
    if !profile_runtime_root.join("profiles").join("alice").is_dir() {
        return Err("managed profile must live on disk under the Host root".into());
    }
    if !host.list_managed_profiles().iter().any(|n| n == "alice") {
        return Err("list_managed_profiles missed alice".into());
    }

    let after = host.cancel_run("run-a").map_err(|e| e.to_string())?;
    let user = after
        .iter()
        .find(|t| t.tab_id == "tab-user")
        .ok_or("missing user tab after cancel")?;
    let app = after
        .iter()
        .find(|t| t.tab_id == managed.tab_id)
        .ok_or("missing managed tab after cancel")?;
    if user.closed {
        return Err("cancel must not close the user's original tab".into());
    }
    if !app.closed {
        return Err("cancel must close App-owned managed tab".into());
    }
    if host.is_closed("tab-user") {
        return Err("user tab closed flag flipped".into());
    }
    let _ = std::fs::remove_dir_all(&profile_runtime_root);
    println!("gate: browser_cancel_keeps_user_tab");

    host.revoke_pairing();
    if host.observe("run-a", "tab-user").is_ok() {
        return Err("revoked pairing must drop the previous connection".into());
    }
    let rotated = {
        let token = host.handshake_pairing().map_err(|e| e.to_string())?;
        let ch = host.rotate_pairing().map_err(|e| e.to_string())?;
        if host
            .attach_existing(TabAttachment {
                session: "sess-c",
                run_id: "run-c",
                tab_id: "tab-rot",
                title: "Docs",
                url: "https://example.test/",
                origin: "chrome-extension://pw-ext-installed/",
                extension_id: Some("pw-ext-installed"),
                pairing_token: Some(&token),
                home_index: Some(1),
                document_generation: Some(1),
                connection_generation: Some(1),
                focused: true,
            })
            .is_ok()
        {
            return Err("rotated pairing must reject the old key".into());
        }
        host.confirm_pairing_app().map_err(|e| e.to_string())?;
        host.complete_pairing(&ExtensionPairing::extension_response(&ch))
            .map_err(|e| e.to_string())?
    };
    let _ = rotated;
    println!("gate: browser_pairing_challenge_revoke");

    {
        let host = ExistingTabHost::new();
        host.set_installed_extension_id("pw-ext-installed");
        let token = host.handshake_pairing().map_err(|e| e.to_string())?;
        host.offer_shared_tab(SharedTabOffer {
            pairing_token: &token,
            origin: "chrome-extension://pw-ext-installed/",
            extension_id: Some("pw-ext-installed"),
            tab_id: "tab-secret",
            title: "Secret Inbox",
            url: "https://mail.test/inbox",
            browser_id: "chrome",
            profile_id: "default",
            document_generation: 1,
            connection_generation: 1,
            focused: true,
        })
        .map_err(|e| e.to_string())?;
        let listed = host.list_for_run("run-a");
        if listed
            .iter()
            .any(|t| t.title.contains("Secret") || t.url.contains("mail.test"))
        {
            return Err("unshared tab must not appear on the model list".into());
        }
        match host.observe("run-a", "tab-secret") {
            Ok(info) => {
                return Err(format!("ungranted tab leaked {:?}", info.title));
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("Secret") || msg.contains("mail.test") {
                    return Err(format!("observe error leaked tab identity: {msg}"));
                }
            }
        }
        if host
            .picker_grant(TabAttachment {
                pairing_token: Some(&token),
                origin: "chrome-extension://pw-ext-installed/",
                extension_id: Some("pw-ext-installed"),
                session: "sess-a",
                run_id: "run-a",
                tab_id: "tab-secret",
                title: "",
                url: "",
                home_index: Some(1),
                document_generation: Some(0),
                connection_generation: Some(1),
                focused: true,
            })
            .is_ok()
        {
            return Err("grant without document generation must fail".into());
        }
        let granted = host
            .picker_grant(TabAttachment {
                pairing_token: Some(&token),
                origin: "chrome-extension://pw-ext-installed/",
                extension_id: Some("pw-ext-installed"),
                session: "sess-a",
                run_id: "run-a",
                tab_id: "tab-secret",
                title: "",
                url: "",
                home_index: Some(1),
                document_generation: Some(4),
                connection_generation: Some(2),
                focused: true,
            })
            .map_err(|e| e.to_string())?;
        if granted.document_generation != 4 || granted.connection_generation != 2 {
            return Err(format!("grant identity missing gens {granted:?}"));
        }
        if host
            .list_for_run("run-a")
            .iter()
            .all(|t| t.tab_id != "tab-secret")
        {
            return Err("granted tab missing from run-a list".into());
        }
        if !host.list_for_run("run-b").is_empty() {
            return Err("run-b must not see run-a granted tab".into());
        }
        if host.observe("run-b", "tab-secret").is_ok() {
            return Err("run-b must not observe run-a granted tab".into());
        }
        host.offer_shared_tab(SharedTabOffer {
            pairing_token: &token,
            origin: "chrome-extension://pw-ext-installed/",
            extension_id: Some("pw-ext-installed"),
            tab_id: "tab-other",
            title: "Other Window",
            url: "https://other.test/",
            browser_id: "chrome",
            profile_id: "default",
            document_generation: 2,
            connection_generation: 1,
            focused: true,
        })
        .map_err(|e| e.to_string())?;
        if host
            .list_for_run("run-a")
            .iter()
            .any(|t| t.tab_id == "tab-other" || t.title.contains("Other"))
        {
            return Err("shared-but-not-granted tab must stay off the model list".into());
        }
        println!("gate: browser_share_grant_isolation");
    }

    let host = ExistingTabHost::new();
    host.set_installed_extension_id("pw-ext-installed");
    let token = host.handshake_pairing().map_err(|e| e.to_string())?;
    host.share_and_grant(TabAttachment {
        pairing_token: Some(&token),
        origin: "chrome-extension://pw-ext-installed/",
        extension_id: Some("pw-ext-installed"),
        session: "sess-a",
        run_id: "run-a",
        tab_id: "tab-nav",
        title: "Docs",
        url: "https://example.test/a",
        home_index: Some(1),
        document_generation: Some(1),
        connection_generation: Some(1),
        focused: true,
    })
    .map_err(|e| e.to_string())?;
    if host
        .navigate("run-a", "tab-nav", "https://example.test/b", "nav-noworker")
        .is_ok()
    {
        return Err("navigate must not succeed in-memory without a worker".into());
    }
    let root = std::env::temp_dir().join(format!("cu-dl-{}", uuid::Uuid::new_v4()));
    let worker = std::sync::Arc::new(RecordingBrowserWorker::new(root.clone()));
    host.set_worker(worker.clone());
    host.set_staging_root(root.clone());
    let gone = host.navigate("run-b", "tab-nav", "https://example.test/b", "nav-gone");
    if gone.is_ok() {
        return Err("navigate must not use another run's tab".into());
    }
    worker.set_actual_url(Some("https://evil.test/hijack".into()));
    if host
        .navigate("run-a", "tab-nav", "https://example.test/b", "nav-hijack")
        .is_ok()
    {
        return Err("navigate must reject a worker URL that is not the requested target".into());
    }
    worker.set_actual_url(Some("https://example.test/landed".into()));
    let bounced = host
        .navigate(
            "run-a",
            "tab-nav",
            "https://example.test/start",
            "nav-start",
        )
        .map_err(|e| e.to_string())?;
    if bounced.url != "https://example.test/landed" {
        return Err(format!(
            "same-origin redirect must be recorded, got {}",
            bounced.url
        ));
    }
    worker.set_actual_url(Some("http://169.254.169.254/latest".into()));
    if host
        .navigate("run-a", "tab-nav", "https://example.test/ok", "nav-meta")
        .is_ok()
    {
        return Err("redirect onto instance metadata must fail".into());
    }
    worker.set_actual_url(None);
    let moved = host
        .navigate("run-a", "tab-nav", "https://example.test/b", "nav-b")
        .map_err(|e| e.to_string())?;
    if moved.url != "https://example.test/b" {
        return Err(format!("navigate did not re-check url: {}", moved.url));
    }
    if worker.gotos.lock().len() < 2 {
        return Err("navigate must invoke the managed worker".into());
    }
    println!("gate: browser_navigate_rechecks_target");

    match crate::staging::reject_model_path(Some("/tmp/evil.bin")) {
        Err(_) => {}
        Ok(()) => return Err("model path must be rejected".into()),
    }
    let dest = host
        .stage_download_with_action(
            "run-a",
            "tab-nav",
            1,
            "dl-1",
            "rec-snap",
            Some("rec-ref"),
            "report.bin",
        )
        .map_err(|e| e.to_string())?;
    let body = std::fs::read_to_string(&dest).unwrap_or_default();
    if !body.starts_with("worker:") {
        return Err(format!("download must come from the worker, got {body:?}"));
    }
    if *worker.downloads.lock() == 0 {
        return Err("download must invoke the managed worker".into());
    }
    if dest.to_string_lossy().contains("evil") {
        return Err("staging used a model path".into());
    }
    println!("gate: browser_download_staging");

    host.open_managed_profile("sess-a", "run-a", "p-eval")
        .map_err(|e| e.to_string())?;
    match host.act_managed(
        "run-a",
        "p-eval",
        "act-eval",
        "evaluate",
        ManagedLocator::default(),
        serde_json::json!({"script": "1+1"}),
    ) {
        Err(e) if e.to_string().contains("evaluate") => {}
        other => return Err(format!("evaluate must be forbidden, got {other:?}")),
    }
    println!("gate: browser_evaluate_forbidden");
    let staged =
        crate::staging::staging_file(&root, "run-a", "note.txt").map_err(|e| e.to_string())?;
    std::fs::write(&staged, "hello-upload").map_err(|e| e.to_string())?;
    host.upload_managed("run-a", "tab-nav", "up-1", "#file", &staged)
        .map_err(|e| e.to_string())?;
    let evil = std::env::temp_dir().join(format!("cu-evil-{}.bin", uuid::Uuid::new_v4()));
    std::fs::write(&evil, "nope").map_err(|e| e.to_string())?;
    if host
        .upload_managed("run-a", "tab-nav", "up-2", "#file", &evil)
        .is_ok()
    {
        let _ = std::fs::remove_file(&evil);
        return Err("upload outside staging must fail".into());
    }
    let _ = std::fs::remove_file(&evil);
    if crate::browser::is_allowed_navigate_url("javascript:alert(1)").is_ok()
        || crate::browser::is_allowed_navigate_url("file:///c:/windows/notepad.exe").is_ok()
        || crate::browser::is_allowed_navigate_url("https://user:pass@example.com/").is_ok()
    {
        return Err("javascript/file/credentials must stay rejected".into());
    }
    println!("gate: browser_upload_staging_only");
    let _ = std::fs::remove_dir_all(&root);
    gate_browser_borrow_return_disconnect()?;
    Ok(())
}

#[cfg(test)]
mod browser_gate_tests {
    #[test]
    fn browser_borrow_return_disconnect() {
        super::gate_browser_borrow_return_disconnect().expect("s8.3 borrow/return/disconnect");
    }
}
