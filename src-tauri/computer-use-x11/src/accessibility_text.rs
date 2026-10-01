//! Exact-range AT-SPI editing, never a whole-field SetTextContents fallback.
//! DeleteText + InsertText are not atomic. Every boundary validates the original
//! object and expected text/selection; a partial edit is reported, never retried
//! or rolled back over a user's intervening edit. No clipboard or keymap writes.
use super::*;
use grok_computer_use_core::protocol::OutcomeKind;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextState {
    text: String,
    caret: i32,
    selection: Option<(i32, i32)>,
}

impl TextState {
    fn checked(text: String, caret: i32, selection: Option<(i32, i32)>) -> Result<Self, String> {
        let count = text.chars().count() as i32;
        if text.len() > MAX_TEXT || !(0..=count).contains(&caret) {
            return Err("AT-SPI text/caret exceeds bounds".into());
        }
        let selection = match selection {
            Some((a, b)) => {
                let (start, end) = (a.min(b), a.max(b));
                if start < 0 || end > count || start >= end || (caret != start && caret != end) {
                    return Err("AT-SPI selection is inconsistent with text/caret".into());
                }
                Some((start, end))
            }
            None => None,
        };
        Ok(Self {
            text,
            caret,
            selection,
        })
    }
}

fn read_state(connection: &Connection, object: &Object) -> Result<TextState, String> {
    fn sample(connection: &Connection, object: &Object) -> Result<TextState, String> {
        let value = read_text(connection, object)?;
        let text = proxy(connection, object, TEXT)?;
        let caret: i32 = text.get_property("CaretOffset").map_err(err)?;
        let selections: i32 = text.call("GetNSelections", &()).map_err(err)?;
        let selection = match selections {
            0 => None,
            1 => Some(text.call("GetSelection", &(0i32,)).map_err(err)?),
            _ => {
                return Err(
                    "AT-SPI multiple/invalid selections cannot be replaced as one range".into(),
                )
            }
        };
        TextState::checked(value, caret, selection)
    }
    let first = sample(connection, object)?;
    if sample(connection, object)? != first {
        return Err("AT-SPI text/caret/selection changed while reading".into());
    }
    Ok(first)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Edit<'a> {
    Delete(i32, i32),
    Insert(i32, &'a str),
    Caret(i32),
}

trait Backend {
    fn validate(&mut self) -> Result<(), String>;
    fn read(&mut self) -> Result<TextState, String>;
    fn write(&mut self, edit: Edit<'_>) -> Result<bool, String>;
    fn uncertain(&self) -> bool;
}

struct Native<'a, F> {
    connection: &'a Connection,
    snapshot: &'a Snapshot,
    node: &'a Node,
    req: &'a DispatchRequest,
    calls: &'a mut call::NativeCalls,
    validate_native: F,
}

impl<F: FnMut() -> Result<(), String>> Backend for Native<'_, F> {
    fn validate(&mut self) -> Result<(), String> {
        self.req.cancellation.check()?;
        (self.validate_native)()?;
        validate_node(self.connection, self.snapshot, self.node)?;
        if !state(&states(self.connection, &self.node.object)?, EDITABLE_STATE) {
            return Err("AT-SPI node is no longer editable".into());
        }
        self.req.cancellation.check()?;
        (self.validate_native)()
    }
    fn read(&mut self) -> Result<TextState, String> {
        self.req.cancellation.check()?;
        let result = read_state(self.connection, &self.node.object)?;
        self.req.cancellation.check()?;
        Ok(result)
    }
    fn write(&mut self, edit: Edit<'_>) -> Result<bool, String> {
        let message =
            match edit {
                Edit::Delete(start, end) => {
                    input_message(self.connection, &self.node.object, EDITABLE, "DeleteText")?
                        .build(&(start, end))
                }
                Edit::Insert(start, value) => {
                    input_message(self.connection, &self.node.object, EDITABLE, "InsertText")?
                        .build(&(start, value, value.len() as i32))
                }
                Edit::Caret(offset) => {
                    input_message(self.connection, &self.node.object, TEXT, "SetCaretOffset")?
                        .build(&(offset,))
                }
            }
            .map_err(err)?;
        self.calls.call(self.connection, message, || {
            self.req.cancellation.check()?;
            (self.validate_native)()
        })
    }
    fn uncertain(&self) -> bool {
        self.calls.uncertain
    }
}

