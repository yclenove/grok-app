//! Broker and adapter navigation lifecycle regression tests.

use super::*;

#[test]
fn broker_navigation_replays_the_exact_successor_without_a_second_dispatch() {
    let world = NavigationBrokerWorld::new(&["a"]);
    let grant = world.grant("navigation-owner", &["a"]);
    let view = world.view("a");
    let (origin, generation) = world.broker.authorized_target(&grant.run_id).unwrap();
    let args = navigation_tool_args(
        &grant.run_id,
        Some(&origin),
        generation,
        "navigate-exactly-once",
        "url",
        Some("https://fixture.invalid/successor"),
    );
    world
        .broker
        .preflight_tool_permission(&grant.token, "computer_navigate", &args)
        .unwrap();
    let first = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        args.clone(),
        None,
    );
    assert_eq!(first["isError"], false, "{first}");
    let first_payload = tool_payload(&first);
    assert_ne!(first_payload["targetId"], origin);
    assert_eq!(first_payload["targetGeneration"], generation + 1);
    assert_eq!(first_payload["generation"], 2);
    assert_eq!(view.navigation_generation(), 2);
    assert_eq!(
        world.broker.authorized_target(&grant.run_id).unwrap(),
        (
            first_payload["targetId"].as_str().unwrap().to_string(),
            generation + 1
        )
    );

    world
        .broker
        .preflight_tool_permission(&grant.token, "computer_navigate", &args)
        .unwrap();
    let replay = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        args.clone(),
        None,
    );
    assert_eq!(tool_payload(&replay), first_payload);
    assert_eq!(view.navigation_generation(), 2);

    let mut without_target = args.clone();
    without_target.as_object_mut().unwrap().remove("targetId");
    let replay_without_target = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        without_target,
        None,
    );
    assert_eq!(tool_payload(&replay_without_target), first_payload);
    assert_eq!(view.navigation_generation(), 2);

    let mut conflicts = Vec::new();
    let mut different_url = args.clone();
    different_url["url"] = serde_json::json!("https://fixture.invalid/different");
    conflicts.push(different_url);
    let mut different_kind = args.clone();
    different_kind["kind"] = serde_json::json!("reload");
    different_kind.as_object_mut().unwrap().remove("url");
    conflicts.push(different_kind);
    let mut different_target = args.clone();
    different_target["targetId"] = serde_json::json!("wv|forged-retired-document");
    conflicts.push(different_target);
    let mut different_generation = args;
    different_generation["targetGeneration"] = serde_json::json!(generation + 1);
    conflicts.push(different_generation);
    for conflict in conflicts {
        assert!(world
            .broker
            .preflight_tool_permission(&grant.token, "computer_navigate", &conflict)
            .is_err());
        let rejected = grok_computer_use_core::tools::dispatch_with_transport(
            &world.broker,
            &grant.token,
            "computer_navigate",
            conflict,
            None,
        );
        assert_eq!(rejected["isError"], true, "{rejected}");
        assert_eq!(view.navigation_generation(), 2);
    }
}

#[test]
fn dispatched_unknown_navigation_replays_unknown_after_native_retirement() {
    let world = NavigationBrokerWorld::new(&["unknown"]);
    let grant = world.grant("unknown-owner", &["unknown"]);
    let view = world.view("unknown");
    let (origin, generation) = world.broker.authorized_target(&grant.run_id).unwrap();
    view.navigation_unknown.store(true, Ordering::SeqCst);
    let args = navigation_tool_args(
        &grant.run_id,
        Some(&origin),
        generation,
        "unknown-once",
        "url",
        Some("https://fixture.invalid/unknown-result"),
    );
    let first = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        args.clone(),
        None,
    );
    assert_eq!(first["isError"], true, "{first}");
    assert!(first["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("outcome unknown"));
    assert_eq!(view.navigation_generation(), 2);
    assert!(!world.adapter.target_alive(&origin));

    // Permission admission recognizes an exact terminal replay without asking
    // the retired native document to prove liveness again.
    world
        .broker
        .preflight_tool_permission(&grant.token, "computer_navigate", &args)
        .unwrap();
    let replay = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        args,
        None,
    );
    assert_eq!(replay, first);
    assert_eq!(view.navigation_generation(), 2);
}

