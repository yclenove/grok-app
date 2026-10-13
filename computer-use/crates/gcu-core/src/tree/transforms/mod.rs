/// Architecture §4.1 step 13: assign indices.
pub mod assign_indices;
/// Architecture §4.1 step 3: associate title elements.
pub mod associate_title_elements;
/// Architecture §4.1 step 12: budget.
pub mod budget;
/// Architecture §4.1 step 2: filter actions.
pub mod filter_actions;
/// Architecture §4.1 step 6: flatten into selectable ancestor.
pub mod flatten_into_selectable_ancestor;
/// Architecture §4.1 step 11: flatten links into markdown.
pub mod flatten_links_into_markdown;
/// Architecture §4.1 step 8: flatten redundant hierarchy.
pub mod flatten_redundant_hierarchy;
/// Architecture §4.1 step 10: flatten repetitive static text.
pub mod flatten_repetitive_static_text;
/// Architecture §4.1 step 7: merge single item groups.
pub mod merge_single_item_groups;
/// Architecture §4.1 step 9: merge text only siblings.
pub mod merge_text_only_siblings;
/// Architecture §4.1 step 4: prune empty disabled.
pub mod prune_empty_disabled;
/// Architecture §4.1 step 5: prune non descriptive.
pub mod prune_non_descriptive;
/// Architecture §4.1 step 1: prune offscreen.
pub mod prune_offscreen;
