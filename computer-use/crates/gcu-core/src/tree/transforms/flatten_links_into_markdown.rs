use crate::model::*;
use crate::tree::*;

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}
fn walk(node: &mut ProcessedNode, in_web: bool) {
    let in_web = in_web || node.node.role == Role::WebArea;
    for child in &mut node.children {
        walk(child, in_web);
    }
    if in_web && node.node.role == Role::Link && !node.protected() && node.node.url.is_some() {
        let mut labels = Vec::new();
        if let Some(name) = node.display_text() {
            labels.push(name.to_owned());
        }
        node.children.retain(|child| {
            if child.plain_text() {
                if let Some(label) = child.display_text() {
                    labels.push(label.to_owned());
                }
                false
            } else {
                true
            }
        });
        labels.dedup();
        let url = node
            .node
            .url
            .as_deref()
            .unwrap_or_default()
            .replace('(', "%28")
            .replace(')', "%29");
        node.node.name = Some(format!("[{}]({})", escape(&labels.join(" ")), url));
        node.node.value = None;
    }
}
/// Render web links as markdown and fold their redundant presentation children.
/// Link roles, actions, fingerprints, and native handles remain independently usable.
pub fn transform(mut tree: ProcessedTree, _: &TreeOpts) -> ProcessedTree {
    if let Some(root) = &mut tree.root {
        walk(root, false);
    }
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_support::*;
    #[test]
    fn flatten_links_into_markdown_web_prose_inlined() {
        let tree = {
            let mut link = n(Role::Link, "Docs", vec![]);
            link.url = Some("https://example.test".into());
            input(vec![n(
                Role::WebArea,
                "Page",
                vec![n(Role::StaticText, "Read", vec![link])],
            )])
        };
        check(
            "flatten_links_into_markdown_web_prose_inlined",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_links_into_markdown_outside_web_retained() {
        let tree = {
            let mut link = n(Role::Link, "Docs", vec![]);
            link.url = Some("https://example.test".into());
            input(vec![n(Role::StaticText, "Read", vec![link])])
        };
        check(
            "flatten_links_into_markdown_outside_web_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
    #[test]
    fn flatten_links_into_markdown_focused_link_retained() {
        let tree = {
            let mut link = n(Role::Link, "Docs", vec![]);
            link.url = Some("https://example.test".into());
            link.states |= States::FOCUSED;
            input(vec![n(
                Role::WebArea,
                "Page",
                vec![n(Role::StaticText, "Read", vec![link])],
            )])
        };
        check(
            "flatten_links_into_markdown_focused_link_retained",
            transform,
            tree,
            TreeOpts::default(),
        );
    }
}
