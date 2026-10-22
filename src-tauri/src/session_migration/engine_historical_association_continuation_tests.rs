//! G2: real continuation creation across accepted publication and public Undo.
//! Tokens stay in memory/persisted product files; never serialize or print them.
use super::*;
use crate::session_migration::{continuation, mcp_stdio};
use continuation::{DispatchState, HandoffDraft};

// Deliberately no Debug: raw sidecar/ref bytes contain a grant token. Assertions
// on those bytes must report only equality, never dump the protected payload.
struct PinnedContinuation {
    session_id: String,
    grant: mcp_stdio::ImportHistoryGrant,
    sidecar_bytes: Vec<u8>,
    sidecar_hash: String,
    ref_bytes: Vec<u8>,
    handoff_bytes: Vec<u8>,
    handoff_text_hash: String,
    session_bytes: Vec<u8>,
    draft_hash: String,
    request_id: String,
}

fn ref_snapshot(conn: &Connection, sid: &str) -> (Vec<String>, Vec<u8>) {
    conn.query_row(
        "SELECT import_id,revision_id,branch_leaf,allowed_selection_hash,handoff_hash,request_id,CAST(json AS BLOB) FROM continuation_refs WHERE app_session_id=?1",
        [sid],
        |row| Ok(((0..6).map(|i| row.get(i)).collect::<rusqlite::Result<_>>()?, row.get(6)?)),
    )
    .unwrap()
}

fn session_snapshot(sid: &str) -> Vec<u8> {
    let sessions = crate::store::load_sessions_snapshot().unwrap();
    let session = sessions.iter().find(|session| session.id == sid).unwrap();
    serde_json::to_vec(session).unwrap()
}

impl PinnedContinuation {
    fn create(draft: &HandoffDraft, request: &str) -> Self {
        let sid =
            engine::continue_create(&draft.draft_id, &draft.draft_hash, request, None).unwrap();
        let grant = engine::grant_for_session(&sid).expect("real created session grant");
        assert_eq!(grant.app_session_id, sid);
        assert_eq!(grant.import_id, IMPORT);
        assert_eq!(grant.revision_id, draft.revision_id);
        assert_eq!(grant.branch_leaf, draft.leaf);
        assert_eq!(grant.allowed_selection_hash, draft.selection.hash());
        assert_eq!(grant.allowed_event_ids, draft.readable_event_ids);
        assert_eq!(
            grant.granted_selection().unwrap().hash(),
            draft.selection.hash()
        );
        let side_path = continuation::sidecar_path(&sid);
        let sidecar_bytes = std::fs::read(&side_path).unwrap();
        let handoff_bytes = std::fs::read(side_path.with_file_name("handoff_draft.txt")).unwrap();
        assert_eq!(handoff_bytes, draft.text.as_bytes());
        let (_, ref_bytes) = ref_snapshot(&open_catalog().unwrap(), &sid);
        Self {
            sidecar_hash: hash::sha256_bytes(&sidecar_bytes),
            handoff_text_hash: hash::sha256_bytes(&handoff_bytes),
            session_bytes: session_snapshot(&sid),
            session_id: sid,
            grant,
            sidecar_bytes,
            ref_bytes,
            handoff_bytes,
            draft_hash: draft.draft_hash.clone(),
            request_id: request.into(),
        }
    }

