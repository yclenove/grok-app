//! Approval window, dialog plan, and persistent grants used by `gcu` and its tests.
//!
//! `GCU_APPROVAL_TEST_AUTOCLICK` is not read by the `gcu` process. Tests pass the
//! hook into [`decide_with_autoclick`] themselves.

pub use crate::host::approval::{
    autoclick_from_env, confirm_for_test, decide_with_autoclick, handoff_for_test, HookRequest,
};
pub use crate::host::dialog::{dialog_plan, DialogCopy, DialogPlan};
pub use crate::host::page::{approval_page, window_problems};
pub use crate::host::policy_store::{
    allows, grant, list_json, list_text, revoke, storage_key, StoreFault,
};