#[test]
fn navigation_replay_is_scoped_to_its_owner_task_and_run() {
    let world = NavigationBrokerWorld::new(&["a", "b"]);
    let first_grant = world.grant("owner-a", &["a"]);
    let first_view = world.view("a");
    let (first_origin, first_generation) =
        world.broker.authorized_target(&first_grant.run_id).unwrap();
    let first_args = navigation_tool_args(
        &first_grant.run_id,
        Some(&first_origin),
        first_generation,
        "shared-action-id",
        "url",
        Some("https://fixture.invalid/owner-a"),
    );
    let first = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &first_grant.token,
        "computer_navigate",
        first_args.clone(),
        None,
    );
    assert_eq!(first["isError"], false, "{first}");
    assert_eq!(first_view.navigation_generation(), 2);

    let second_grant = world.grant("owner-b", &["b"]);
    let second_view = world.view("b");
    let foreign_owner = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &second_grant.token,
        "computer_navigate",
        first_args,
        None,
    );
    assert_eq!(foreign_owner["isError"], true, "{foreign_owner}");
    assert_eq!(first_view.navigation_generation(), 2);
    assert_eq!(second_view.navigation_generation(), 1);
    let (second_origin, second_generation) = world
        .broker
        .authorized_target(&second_grant.run_id)
        .unwrap();
    let foreign_predecessor = navigation_tool_args(
        &second_grant.run_id,
        Some(&first_origin),
        first_generation,
        "shared-action-id",
        "url",
        Some("https://fixture.invalid/owner-a"),
    );
    let denied = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &second_grant.token,
        "computer_navigate",
        foreign_predecessor,
        None,
    );
    assert_eq!(denied["isError"], true, "{denied}");
    assert_eq!(second_view.navigation_generation(), 1);

    let second_args = navigation_tool_args(
        &second_grant.run_id,
        Some(&second_origin),
        second_generation,
        "shared-action-id",
        "url",
        Some("https://fixture.invalid/owner-b"),
    );
    let second = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &second_grant.token,
        "computer_navigate",
        second_args,
        None,
    );
    assert_eq!(second["isError"], false, "{second}");
    assert_ne!(
        tool_payload(&second)["targetId"],
        tool_payload(&first)["targetId"]
    );
    assert_eq!(second_view.navigation_generation(), 2);
}

