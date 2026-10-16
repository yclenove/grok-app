//! Conservative clipboard exposure classification, independent of approvals.
use super::{risk, terms::TermIndex, text::fold, ActionIntent, Tool};
use crate::{driver::TreeOpts, model::RawTree, tree::Pipeline};
use gcu_protocol::AppInfo;

/// Whether clipboard exposure must be refused for this text and exact ancestry.
/// Reuses core's password, browser, unknown-target and fifteen-locale sensitive
/// action detection. Approval never makes a high-risk clipboard target safe.
/// The raw tree must contain the exact focused node and all ancestors; missing
/// or incomplete facts fail closed. This synchronous helper performs no IO and
/// has no cancellation points. Credential hints are conservative, not exhaustive.
pub fn is_sensitive(app: &AppInfo, ancestry: RawTree, text: &str) -> bool {
    if ancestry.incomplete.is_some() || text.contains('\0') {
        return true;
    }
    let tree = Pipeline::default().run(ancestry, &TreeOpts::default());
    let terms = TermIndex::builtin();
    let mut intent = ActionIntent::new(Tool::TypeText);
    intent.text = Some(text.to_owned());
    intent.element_index = tree.focused;
    if super::target::resolve(&tree, &intent).is_none_or(|hit| hit.execution) {
        return true;
    }
    let high = risk::clipboard_high_risk(terms, app, &tree, &intent);
    let folded = fold(text);
    high || terms.hit(&[text])
        || [
            "password",
            "passwd",
            "api_key",
            "api-key",
            "access_token",
            "access-token",
            "private key",
            "-----begin",
            "bearer ",
            "secret",
            "sk-",
            "密码",
            "密碼",
            "пароль",
            "パスワード",
        ]
        .iter()
        .any(|hint| folded.contains(hint))
}
