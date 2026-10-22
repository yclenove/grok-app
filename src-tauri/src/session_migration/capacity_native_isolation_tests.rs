//! Check the same native-store/usage invariants at 8, 64 and 10,000 imports.

use crate::{account, paths, store};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct NativeIsolation {
    session_id: String,
    index: Vec<u8>,
    messages: Value,
    roots: Vec<PathBuf>,
    logs: Value,
}

impl NativeIsolation {
    pub(super) fn seed(home_root: &Path) -> Self {
        let session = store::create_session(None, Some("Native history".into()), false).unwrap();
        store::append_message(
            &session.id,
            store::ChatMessageStored {
                id: "native-answer".into(),
                role: "assistant".into(),
                content: "Existing native history".into(),
                thought: None,
                created_at: chrono::Utc::now(),
                is_error: false,
                attachments: None,
                marker: None,
            },
        )
        .unwrap();
        let imports = crate::session_migration::catalog::imports_dir();
        let roots = account::session_roots();
        assert_eq!(
            roots.len(),
            3,
            "isolate all three distinct native usage roots"
        );
        assert!(roots.iter().all(|root| root.starts_with(home_root)));
        assert!(roots
            .iter()
            .all(|root| !root.starts_with(&imports) && !imports.starts_with(root)));
        let usage_dir = paths::agent_home_dir()
            .join("sessions")
            .join("native-project")
            .join(&session.id);
        std::fs::create_dir_all(&usage_dir).unwrap();
        store::write_json(
            &usage_dir.join("signals.json"),
            &serde_json::json!({"turnCount": 1, "contextTokensUsed": 77}),
        )
        .unwrap();
        std::fs::write(
            usage_dir.join("updates.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({"params": {"update": {"sessionUpdate": "turn_completed",
                    "usage": {"inputTokens": 100, "outputTokens": 25, "totalTokens": 125}}}})
            ),
        )
        .unwrap();
        let (heatmap, logs) = account::local_usage(7, 100);
        assert_eq!(heatmap.iter().map(|day| day.tokens).sum::<u64>(), 125);
        assert_eq!(heatmap.iter().map(|day| day.requests).sum::<u64>(), 1);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, session.id);
        assert_eq!(logs[0].usage_tokens, 125);
        let rows = store::load_sessions_index();
        assert_eq!(rows.len(), 1, "enqueue must not create native sessions");
        assert_eq!(rows[0].id, session.id);
        Self {
            index: std::fs::read(paths::sessions_index_file()).unwrap(),
            messages: serde_json::to_value(store::load_messages(&session.id)).unwrap(),
            session_id: session.id,
            roots,
            logs: serde_json::to_value(logs).unwrap(),
        }
    }

    pub(super) fn assert_unchanged(&self) {
        assert_eq!(
            std::fs::read(paths::sessions_index_file()).unwrap(),
            self.index,
            "publication must preserve native metadata and tray session identities"
        );
        assert_eq!(
            serde_json::to_value(store::load_messages(&self.session_id)).unwrap(),
            self.messages,
            "publication must preserve the native journal"
        );
        assert_eq!(account::session_roots(), self.roots);
        let (heatmap, logs) = account::local_usage(7, 100);
        // A long 10k run can cross UTC midnight; compare totals, not date labels.
        assert_eq!(heatmap.iter().map(|day| day.tokens).sum::<u64>(), 125);
        assert_eq!(heatmap.iter().map(|day| day.requests).sum::<u64>(), 1);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].usage_tokens, 125);
        assert_eq!(
            serde_json::to_value(logs).unwrap(),
            self.logs,
            "source tokens must not enter native call-log usage"
        );
    }
}

#[test]
fn native_usage_isolation_detects_writes_in_every_production_root() {
    let home = super::super::CapacityHome::new();
    let native = NativeIsolation::seed(&home.root);
    for root in &native.roots {
        let leaked = root.join("imported-project").join("leaked-source-tokens");
        std::fs::create_dir_all(&leaked).unwrap();
        store::write_json(
            &leaked.join("signals.json"),
            &serde_json::json!({"turnCount": 1, "contextTokensUsed": 1_000_000}),
        )
        .unwrap();
        assert!(
            std::panic::catch_unwind(|| native.assert_unchanged()).is_err(),
            "missed imported usage under {}",
            root.display()
        );
        std::fs::remove_dir_all(leaked).unwrap();
        native.assert_unchanged();
    }
}
