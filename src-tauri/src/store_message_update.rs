//! Decide a single journal-row update against the current locked store.
use super::{read_json_recover, write_messages_locked, ChatMessageStored};
use crate::paths::session_dir;
use chrono::{Duration, Utc};

/// Read, decide and write under the same process + sidecar lock as append/save.
/// The callback must only prepare a row; it must not acquire store locks or do
/// other store I/O. Returning None leaves the journal and row revision unchanged.
pub(crate) fn update_message(
    session: &str,
    id: &str,
    update: impl FnOnce(Option<&ChatMessageStored>) -> Option<ChatMessageStored>,
) -> Result<bool, String> {
    let path = session_dir(session).join("messages.json");
    crate::store_lock::with_exclusive_lock(&path, || {
        let mut rows: Vec<ChatMessageStored> = read_json_recover(&path);
        let index = rows.iter().position(|row| row.id == id);
        let Some(mut next) = update(index.map(|index| &rows[index])) else {
            return Ok(false);
        };
        if next.id != id {
            return Err("message update changed its target id".into());
        }
        if let Some(index) = index {
            // created_at is already the store's per-row revision for stale
            // snapshot merges. Advance it even if the stored clock is ahead.
            let minimum = rows[index]
                .created_at
                .checked_add_signed(Duration::nanoseconds(1))
                .ok_or("message update revision overflow")?;
            next.created_at = next.created_at.max(Utc::now()).max(minimum);
            rows[index] = next;
        } else {
            rows.push(next);
        }
        write_messages_locked(&path, &rows)?;
        Ok(true)
    })
}

/// Remove a pre-dispatch user row only when it is still the exact journal tail.
/// This read is deliberately strict: corruption is neither quarantined nor
/// interpreted as an empty journal during a rollback decision.
pub(crate) fn rollback_tail_message(
    session: &str,
    expected: &ChatMessageStored,
) -> Result<(), String> {
    let path = session_dir(session).join("messages.json");
    crate::store_lock::with_exclusive_lock(&path, || {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read journal for exact rollback: {error}"))?;
        let mut rows: Vec<ChatMessageStored> = serde_json::from_slice(&bytes)
            .map_err(|error| format!("cannot parse journal for exact rollback: {error}"))?;
        match rows.last() {
            Some(tail) if tail == expected => {}
            Some(_) => return Err("journal tail no longer matches the opened turn".into()),
            None => return Err("journal is empty before opened-turn rollback".into()),
        }
        rows.pop();
        write_messages_locked(&path, &rows)
    })
}

#[cfg(test)]
mod rollback_tests {
    use super::*;
    use crate::session_migration::test_home::IsolatedAppHome;

    fn row(id: &str, content: &str) -> ChatMessageStored {
        ChatMessageStored {
            id: id.into(),
            role: "user".into(),
            content: content.into(),
            thought: None,
            created_at: Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        }
    }

    #[test]
    fn rollback_removes_only_the_exact_appended_tail() {
        let _home = IsolatedAppHome::acquire("message-tail-rollback");
        let session = crate::store::create_session(None, None, false).unwrap();
        let base = row("base", "keep");
        let target = row("target", "unsent");
        crate::store::append_message(&session.id, base.clone()).unwrap();
        crate::store::append_message(&session.id, target.clone()).unwrap();
        rollback_tail_message(&session.id, &target).unwrap();
        assert_eq!(crate::store::load_messages(&session.id), vec![base]);
    }

    #[test]
    fn rollback_fails_closed_when_another_row_follows_the_target() {
        let _home = IsolatedAppHome::acquire("message-tail-conflict");
        let session = crate::store::create_session(None, None, false).unwrap();
        let target = row("target", "unsent");
        let later = row("later", "must survive");
        crate::store::append_message(&session.id, target.clone()).unwrap();
        crate::store::append_message(&session.id, later.clone()).unwrap();
        assert!(rollback_tail_message(&session.id, &target).is_err());
        assert_eq!(
            crate::store::load_messages(&session.id),
            vec![target, later]
        );
    }

    #[test]
    fn rollback_does_not_quarantine_or_replace_a_corrupt_journal() {
        let _home = IsolatedAppHome::acquire("message-tail-corrupt");
        let session = crate::store::create_session(None, None, false).unwrap();
        let path = session_dir(&session.id).join("messages.json");
        let corrupt = b"{synthetic-corruption";
        std::fs::write(&path, corrupt).unwrap();
        assert!(rollback_tail_message(&session.id, &row("target", "unsent")).is_err());
        assert_eq!(std::fs::read(path).unwrap(), corrupt);
    }
}
