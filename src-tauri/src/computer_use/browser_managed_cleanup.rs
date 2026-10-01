//! Preserve probe failures without deleting profiles whose owner is still live.

pub(super) fn finish_probe(
    result: Result<(), String>,
    shutdown: Result<(), String>,
    fixture: Result<(), String>,
    remove_profile: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if let Err(error) = result {
        failures.push(error);
    }
    let physically_closed = shutdown.is_ok();
    if let Err(error) = shutdown {
        failures.push(format!(
            "owned worker cleanup also failed: {error}; probe profile retained"
        ));
    }
    if let Err(error) = fixture {
        failures.push(format!("fixture HTTP cleanup also failed: {error}"));
    }
    if physically_closed {
        if let Err(error) = remove_profile() {
            failures.push(format!("remove probe profile root: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::finish_probe;
    use std::cell::Cell;

    #[test]
    fn confirmed_shutdown_allows_profile_removal() {
        let removed = Cell::new(false);
        assert_eq!(
            finish_probe(Ok(()), Ok(()), Ok(()), || {
                removed.set(true);
                Ok(())
            }),
            Ok(())
        );
        assert!(removed.get());
    }

    #[test]
    fn unconfirmed_shutdown_preserves_profile_and_every_failure() {
        let error = finish_probe(
            Err("primary".into()),
            Err("owner still live".into()),
            Err("fixture failed".into()),
            || panic!("unconfirmed physical cleanup cannot delete a profile"),
        )
        .unwrap_err();
        assert_eq!(error, "primary; owned worker cleanup also failed: owner still live; probe profile retained; fixture HTTP cleanup also failed: fixture failed");
    }

    #[test]
    fn successful_checks_cannot_hide_failed_shutdown() {
        let error = finish_probe(Ok(()), Err("pending".into()), Ok(()), || {
            panic!("profile must be retained")
        })
        .unwrap_err();
        assert!(error.contains("pending"));
        assert!(error.contains("profile retained"));
    }

    #[test]
    fn primary_and_profile_removal_errors_both_survive() {
        let error = finish_probe(Err("primary".into()), Ok(()), Ok(()), || {
            Err("file busy".into())
        })
        .unwrap_err();
        assert_eq!(error, "primary; remove probe profile root: file busy");
    }
}