fn ensure(backend: &mut impl Backend, expected: &TextState) -> Result<(), String> {
    backend.validate()?;
    if backend.read()? != *expected {
        return Err("AT-SPI text/caret/selection changed; no further input".into());
    }
    backend.validate()
}

fn dispatch(
    backend: &mut impl Backend,
    edit: Edit<'_>,
    before: &TextState,
    applied: &mut bool,
) -> Result<(), String> {
    ensure(backend, before)?;
    if !backend.write(edit)? {
        // A completed negative reply does not permit replay or rollback. If the
        // application changed anyway, retain the visible partial-effect status.
        *applied |= backend.read()? != *before;
        return Err("AT-SPI toolkit declined a text-edit step".into());
    }
    *applied = true;
    Ok(())
}

fn execute(backend: &mut impl Backend, value: &str) -> Result<AdapterActResult, String> {
    if value.len() > MAX_TEXT || value.contains('\0') {
        return Err("text exceeds AT-SPI bounds".into());
    }
    backend.validate()?;
    let original = backend.read()?;
    let (start, end) = original
        .selection
        .unwrap_or((original.caret, original.caret));
    // Unicode offsets on the wire are code points; InsertText length is bytes.
    let deleted = original
        .text
        .chars()
        .enumerate()
        .filter_map(|(i, c)| (!(start as usize..end as usize).contains(&i)).then_some(c))
        .collect::<String>();
    let final_text = inserted(&deleted, start, value)?;
    let final_caret = start + value.chars().count() as i32;
    let mut applied = false;
    let result = (|| {
        ensure(backend, &original)?;
        // An empty TypeText is a no-op, not an implicit delete-selection action.
        if value.is_empty() {
            return Ok(());
        }
        let mut current = original.clone();
        if original.selection.is_some() {
            dispatch(backend, Edit::Delete(start, end), &current, &mut applied)?;
            current = TextState {
                text: deleted,
                caret: start,
                selection: None,
            };
            ensure(backend, &current)?;
        }
        dispatch(backend, Edit::Insert(start, value), &current, &mut applied)?;
        backend.validate()?;
        current = backend.read()?;
        if current.text != final_text || current.selection.is_some() {
            return Err(
                "AT-SPI inserted text/selection did not match; no corrective rewrite".into(),
            );
        }
        if current.caret != final_caret {
            if current.caret != start {
                return Err("AT-SPI caret moved away from the edit; no caret takeover".into());
            }
            dispatch(backend, Edit::Caret(final_caret), &current, &mut applied)?;
        }
        ensure(
            backend,
            &TextState {
                text: final_text,
                caret: final_caret,
                selection: None,
            },
        )
    })();
    match result {
        Ok(()) => Ok(AdapterActResult {
            applied,
            outcome: Some(OutcomeKind::Verified),
            postcondition_ok: true,
            verifiable: true,
            detail: if applied {
                "AT-SPI exact-range text and caret verified; no clipboard/keymap writes"
            } else {
                "AT-SPI empty TypeText: verified no input and preserved selection"
            }
            .into(),
        }),
        Err(error) if backend.uncertain() => Err(format!(
            "AT-SPI text step completion unknown; not replayed: {error}"
        )),
        Err(error) if applied => Ok(AdapterActResult {
            applied: true,
            outcome: Some(OutcomeKind::Applied),
            postcondition_ok: false,
            verifiable: false,
            detail: format!("AT-SPI partial text edit; no retry or rollback: {error}"),
        }),
        Err(error) => Err(error),
    }
}