#[test]
fn reload_replaces_only_the_active_member_of_a_multi_webview_grant() {
    let world = NavigationBrokerWorld::new(&["a", "b"]);
    let grant = world.grant("multi-owner", &["a", "b"]);
    let before = world
        .broker
        .approved_task_targets(&grant.token, &serde_json::json!({"runId": grant.run_id}))
        .unwrap();
    assert_eq!(before.len(), 2);
    let (active, generation) = world.broker.authorized_target(&grant.run_id).unwrap();
    let untouched = before
        .iter()
        .find_map(|row| {
            let target = row["targetId"].as_str()?;
            (target != active).then(|| target.to_string())
        })
        .unwrap();
    let active_view = if active.contains("native-a") {
        world.view("a")
    } else {
        world.view("b")
    };
    let initial_url = active_view.url().unwrap();
    let args = navigation_tool_args(
        &grant.run_id,
        Some(&active),
        generation,
        "reload-active-member",
        "reload",
        None,
    );
    let result = grok_computer_use_core::tools::dispatch_with_transport(
        &world.broker,
        &grant.token,
        "computer_navigate",
        args,
        None,
    );
    assert_eq!(result["isError"], false, "{result}");
    let payload = tool_payload(&result);
    assert_eq!(payload["url"], initial_url);
    assert_ne!(payload["targetId"], active);
    assert_eq!(active_view.navigation_generation(), 2);

    let after = world
        .broker
        .approved_task_targets(&grant.token, &serde_json::json!({"runId": grant.run_id}))
        .unwrap();
    assert_eq!(after.len(), 2);
    let after_ids = after
        .iter()
        .map(|row| row["targetId"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert!(after_ids.contains(untouched.as_str()));
    assert!(after_ids.contains(payload["targetId"].as_str().unwrap()));
    assert!(!after_ids.contains(active.as_str()));
}

#[test]
fn staged_navigation_is_private_until_the_exact_successor_commit() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let previous = bind(&adapter, view);
    let transition = adapter
        .stage_navigation(&navigation_request(
            &previous,
            "navigate-once",
            NavigationKind::Url,
            Some("https://fixture.invalid/next"),
        ))
        .unwrap();
    let result = transition.result().clone();

    assert!(!adapter.target_alive(&previous.target_id));
    assert!(!adapter.target_alive(&result.target.target_id));
    assert!(adapter.list_targets().unwrap().is_empty());

    let mut commits = 0;
    transition
        .commit(&mut || {
            commits += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(commits, 1);
    assert!(!adapter.target_alive(&previous.target_id));
    assert!(adapter.target_alive(&result.target.target_id));
    assert_eq!(result.target.lifecycle_stamp, 2);
    assert_eq!(adapter.list_targets().unwrap().len(), 1);
    assert!(adapter.observe(&previous.target_id).is_err());
    assert_eq!(
        adapter.observe(&result.target.target_id).unwrap().target_id,
        result.target.target_id
    );
}

#[test]
fn dropped_or_failed_navigation_commit_never_publishes_a_successor() {
    for fail_commit in [false, true] {
        let adapter = WebViewAdapter::new();
        let view = TestView::new(0);
        let previous = bind(&adapter, view);
        let transition = adapter
            .stage_navigation(&navigation_request(
                &previous,
                "navigate-abort",
                NavigationKind::Url,
                Some("https://fixture.invalid/aborted"),
            ))
            .unwrap();
        let successor = transition.result().target.target_id.clone();
        if fail_commit {
            assert_eq!(
                transition.commit(&mut || Err(crate::computer_use::BrokerError::StopRequested)),
                Err(crate::computer_use::BrokerError::StopRequested)
            );
        } else {
            drop(transition);
        }
        assert!(!adapter.target_alive(&previous.target_id));
        assert!(!adapter.target_alive(&successor));
        assert!(adapter.list_targets().unwrap().is_empty());
    }
}

#[test]
fn reload_and_history_each_create_a_fresh_document_identity() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let initial_url = view.url().unwrap();
    let previous = bind(&adapter, view.clone());
    let reload = adapter
        .stage_navigation(&navigation_request(
            &previous,
            "reload-once",
            NavigationKind::Reload,
            None,
        ))
        .unwrap();
    let reloaded = reload.result().clone();
    reload.commit(&mut || Ok(())).unwrap();
    assert_eq!(reloaded.url, initial_url);
    assert_ne!(reloaded.target.target_id, previous.target_id);
    assert_eq!(reloaded.target.lifecycle_stamp, 2);

    view.history_available.store(true, Ordering::SeqCst);
    let back = adapter
        .stage_navigation(&navigation_request(
            &reloaded.target,
            "back-once",
            NavigationKind::Back,
            None,
        ))
        .unwrap();
    let backed = back.result().clone();
    back.commit(&mut || Ok(())).unwrap();
    assert_ne!(backed.target.target_id, reloaded.target.target_id);
    assert_eq!(backed.target.lifecycle_stamp, 3);
    assert!(backed.url.ends_with("/back"));

    let forward = adapter
        .stage_navigation(&navigation_request(
            &backed.target,
            "forward-once",
            NavigationKind::Forward,
            None,
        ))
        .unwrap();
    let forwarded = forward.result().clone();
    forward.commit(&mut || Ok(())).unwrap();
    assert_ne!(forwarded.target.target_id, backed.target.target_id);
    assert_eq!(forwarded.target.lifecycle_stamp, 4);
    assert!(forwarded.url.ends_with("/forward"));
    assert!(adapter.target_alive(&forwarded.target.target_id));
}

#[test]
fn history_without_an_entry_and_blocked_schemes_never_dispatch() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let previous = bind(&adapter, view.clone());
    let back = adapter
        .stage_navigation(&navigation_request(
            &previous,
            "back-empty",
            NavigationKind::Back,
            None,
        ))
        .err()
        .expect("history without an entry must reject");
    assert_eq!(back.disposition, NavigationDisposition::NotDispatched);
    assert_eq!(view.navigation_generation(), 1);
    assert!(adapter.target_alive(&previous.target_id));

    let blocked = adapter
        .stage_navigation(&navigation_request(
            &previous,
            "blocked-url",
            NavigationKind::Url,
            Some("javascript:document.body.remove()"),
        ))
        .err()
        .expect("blocked scheme must reject");
    assert_eq!(blocked.disposition, NavigationDisposition::NotDispatched);
    assert_eq!(view.navigation_generation(), 1);
    assert!(adapter.target_alive(&previous.target_id));
}

#[test]
fn dispatched_unknown_or_a_second_navigation_retires_both_staged_identities() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let previous = bind(&adapter, view.clone());
    view.navigation_unknown.store(true, Ordering::SeqCst);
    let unknown = adapter
        .stage_navigation(&navigation_request(
            &previous,
            "unknown-navigation",
            NavigationKind::Url,
            Some("https://fixture.invalid/unknown"),
        ))
        .err()
        .expect("unknown dispatch must not stage a successor");
    assert_eq!(
        unknown.disposition,
        NavigationDisposition::DispatchedUnknown
    );
    assert_eq!(view.navigation_generation(), 2);
    assert!(!adapter.target_alive(&previous.target_id));
    assert!(adapter.list_targets().unwrap().is_empty());

    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let previous = bind(&adapter, view.clone());
    let transition = adapter
        .stage_navigation(&navigation_request(
            &previous,
            "stale-successor",
            NavigationKind::Url,
            Some("https://fixture.invalid/staged"),
        ))
        .unwrap();
    let successor = transition.result().target.target_id.clone();
    view.navigate("https://fixture.invalid/external").unwrap();
    assert_eq!(
        transition.commit(&mut || Ok(())),
        Err(crate::computer_use::BrokerError::DeadTarget)
    );
    assert!(!adapter.target_alive(&previous.target_id));
    assert!(!adapter.target_alive(&successor));
    assert!(adapter.list_targets().unwrap().is_empty());
}
