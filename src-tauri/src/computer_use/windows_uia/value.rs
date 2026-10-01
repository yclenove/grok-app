//! Text policy for the UIA Value pattern. A provider call is never replayed.
use crate::computer_use::protocol::{ActionKind, TEXT_MAX_CHARS};

pub(super) fn apply(
    action: ActionKind,
    text: &str,
    check: &dyn Fn() -> Result<(), String>,
    read: &dyn Fn() -> Result<String, String>,
    write: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    if text.contains('\0') || text.chars().count() > TEXT_MAX_CHARS {
        return Err("invalid UIA text request".into());
    }
    check()?;
    let expected = match action {
        ActionKind::SetValue => text.to_string(),
        ActionKind::TypeText => {
            if text.is_empty() {
                return Ok(());
            }
            let before = read()?;
            if before.contains('\0')
                || before.encode_utf16().count() + text.encode_utf16().count() > 1024 * 1024
            {
                return Err("UIA append exceeds bounded verification capacity".into());
            }
            format!("{before}{text}")
        }
        _ => return Err("action does not use the UIA Value pattern".into()),
    };
    check()?;
    write(&expected)?;
    if read()? != expected {
        return Err("UIA text effect is unverified; input was not replayed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn append_preserves_existing_text_and_set_value_replaces() {
        let value = RefCell::new("原文🙂".to_string());
        let writes = Cell::new(0);
        let read = || Ok(value.borrow().clone());
        let write = |s: &str| {
            writes.set(writes.get() + 1);
            *value.borrow_mut() = s.into();
            Ok(())
        };
        apply(ActionKind::TypeText, "追加", &|| Ok(()), &read, &write).unwrap();
        assert_eq!(&*value.borrow(), "原文🙂追加");
        apply(
            ActionKind::SetValue,
            "replacement",
            &|| Ok(()),
            &read,
            &write,
        )
        .unwrap();
        assert_eq!(&*value.borrow(), "replacement");
        assert_eq!(writes.get(), 2);
    }

    #[test]
    fn ignored_provider_write_is_not_proven_by_existing_substring() {
        let writes = Cell::new(0);
        let result = apply(
            ActionKind::TypeText,
            "text",
            &|| Ok(()),
            &|| Ok("old text".into()),
            &|_| {
                writes.set(writes.get() + 1);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(writes.get(), 1);
    }

    #[test]
    fn failed_pre_read_never_dispatches_an_append() {
        assert!(apply(
            ActionKind::TypeText,
            "x",
            &|| Ok(()),
            &|| Err("provider unavailable".into()),
            &|_| panic!("must not write")
        )
        .is_err());
    }

    #[test]
    fn late_cancel_never_dispatches_a_value_write() {
        let calls = Cell::new(0);
        let check = || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err("canceled".into())
            } else {
                Ok(())
            }
        };
        assert!(apply(
            ActionKind::TypeText,
            "x",
            &check,
            &|| Ok("old".into()),
            &|_| panic!("must not write")
        )
        .is_err());
    }

    #[test]
    fn failed_write_or_readback_is_never_retried() {
        for fail_write in [true, false] {
            let writes = Cell::new(0);
            assert!(apply(
                ActionKind::SetValue,
                "x",
                &|| Ok(()),
                &|| Err("read failed".into()),
                &|_| {
                    writes.set(writes.get() + 1);
                    if fail_write {
                        Err("write failed".into())
                    } else {
                        Ok(())
                    }
                }
            )
            .is_err());
            assert_eq!(writes.get(), 1);
        }
    }

    #[test]
    fn empty_append_does_not_mutate_or_read_the_field() {
        apply(
            ActionKind::TypeText,
            "",
            &|| Ok(()),
            &|| panic!("no read needed"),
            &|_| panic!("no write needed"),
        )
        .unwrap();
    }

    #[test]
    fn invalid_requests_and_oversized_existing_values_do_not_write() {
        for text in ["a\0b".into(), "a".repeat(TEXT_MAX_CHARS + 1)] {
            assert!(apply(
                ActionKind::TypeText,
                &text,
                &|| Ok(()),
                &|| panic!("invalid input"),
                &|_| panic!("must not write")
            )
            .is_err());
        }
        for before in ["old\0text".into(), "a".repeat(1024 * 1024)] {
            assert!(apply(
                ActionKind::TypeText,
                "x",
                &|| Ok(()),
                &|| Ok(before.clone()),
                &|_| panic!("must not write")
            )
            .is_err());
        }
    }
}
