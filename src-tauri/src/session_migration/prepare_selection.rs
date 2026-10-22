//! Resolve the supplied selector, never infer a group's current primary later.
use super::{
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    group_selection, scan, scan_selection,
};
use rusqlite::params;

#[cfg(test)]
type PageObserver = (String, Box<dyn FnMut()>);
#[cfg(test)]
thread_local! {
    static PAGE_OBSERVER: std::cell::RefCell<Option<PageObserver>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) struct PageObserverGuard;
#[cfg(test)]
impl Drop for PageObserverGuard {
    fn drop(&mut self) {
        PAGE_OBSERVER.with(|observer| {
            observer.borrow_mut().take();
        });
    }
}
#[cfg(test)]
pub(crate) fn observe_pages_for_test(
    scan_id: String,
    callback: impl FnMut() + 'static,
) -> PageObserverGuard {
    PAGE_OBSERVER.with(|observer| {
        let mut observer = observer.borrow_mut();
        assert!(
            observer.is_none(),
            "only one owned prepare-page observer per thread"
        );
        *observer = Some((scan_id, Box::new(callback)));
    });
    PageObserverGuard
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectionSnapshot {
    Candidates(scan_selection::CandidateSelection),
    Groups(group_selection::GroupSelection),
}

impl SelectionSnapshot {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Candidates(s) => &s.selection_id,
            Self::Groups(s) => &s.selection_id,
        }
    }
    pub(crate) fn count(&self) -> u32 {
        match self {
            Self::Candidates(s) => s.selected_count,
            Self::Groups(s) => s.selected_count,
        }
    }
    pub(crate) fn state_version(&self) -> u64 {
        match self {
            Self::Candidates(s) => s.state_version,
            Self::Groups(s) => s.state_version,
        }
    }
    pub(crate) fn page(
        &self,
        scan_id: &str,
        epoch: u64,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<(Vec<scan::CandidateRef>, Option<String>, u32), ImportError> {
        let result = match self {
            Self::Candidates(_) => {
                scan_selection::page_refs(scan_id, epoch, self.id(), cursor, limit)
            }
            Self::Groups(_) => group_selection::page_refs(scan_id, epoch, self.id(), cursor, limit),
        };
        #[cfg(test)]
        if result.is_ok() {
            PAGE_OBSERVER.with(|observer| {
                if let Some((owner, callback)) = observer.borrow_mut().as_mut() {
                    if owner == scan_id {
                        callback();
                    }
                }
            });
        }
        result
    }
}

pub(crate) fn get(scan_id: &str, epoch: u64, id: &str) -> Result<SelectionSnapshot, ImportError> {
    let conn = scan::open_scan_readonly(scan_id, epoch)?;
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    let candidate = scan_selection::descriptor_from(&tx, scan_id, epoch, None)?;
    let has_groups: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='group_selections')", [], |row| row.get(0)).map_err(sqlite_storage_error)?;
    let group = if has_groups {
        tx.query_row("SELECT EXISTS(SELECT 1 FROM group_selections WHERE scan_id=?1 AND epoch=?2 AND selection_id=?3)", params![scan_id,epoch,id], |row| row.get::<_,bool>(0)).map_err(sqlite_storage_error)?
    } else {
        false
    };
    match (candidate.selection_id == id, group) {
        (true, false) => Ok(SelectionSnapshot::Candidates(candidate)),
        (false, true) => Ok(SelectionSnapshot::Groups(group_selection::get_owned(
            scan_id, epoch, id,
        )?)),
        _ => Err(ImportError::new(
            ErrorCode::PlanStale,
            "selector is unknown, ambiguous or belongs to another scan",
        )),
    }
}
