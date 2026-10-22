//! Bounded startup discovery of committed cleanup intents. The marker is an
//! index only; exact action MAC/original inputs/receipts are checked by resume.
use super::*;

pub(in crate::session_migration) fn recover(conn: &Connection) -> Result<(), ImportError> {
    let mut after = i64::MIN;
    let mut refused = None;
    loop {
        let rows = conn.prepare(
            "SELECT rowid,key,CASE WHEN typeof(value)='text' AND length(CAST(value AS BLOB))<=?3
                           THEN value END FROM meta WHERE key GLOB ?1
                AND length(CAST(key AS BLOB))=?2 AND rowid>?4 ORDER BY rowid LIMIT 64",
        ).map_err(sqlite_storage_error)?
            .query_map(params![format!("{PENDING_PREFIX}*"), PENDING_PREFIX.len()+36, RECORD_BYTES, after], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1), row.get::<_, String>(2)))
            }).map_err(sqlite_storage_error)?
            .collect::<Result<Vec<_>, _>>().map_err(sqlite_storage_error)?;
        if rows.is_empty() {
            break;
        }
        for (row, key, encoded) in rows {
            after = row;
            let result = (|| {
                let key = key.map_err(|_| unavailable())?;
                let encoded = encoded.map_err(|_| unavailable())?;
                if encoded.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) {
                    return Err(unavailable());
                }
                let envelope: Envelope =
                    serde_json::from_str(&encoded).map_err(|_| unavailable())?;
                let body = envelope.body;
                if body.version != 1
                    || body.action != Action::Undo
                    || body.cleanup.is_none()
                    || key != format!("{PENDING_PREFIX}{}", body.request_id)
                {
                    return Err(unavailable());
                }
                let action = PreparedAction::prepare(
                    conn,
                    &body.job_id,
                    Action::Undo,
                    JobActionRequest {
                        request_id: body.request_id,
                        expected_state_version: body.expected_state_version,
                    },
                )?;
                // resume requires its parent record and matching pending index
                // under the real publish locks. There is no fresh-Undo fallback.
                jobs::resume_undo_action(conn, &action).map(|_| ())
            })();
            if let Err(error) = result {
                refused.get_or_insert(error);
            }
        }
    }
    refused.map_or(Ok(()), Err)
}
