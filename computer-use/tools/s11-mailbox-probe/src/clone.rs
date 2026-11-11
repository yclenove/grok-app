//! Clone ownership. A VM is owned only when `tart clone` exits 0 and a later
//! trusted list shows that exact name. Anything unclear is not deleted.

use crate::budget::GOLDEN;
use crate::inventory::{self, Vm};
use crate::sha256::is_hex32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolEnd {
    Exited(i32),
    Timeout,
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Precheck {
    Ok,
    Stop { reason: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloneOutcome {
    Owned,
    /// Clone ran and the name is confirmed absent. Do not delete.
    NotCreated {
        reason: &'static str,
    },
    /// Presence is unclear. Do not delete.
    Ambiguous {
        reason: &'static str,
    },
}

pub fn clone_name(attempt: &str) -> Result<String, &'static str> {
    if !is_hex32(attempt) {
        return Err("attempt-id");
    }
    let name = format!("s11-0c-{attempt}");
    if !name_is_safe(&name) {
        return Err("clone-name");
    }
    Ok(name)
}

pub fn name_is_safe(name: &str) -> bool {
    let Some(tail) = name.strip_prefix("s11-0c-") else {
        return false;
    };
    is_hex32(tail) && name != GOLDEN
}

pub fn precheck(list: &Result<Vec<Vm>, &'static str>, name: &str) -> Precheck {
    if !name_is_safe(name) || name == GOLDEN {
        return Precheck::Stop {
            reason: "clone-name",
        };
    }
    let vms = match list {
        Ok(vms) => vms,
        Err(_) => {
            return Precheck::Stop {
                reason: "list-untrusted",
            }
        }
    };
    match inventory::find(vms, GOLDEN) {
        None => {
            return Precheck::Stop {
                reason: "golden-missing",
            }
        }
        Some(vm) if vm.state != "stopped" => {
            return Precheck::Stop {
                reason: "golden-not-stopped",
            }
        }
        Some(_) => {}
    }
    if inventory::find(vms, name).is_some() {
        return Precheck::Stop {
            reason: "name-in-use",
        };
    }
    Precheck::Ok
}

pub fn decide_after_clone(
    end: ToolEnd,
    after: &Result<Vec<Vm>, &'static str>,
    name: &str,
) -> CloneOutcome {
    if !name_is_safe(name) || name == GOLDEN {
        return CloneOutcome::Ambiguous {
            reason: "clone-name",
        };
    }
    match end {
        ToolEnd::Timeout => CloneOutcome::Ambiguous {
            reason: "clone-timeout",
        },
        ToolEnd::Overflow => CloneOutcome::Ambiguous {
            reason: "clone-overflow",
        },
        ToolEnd::Exited(0) => match after {
            Ok(vms) if inventory::find(vms, name).is_some() => CloneOutcome::Owned,
            _ => CloneOutcome::Ambiguous {
                reason: "clone-unconfirmed",
            },
        },
        ToolEnd::Exited(_) => match after {
            Ok(vms) if inventory::find(vms, name).is_none() => CloneOutcome::NotCreated {
                reason: "clone-failed",
            },
            _ => CloneOutcome::Ambiguous {
                reason: "clone-unconfirmed",
            },
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletePlan {
    Delete,
    DoNotDelete { reason: &'static str },
}

pub fn decide_delete(owned: bool, name: &str) -> DeletePlan {
    if name == GOLDEN || !name_is_safe(name) {
        return DeletePlan::DoNotDelete {
            reason: "not-our-vm",
        };
    }
    if !owned {
        return DeletePlan::DoNotDelete {
            reason: "not-owned",
        };
    }
    DeletePlan::Delete
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Absent {
    Yes,
    No,
    Unknown,
}

pub fn confirm_absent(after_delete: &Result<Vec<Vm>, &'static str>, name: &str) -> Absent {
    match after_delete {
        Ok(vms) if inventory::find(vms, name).is_none() => Absent::Yes,
        Ok(_) => Absent::No,
        Err(_) => Absent::Unknown,
    }
}

/// A list is trusted only when the command exited 0 and the body parsed.
/// Nonzero status with `[]` is still untrusted.
pub fn list_from_end(end: ToolEnd, body: &str) -> Result<Vec<Vm>, &'static str> {
    if end != ToolEnd::Exited(0) {
        return Err("list-untrusted");
    }
    inventory::parse_tart_list(body).map_err(|_| "list-untrusted")
}

pub fn confirm_stopped(
    list: &Result<Vec<Vm>, &'static str>,
    name: &str,
) -> Result<(), &'static str> {
    let vms = list.as_ref().map_err(|_| "list-untrusted")?;
    match inventory::find(vms, name) {
        Some(vm) if vm.state == "stopped" => Ok(()),
        _ => Err("not-stopped"),
    }
}

/// This clone is the only VM in `running`. Suspended and stopped do not count.
pub fn sole_running(vms: &[Vm], name: &str) -> bool {
    let mut found = false;
    for vm in vms {
        if vm.state == "running" {
            if found || vm.name != name {
                return false;
            }
            found = true;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(name: &str) -> Vm {
        Vm {
            name: name.into(),
            state: "stopped".into(),
        }
    }

    fn id() -> String {
        "0123456789abcdef0123456789abcdef".into()
    }

    #[test]
    fn owns_only_a_confirmed_new_name_and_never_golden() {
        let name = clone_name(&id()).unwrap();
        let before = Ok(vec![vm(GOLDEN), vm("other-vm")]);
        assert_eq!(precheck(&before, &name), Precheck::Ok);
        assert_eq!(
            precheck(&before, GOLDEN),
            Precheck::Stop {
                reason: "clone-name"
            }
        );
        assert_eq!(
            precheck(&Ok(vec![vm(GOLDEN), vm(&name)]), &name),
            Precheck::Stop {
                reason: "name-in-use"
            }
        );
        assert_eq!(
            precheck(&Ok(vec![vm("other-vm")]), &name),
            Precheck::Stop {
                reason: "golden-missing"
            }
        );
        assert!(matches!(
            precheck(&Err("list-untrusted"), &name),
            Precheck::Stop { .. }
        ));

        let after = Ok(vec![vm(GOLDEN), vm(&name)]);
        assert_eq!(
            decide_after_clone(ToolEnd::Exited(0), &after, &name),
            CloneOutcome::Owned
        );
        assert_eq!(
            decide_after_clone(ToolEnd::Exited(1), &Ok(vec![vm(GOLDEN)]), &name),
            CloneOutcome::NotCreated {
                reason: "clone-failed"
            }
        );
        assert_eq!(
            decide_after_clone(ToolEnd::Exited(1), &Err("list-untrusted"), &name),
            CloneOutcome::Ambiguous {
                reason: "clone-unconfirmed"
            }
        );
        assert_eq!(
            decide_after_clone(ToolEnd::Exited(0), &Ok(vec![vm(GOLDEN)]), &name),
            CloneOutcome::Ambiguous {
                reason: "clone-unconfirmed"
            }
        );
        assert!(matches!(
            decide_after_clone(ToolEnd::Timeout, &after, &name),
            CloneOutcome::Ambiguous { .. }
        ));
        assert_eq!(
            decide_delete(false, &name),
            DeletePlan::DoNotDelete {
                reason: "not-owned"
            }
        );
        assert_eq!(
            decide_delete(true, GOLDEN),
            DeletePlan::DoNotDelete {
                reason: "not-our-vm"
            }
        );
        assert_eq!(
            decide_delete(true, "other-vm"),
            DeletePlan::DoNotDelete {
                reason: "not-our-vm"
            }
        );
        assert_eq!(decide_delete(true, &name), DeletePlan::Delete);
        assert_eq!(confirm_absent(&Ok(vec![vm(GOLDEN)]), &name), Absent::Yes);
        assert_eq!(confirm_absent(&Ok(vec![vm(&name)]), &name), Absent::No);
        assert_eq!(confirm_absent(&Err("x"), &name), Absent::Unknown);
        assert!(list_from_end(ToolEnd::Exited(1), "[]").is_err());
        assert!(list_from_end(ToolEnd::Timeout, "[]").is_err());
        assert!(list_from_end(ToolEnd::Exited(0), "[]").unwrap().is_empty());
        let running = Vm {
            name: name.clone(),
            state: "running".into(),
        };
        let golden_run = Vm {
            name: GOLDEN.into(),
            state: "running".into(),
        };
        assert!(sole_running(&[vm(GOLDEN), running.clone()], &name));
        assert!(!sole_running(&[golden_run, running], &name));
        assert!(confirm_stopped(
            &Ok(vec![Vm {
                name: name.clone(),
                state: "stopped".into()
            }]),
            &name
        )
        .is_ok());
        assert!(confirm_stopped(
            &Ok(vec![Vm {
                name: name.clone(),
                state: "suspended".into()
            }]),
            &name
        )
        .is_err());
        assert!(confirm_stopped(&Err("list-untrusted"), &name).is_err());
        let golden_run = Ok(vec![Vm {
            name: GOLDEN.into(),
            state: "running".into(),
        }]);
        assert_eq!(
            precheck(&golden_run, &name),
            Precheck::Stop {
                reason: "golden-not-stopped"
            }
        );
    }
}