    fn check(&self, selected: &str, unselected: &str, visible: &str, omitted: &str) {
        let conn = open_catalog().unwrap();
        let (columns, bytes) = ref_snapshot(&conn, &self.session_id);
        assert_eq!(
            columns,
            vec![
                IMPORT.to_string(),
                self.grant.revision_id.clone(),
                self.grant.branch_leaf.clone(),
                self.grant.allowed_selection_hash.clone(),
                self.draft_hash.clone(),
                self.request_id.clone(),
            ]
        );
        assert!(bytes == self.ref_bytes, "pinned ref bytes changed");
        let side_path = continuation::sidecar_path(&self.session_id);
        let side_bytes = std::fs::read(&side_path).unwrap();
        assert!(
            side_bytes == self.sidecar_bytes,
            "pinned sidecar bytes changed"
        );
        assert_eq!(hash::sha256_bytes(&side_bytes), self.sidecar_hash);
        let side = continuation::read_sidecar_strict(&self.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(side.import_id, IMPORT);
        assert_eq!(side.revision_id, self.grant.revision_id);
        assert_eq!(side.branch_leaf, self.grant.branch_leaf);
        assert_eq!(
            side.allowed_selection_hash,
            self.grant.allowed_selection_hash
        );
        assert_eq!(side.handoff_hash, self.draft_hash);
        assert_eq!(side.request_id, self.request_id);
        assert_eq!(side.state, DispatchState::Prepared);
        assert!(side.dispatch_id.is_none() && side.agent_session_id.is_none());
        let handoff = std::fs::read(side_path.with_file_name("handoff_draft.txt")).unwrap();
        assert!(
            handoff == self.handoff_bytes,
            "pinned handoff bytes changed"
        );
        assert_eq!(hash::sha256_bytes(&handoff), self.handoff_text_hash);
        assert_eq!(
            std::fs::read(side_path.with_file_name("messages.json")).unwrap(),
            b"[]"
        );
        assert!(
            session_snapshot(&self.session_id) == self.session_bytes,
            "native session changed"
        );
        let restored = engine::grant_for_session(&self.session_id).expect("pinned grant restored");
        assert!(restored.token == self.grant.token, "grant token changed");
        assert_eq!(restored.app_session_id, self.session_id);
        assert_eq!(restored.import_id, self.grant.import_id);
        assert_eq!(restored.revision_id, self.grant.revision_id);
        assert_eq!(restored.branch_leaf, self.grant.branch_leaf);
        assert_eq!(restored.allowed_event_ids, self.grant.allowed_event_ids);
        assert_eq!(
            restored.allowed_selection_hash,
            self.grant.allowed_selection_hash
        );
        assert_eq!(
            restored.granted_selection().unwrap().hash(),
            self.grant.allowed_selection_hash
        );
        // Test both the original in-memory grant and the freshly resolved one.
        for grant in [&self.grant, &restored] {
            let read =
                mcp_stdio::call_tool(grant, "import_history_read", &json!({"citation": selected}))
                    .unwrap();
            let body: serde_json::Value =
                serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(
                body["revision"].as_str(),
                Some(self.grant.revision_id.as_str())
            );
            assert_eq!(body["citation"].as_str(), Some(selected));
            assert!(body["text"].as_str().unwrap().contains(visible));
            assert!(!body["text"].as_str().unwrap().contains(omitted));
            let denied = mcp_stdio::call_tool(
                grant,
                "import_history_read",
                &json!({"citation": unselected}),
            )
            .unwrap_err();
            assert!(denied.contains("HISTORY_ACCESS_UNAVAILABLE"), "{denied}");
            let search =
                mcp_stdio::call_tool(grant, "import_history_search", &json!({"query": omitted}))
                    .unwrap();
            let body: serde_json::Value =
                serde_json::from_str(search["content"][0]["text"].as_str().unwrap()).unwrap();
            assert!(body["hits"].as_array().unwrap().is_empty());
        }
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_continuation_refs_and_grants_survive_public_undo() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (events, cursor, count) =
        engine::events_page(IMPORT, Some(REVISION), None, Some(20), None).unwrap();
    assert!(cursor.is_none());
    assert_eq!(count, 2);
    assert_eq!(events.len(), 2);
    let old_selected = events[0]["id"].as_str().unwrap();
    let old_unselected = events[1]["id"].as_str().unwrap();
    let old_draft = engine::continue_prepare(IMPORT, None, Some(old_selected), None, None).unwrap();
    assert_eq!(old_draft.revision_id, REVISION);
    assert_eq!(old_draft.readable_event_ids, vec![old_selected]);
    let old = PinnedContinuation::create(&old_draft, "synthetic-association-G2-old");
    old.check(old_selected, old_unselected, "hello one", "hi two");
    let old_hashes = immutable_hashes(REVISION);

    let (_, completed) = publish("synthetic-association-G2-publish");
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(revision)
    );
    old.check(old_selected, old_unselected, "hello one", "hi two");
    let (new_events, cursor, count) =
        engine::events_page(IMPORT, Some(revision), None, Some(20), None).unwrap();
    assert!(cursor.is_none());
    assert_eq!(count, 2);
    assert_eq!(new_events.len(), 2);
    let new_selected = new_events[1]["id"].as_str().unwrap();
    let new_unselected = new_events[0]["id"].as_str().unwrap();
    // Real narrower scope on the new archive, distinct from the old prefix.
    let new_draft = engine::continue_prepare_with_selection(
        IMPORT,
        None,
        Some(new_selected),
        None,
        None,
        Some(continuation::AllowedSelection {
            event_ids: vec![new_selected.into()],
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(new_draft.revision_id, revision);
    assert_eq!(new_draft.readable_event_ids, vec![new_selected]);
    let new = PinnedContinuation::create(&new_draft, "synthetic-association-G2-new");
    assert_ne!(old.session_id, new.session_id);
    assert_ne!(old.grant.revision_id, new.grant.revision_id);
    assert_ne!(
        old.grant.allowed_selection_hash,
        new.grant.allowed_selection_hash
    );
    assert_ne!(old.draft_hash, new.draft_hash);
    assert_ne!(old.handoff_text_hash, new.handoff_text_hash);
    let new_hashes = immutable_hashes(revision);
    let all_receipts = receipts(&conn);
    let proof = proof_bytes(&conn);
    let check_both = || {
        old.check(old_selected, old_unselected, "hello one", "hi two");
        new.check(new_selected, new_unselected, "hi two", "hello one");
        assert_eq!(crate::store::load_sessions_snapshot().unwrap().len(), 2);
        assert_eq!(
            continuation::list_refs_for_import(&conn, IMPORT)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(immutable_hashes(REVISION), old_hashes);
        assert_eq!(immutable_hashes(revision), new_hashes);
        assert_eq!(receipts(&conn), all_receipts);
        assert_eq!(proof_bytes(&conn), proof);
    };
    check_both();
    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(REVISION)
    );
    check_both();
    reset_runtime();
    check_both();
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!("ASSOCIATION_CONTINUATION_PINNED={{\"sessions\":2,\"refs\":2,\"oldAndNewPinned\":true,\"scopeDenials\":true,\"publicUndo\":true,\"runtimeReset\":true}}");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_continuation_uncreated_old_draft_refuses_after_publication() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let draft = engine::continue_prepare(IMPORT, None, None, None, None).unwrap();
    assert_eq!(draft.revision_id, REVISION);
    let (_, completed) = publish("synthetic-association-G2-stale-draft-publish");
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(revision)
    );
    let after_publish = raw_catalog_rows(&conn);
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    let error = engine::continue_create(
        &draft.draft_id,
        &draft.draft_hash,
        "synthetic-association-G2-stale-create",
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(
        raw_catalog_rows(&conn) == after_publish,
        "stale create changed catalog rows"
    );
    assert!(crate::store::load_sessions_snapshot().unwrap().is_empty());
    assert!(continuation::list_refs_for_import(&conn, IMPORT)
        .unwrap()
        .is_empty());
    assert!(!crate::session_migration::continuation_intent::exists(&conn).unwrap());
    // Advisory lock-only directories are not published sessions. Refusal must
    // never create a sidecar, handoff or native message file.
    for entry in std::fs::read_dir(home.path.join("sessions")).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            for name in [
                "import_continuation.json",
                "handoff_draft.txt",
                "messages.json",
            ] {
                assert!(!path.join(name).exists());
            }
        }
    }
    assert_eq!(immutable_hashes(REVISION), old_hashes);
    assert_eq!(immutable_hashes(revision), new_hashes);
    reset_runtime();
    assert!(
        raw_catalog_rows(&conn) == after_publish,
        "reset changed stale-create facts"
    );
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!("ASSOCIATION_CONTINUATION_STALE_DRAFT={{\"code\":\"HISTORY_ACCESS_UNAVAILABLE\",\"sessions\":0,\"refs\":0,\"intents\":0,\"catalogUnchanged\":true}}");
}
