//! The sidebar's multi-selection: the requests and folders picked with Ctrl+click (Cmd+click on
//! macOS) to move them together by drag and drop. Pure logic over workspace ids, so it is
//! unit-tested without `gpui`; `views/sidebar.rs` feeds it the clicks and `views/root.rs` moves the
//! entries. The tree's own single selection is separate: while this one is empty, it alone is
//! highlighted.

use std::collections::HashSet;

use postino_workspace::Node;

use super::tree_drop::{drop_target, is_within, moved_id};

/// The ids picked with Ctrl+click, in the order they were picked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TreeSelection {
    ids: Vec<String>,
}

impl TreeSelection {
    /// Whether nothing is picked, so the tree's own selection is the one shown.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Whether `id` is picked.
    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|picked| picked == id)
    }

    /// Ctrl+click on `id`: adds it, or removes it when it was already picked. The first one also
    /// picks `anchor`, the row the tree had selected, so Ctrl+click extends what is highlighted.
    pub fn toggle(&mut self, id: &str, anchor: Option<&str>) {
        if self.ids.is_empty()
            && let Some(anchor) = anchor.filter(|anchor| *anchor != id)
        {
            self.ids.push(anchor.to_string());
        }
        match self.ids.iter().position(|picked| picked == id) {
            Some(ix) => {
                self.ids.remove(ix);
            }
            None => self.ids.push(id.to_string()),
        }
    }

    /// Forgets every picked id, after a plain click.
    pub fn clear(&mut self) {
        self.ids.clear();
    }

    /// Keeps only the ids `exists` still accepts, after the workspace changed on disk.
    pub fn retain_existing(&mut self, exists: impl Fn(&str) -> bool) {
        self.ids.retain(|id| exists(id));
    }

    /// Follows the entry `old` (and everything below it) to its new id `new` after a move.
    pub fn follow_move(&mut self, old: &str, new: &str) {
        for id in &mut self.ids {
            *id = moved_id(id, old, new);
        }
    }

    /// What dragging the row `id` moves: every picked entry when `id` is one of them, otherwise
    /// `id` alone. An entry inside a picked folder is left out, since it moves with its folder.
    pub fn dragged_ids(&self, id: &str) -> Vec<String> {
        if !self.contains(id) {
            return vec![id.to_string()];
        }
        self.ids
            .iter()
            .filter(|picked| {
                !self
                    .ids
                    .iter()
                    .any(|other| other != *picked && is_within(picked, other))
            })
            .cloned()
            .collect()
    }
}

/// The id of every request and folder in `nodes`, for [`TreeSelection::retain_existing`].
pub fn ids_in(nodes: &[Node]) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut pending: Vec<&Node> = nodes.iter().collect();
    while let Some(node) = pending.pop() {
        ids.insert(node.id().to_string());
        if let Node::Folder(folder) = node {
            pending.extend(&folder.children);
        }
    }
    ids
}

/// Where the entries `ids` go when dropped with the pointer over `hovered`: `None` when it is the
/// folder of every one of them, since nothing would change, or when one of them is that folder or
/// holds it. An entry already in the target stays where it is.
pub fn drop_target_for(ids: &[String], hovered: Option<String>) -> Option<String> {
    let folder = hovered?;
    let blocked = ids.iter().any(|id| is_within(&folder, id));
    let moves_something = ids
        .iter()
        .any(|id| drop_target(id, Some(folder.clone())).is_some());
    (!blocked && moves_something).then_some(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn the_first_toggle_also_picks_the_anchor() {
        let mut selection = TreeSelection::default();
        selection.toggle("b.postino", Some("a.postino"));
        assert_eq!(selection.ids, ids(&["a.postino", "b.postino"]));

        let mut selection = TreeSelection::default();
        selection.toggle("a.postino", Some("a.postino"));
        assert_eq!(selection.ids, ids(&["a.postino"]));

        let mut selection = TreeSelection::default();
        selection.toggle("a.postino", None);
        assert_eq!(selection.ids, ids(&["a.postino"]));
    }

    #[test]
    fn toggling_a_picked_id_removes_it_and_later_anchors_are_ignored() {
        let mut selection = TreeSelection::default();
        selection.toggle("b.postino", Some("a.postino"));
        selection.toggle("c.postino", Some("b.postino"));
        selection.toggle("a.postino", Some("c.postino"));
        assert_eq!(selection.ids, ids(&["b.postino", "c.postino"]));
        assert!(selection.contains("b.postino"));
        assert!(!selection.contains("a.postino"));
        selection.clear();
        assert!(selection.is_empty());
    }

    #[test]
    fn dragging_a_picked_row_moves_every_picked_entry() {
        let mut selection = TreeSelection::default();
        selection.toggle("auth/login.postino", Some("ping.postino"));
        assert_eq!(
            selection.dragged_ids("ping.postino"),
            ids(&["ping.postino", "auth/login.postino"])
        );
        assert_eq!(
            selection.dragged_ids("other.postino"),
            ids(&["other.postino"])
        );
    }

    #[test]
    fn an_entry_inside_a_picked_folder_moves_with_it() {
        let mut selection = TreeSelection::default();
        selection.toggle("auth/login.postino", Some("auth"));
        selection.toggle("auth-extra/x.postino", None);
        assert_eq!(
            selection.dragged_ids("auth"),
            ids(&["auth", "auth-extra/x.postino"])
        );
    }

    #[test]
    fn follow_move_and_retain_existing_keep_the_ids_current() {
        let mut selection = TreeSelection::default();
        selection.toggle("auth/login.postino", Some("auth"));
        selection.toggle("ping.postino", None);
        selection.follow_move("auth", "users/auth");
        assert_eq!(
            selection.ids,
            ids(&["users/auth", "users/auth/login.postino", "ping.postino"])
        );
        selection.retain_existing(|id| id != "ping.postino");
        assert_eq!(
            selection.ids,
            ids(&["users/auth", "users/auth/login.postino"])
        );
    }

    #[test]
    fn drop_target_for_needs_something_to_move_and_never_a_folder_into_itself() {
        let picked = ids(&["auth/login.postino", "users/list.postino"]);
        assert_eq!(
            drop_target_for(&picked, Some("auth".into())),
            Some("auth".into())
        );
        assert_eq!(
            drop_target_for(&picked, Some(String::new())),
            Some(String::new())
        );
        assert_eq!(drop_target_for(&picked, None), None);

        let same_folder = ids(&["auth/login.postino", "auth/logout.postino"]);
        assert_eq!(drop_target_for(&same_folder, Some("auth".into())), None);

        let with_folder = ids(&["auth", "ping.postino"]);
        assert_eq!(drop_target_for(&with_folder, Some("auth".into())), None);
        assert_eq!(
            drop_target_for(&with_folder, Some("auth/admin".into())),
            None
        );
        assert_eq!(
            drop_target_for(&with_folder, Some("users".into())),
            Some("users".into())
        );
    }
}