pub(super) fn run(
    connection: &Connection,
    snapshot: &Snapshot,
    node: &Node,
    req: &DispatchRequest,
    calls: &mut call::NativeCalls,
    validate_native: impl FnMut() -> Result<(), String>,
) -> Result<AdapterActResult, String> {
    // An explicit clipboard request is a different operation, not permission to
    // substitute DeleteText/InsertText. Keep it fail-closed until the native
    // selection transaction and asynchronous paste completion are both wired.
    if req.parameters.get("via").is_some() {
        return Err("X11 clipboard paste is not yet available; no direct-edit fallback".into());
    }
    let value = req
        .parameters
        .get("text")
        .or_else(|| req.parameters.get("value"))
        .and_then(|v| v.as_str())
        .ok_or("text is required")?;
    execute(
        &mut Native {
            connection,
            snapshot,
            node,
            req,
            calls,
            validate_native,
        },
        value,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Model {
        state: TextState,
        writes: Vec<String>,
        fail_after: Option<usize>,
        lost_reply: Option<usize>,
        reject_step: Option<usize>,
        takeover_after: Option<usize>,
        move_caret: bool,
        unknown: bool,
        reads: usize,
        drift_on_read: Option<usize>,
    }
    impl Model {
        fn selected() -> Self {
            Self::new("中文第二波🙂测试", 6, Some((2, 6)))
        }
        fn new(text: &str, caret: i32, selection: Option<(i32, i32)>) -> Self {
            Self {
                state: TextState::checked(text.into(), caret, selection).unwrap(),
                writes: vec![],
                fail_after: None,
                lost_reply: None,
                reject_step: None,
                takeover_after: None,
                move_caret: true,
                unknown: false,
                reads: 0,
                drift_on_read: None,
            }
        }
    }
    impl Backend for Model {
        fn validate(&mut self) -> Result<(), String> {
            if self.fail_after.is_some_and(|n| self.writes.len() >= n) {
                Err("cancelled/revoked/disabled/focus changed".into())
            } else {
                Ok(())
            }
        }
        fn read(&mut self) -> Result<TextState, String> {
            self.reads += 1;
            if self.drift_on_read == Some(self.reads) {
                self.state.caret = 1;
                self.state.selection = None;
            }
            Ok(self.state.clone())
        }
        fn write(&mut self, edit: Edit<'_>) -> Result<bool, String> {
            self.writes.push(format!("{edit:?}"));
            if self.reject_step == Some(self.writes.len()) {
                return Ok(false);
            }
            match edit {
                Edit::Delete(start, end) => {
                    self.state.text = self
                        .state
                        .text
                        .chars()
                        .enumerate()
                        .filter_map(|(i, c)| {
                            (!(start as usize..end as usize).contains(&i)).then_some(c)
                        })
                        .collect();
                    self.state.caret = start;
                    self.state.selection = None;
                }
                Edit::Insert(start, value) => {
                    self.state.text = inserted(&self.state.text, start, value)?;
                    if self.move_caret {
                        self.state.caret = start + value.chars().count() as i32;
                    }
                }
                Edit::Caret(at) => {
                    self.state.caret = at;
                    self.state.selection = None;
                }
            }
            if self.takeover_after == Some(self.writes.len()) {
                self.state.text = "USER EDIT MUST SURVIVE".into();
                self.state.caret = 1;
                self.state.selection = None;
            }
            if self.lost_reply == Some(self.writes.len()) {
                self.unknown = true;
                return Err("lost DBus reply".into());
            }
            Ok(true)
        }
        fn uncertain(&self) -> bool {
            self.unknown
        }
    }
    fn assert_partial(result: AdapterActResult) {
        assert!(result.applied);
        assert_eq!(result.outcome, Some(OutcomeKind::Applied));
        assert!(!result.postcondition_ok);
        assert!(!result.verifiable);
        assert!(result.detail.contains("no retry or rollback"));
        assert!(!result.detail.contains("USER EDIT"));
    }
    #[test]
    fn selection_normalizes_reversed_unicode_offsets_but_rejects_invalid_ranges() {
        assert_eq!(
            TextState::checked("中🙂a".into(), 1, Some((3, 1)))
                .unwrap()
                .selection,
            Some((1, 3))
        );
        for (caret, range) in [
            (0, Some((-1, 1))),
            (4, None),
            (1, Some((1, 1))),
            (1, Some((0, 2))),
            (0, Some((0, 4))),
        ] {
            assert!(TextState::checked("中🙂a".into(), caret, range).is_err());
        }
    }
    #[test]
    fn replace_selected_range_not_whole_field_and_preserve_surrounding_unicode() {
        let mut m = Model::selected();
        let r = execute(&mut m, "替换🦀").unwrap();
        assert!(r.postcondition_ok);
        assert_eq!(
            m.state,
            TextState::checked("中文替换🦀测试".into(), 5, None).unwrap()
        );
        assert_eq!(m.writes, vec!["Delete(2, 6)", "Insert(2, \"替换🦀\")"]);
    }
    #[test]
    fn reverse_selection_replaces_same_span() {
        let mut m = Model::new("中🙂文", 1, Some((2, 1)));
        assert!(execute(&mut m, "XY").unwrap().postcondition_ok);
        assert_eq!(m.state.text, "中XY文");
        assert_eq!(m.state.caret, 3);
    }
    #[test]
    fn whole_selection_and_unselected_unicode_caret_work() {
        for (caret, selection, expected) in [(3, Some((0, 3)), "🙂"), (1, None, "中🙂🙂文")]
        {
            let mut m = Model::new("中🙂文", caret, selection);
            assert!(execute(&mut m, "🙂").unwrap().postcondition_ok);
            assert_eq!(m.state.text, expected);
        }
    }
    #[test]
    fn empty_typing_preserves_selection_and_sends_no_write() {
        let mut m = Model::selected();
        let before = m.state.clone();
        let r = execute(&mut m, "").unwrap();
        assert!(r.postcondition_ok);
        assert!(!r.applied);
        assert_eq!(m.state, before);
        assert!(m.writes.is_empty());
    }
    #[test]
    fn oversized_result_or_nul_rejected_before_any_mutation() {
        for value in ["\0".into(), "x".repeat(MAX_TEXT + 1)] {
            let mut m = Model::selected();
            assert!(execute(&mut m, &value).is_err());
            assert!(m.writes.is_empty());
        }
        let mut m = Model::new(&"x".repeat(MAX_TEXT), 1, None);
        assert!(execute(&mut m, "中").is_err());
        assert!(m.writes.is_empty());
        let mut m = Model::new(
            &"x".repeat(MAX_TEXT),
            MAX_TEXT as i32,
            Some((0, MAX_TEXT as i32)),
        );
        assert!(execute(&mut m, "中").unwrap().postcondition_ok);
    }
    #[test]
    fn cancellation_before_dispatch_is_zero_effect() {
        let mut m = Model::selected();
        m.fail_after = Some(0);
        assert!(execute(&mut m, "x").is_err());
        assert!(m.writes.is_empty());
    }
    #[test]
    fn cancellation_after_delete_reports_partial_and_never_inserts_or_rolls_back() {
        let mut m = Model::selected();
        m.fail_after = Some(1);
        assert_partial(execute(&mut m, "x").unwrap());
        assert_eq!(m.state.text, "中文测试");
        assert_eq!(m.writes, vec!["Delete(2, 6)"]);
    }
    #[test]
    fn cancellation_after_insert_does_not_claim_verified_or_move_caret() {
        let mut m = Model::selected();
        m.fail_after = Some(2);
        m.move_caret = false;
        assert_partial(execute(&mut m, "x").unwrap());
        assert_eq!(m.writes.len(), 2);
        assert_eq!(m.state.caret, 2);
    }
    #[test]
    fn intervening_user_edit_survives_without_retry_or_rollback() {
        for boundary in [1, 2] {
            let mut m = Model::selected();
            m.takeover_after = Some(boundary);
            assert_partial(execute(&mut m, "x").unwrap());
            assert_eq!(m.state.text, "USER EDIT MUST SURVIVE");
            assert_eq!(m.writes.len(), boundary);
        }
    }
    #[test]
    fn prewrite_selection_drift_is_rejected_without_deleting() {
        let mut m = Model::selected();
        m.drift_on_read = Some(2);
        assert!(execute(&mut m, "x").is_err());
        assert!(m.writes.is_empty());
    }
    #[test]
    fn lost_reply_at_each_native_step_is_unknown_and_not_retried() {
        for boundary in [1, 2, 3] {
            let mut m = Model::selected();
            m.move_caret = false;
            m.lost_reply = Some(boundary);
            let error = execute(&mut m, "x").unwrap_err();
            assert!(error.contains("completion unknown"));
            assert!(m.uncertain());
            assert_eq!(m.writes.len(), boundary);
        }
    }
    #[test]
    fn explicit_insert_rejection_retains_completed_deletion() {
        let mut m = Model::selected();
        m.reject_step = Some(2);
        assert_partial(execute(&mut m, "x").unwrap());
        assert_eq!(m.state.text, "中文测试");
        assert_eq!(m.writes.len(), 2);
        assert!(!m.uncertain());
    }
    #[test]
    fn only_expected_stationary_caret_can_be_advanced() {
        let mut m = Model::selected();
        m.move_caret = false;
        assert!(execute(&mut m, "🙂").unwrap().postcondition_ok);
        assert_eq!(m.writes.last().unwrap(), "Caret(3)");
        assert_eq!(m.state.caret, 3);
    }
}
