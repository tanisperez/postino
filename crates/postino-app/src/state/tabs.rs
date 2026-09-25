//! Open request tabs: which requests are open for editing, in which order, which one is active,
//! and whether each has unsaved changes. No `gpui` types here, so this is unit-tested directly.

use postino_core::Request;

/// One open request tab.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenTab {
    /// The request's id in the workspace: its path relative to the workspace root, matching
    /// [`postino_workspace::RequestEntry::id`].
    pub id: String,
    /// The request as currently edited. May differ from what is saved on disk when `dirty`.
    pub request: Request,
    /// Whether `request` has unsaved changes.
    pub dirty: bool,
}

/// The set of open tabs and which one is active, if any.
#[derive(Debug, Default)]
pub struct TabsState {
    open: Vec<OpenTab>,
    active: Option<usize>,
}

impl TabsState {
    /// The open tabs, in the order they were opened.
    pub fn open_tabs(&self) -> &[OpenTab] {
        &self.open
    }

    /// The index of the active tab, if any tab is open.
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    /// The active tab, if any.
    pub fn active(&self) -> Option<&OpenTab> {
        self.active.and_then(|index| self.open.get(index))
    }

    /// The open tab at `index`, mutably. `None` if `index` is out of range.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut OpenTab> {
        self.open.get_mut(index)
    }

    /// The index of the open tab for `id`, if it is already open.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.open.iter().position(|tab| tab.id == id)
    }

    /// Opens `request` as a tab and makes it active. If `id` is already open, its tab is made
    /// active instead of opening a second one, like a normal editor's "one tab per file".
    /// Returns the tab's index.
    pub fn open(&mut self, id: impl Into<String>, request: Request) -> usize {
        let id = id.into();
        if let Some(index) = self.index_of(&id) {
            self.active = Some(index);
            return index;
        }
        self.open.push(OpenTab {
            id,
            request,
            dirty: false,
        });
        let index = self.open.len() - 1;
        self.active = Some(index);
        index
    }

    /// Closes the tab at `index`. If it was the active tab, the tab that takes its place (the
    /// next one, or the previous one if the closed tab was last) becomes active. Does nothing if
    /// `index` is out of range.
    pub fn close(&mut self, index: usize) {
        if index >= self.open.len() {
            return;
        }
        self.open.remove(index);
        self.active = self.active.and_then(|active| {
            if self.open.is_empty() {
                None
            } else if active < index {
                Some(active)
            } else if active > index {
                Some(active - 1)
            } else {
                Some(index.min(self.open.len() - 1))
            }
        });
    }

    /// Makes the tab at `index` active. Does nothing if `index` is out of range.
    pub fn set_active(&mut self, index: usize) {
        if index < self.open.len() {
            self.active = Some(index);
        }
    }

    /// Marks the tab at `index` as saved (no unsaved changes). Does nothing if out of range.
    pub fn mark_saved(&mut self, index: usize) {
        if let Some(tab) = self.open.get_mut(index) {
            tab.dirty = false;
        }
    }

    /// Marks the tab at `index` as having unsaved changes. Does nothing if out of range. Called
    /// by every editing operation in `state::request_edit` once it has applied its change to the
    /// tab's `Request` (Phase 9: "any edit marks the tab dirty").
    pub fn mark_dirty(&mut self, index: usize) {
        if let Some(tab) = self.open.get_mut(index) {
            tab.dirty = true;
        }
    }

    /// Updates the id of every open tab under `old_prefix` to the same path under `new_prefix`,
    /// after [`postino_workspace::Workspace::rename`] moves a request or a whole folder. A tab's
    /// id matches when it equals `old_prefix` exactly (the renamed item was itself a request) or
    /// starts with `old_prefix` followed by `/` (the renamed item was a folder and the tab is
    /// one of its descendants).
    pub fn rename_prefix(&mut self, old_prefix: &str, new_prefix: &str) {
        for tab in &mut self.open {
            if let Some(rest) = matching_rest(&tab.id, old_prefix) {
                tab.id = format!("{new_prefix}{rest}");
            }
        }
    }

    /// Closes every open tab under `prefix` (matched the same way as [`Self::rename_prefix`]),
    /// after [`postino_workspace::Workspace::delete`] removes a request or a whole folder.
    pub fn close_prefix(&mut self, prefix: &str) {
        let mut index = 0;
        while index < self.open.len() {
            if matching_rest(&self.open[index].id, prefix).is_some() {
                self.close(index);
            } else {
                index += 1;
            }
        }
    }
}

/// If `id` equals `prefix` or starts with `prefix` followed by `/`, returns the rest of `id`
/// after `prefix` (an empty string for an exact match). `None` otherwise.
fn matching_rest<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = id.strip_prefix(prefix)?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request::default()
    }

    #[test]
    fn open_adds_a_new_tab_and_makes_it_active() {
        let mut tabs = TabsState::default();
        let index = tabs.open("a.postino", request());
        assert_eq!(index, 0);
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.open_tabs().len(), 1);
        assert_eq!(tabs.active().map(|tab| tab.id.as_str()), Some("a.postino"));
    }

    #[test]
    fn open_reuses_an_already_open_tab_instead_of_duplicating_it() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.open("b.postino", request());
        let index = tabs.open("a.postino", request());
        assert_eq!(index, 0);
        assert_eq!(tabs.open_tabs().len(), 2);
        assert_eq!(tabs.active_index(), Some(0));
    }

    #[test]
    fn closing_the_active_tab_activates_the_tab_that_took_its_place() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.open("b.postino", request());
        tabs.open("c.postino", request());
        tabs.set_active(1);

        tabs.close(1);

        assert_eq!(tabs.open_tabs().len(), 2);
        assert_eq!(tabs.active_index(), Some(1));
        assert_eq!(tabs.open_tabs()[1].id, "c.postino");
    }

    #[test]
    fn closing_the_last_remaining_tab_leaves_nothing_active() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());

        tabs.close(0);

        assert!(tabs.open_tabs().is_empty());
        assert_eq!(tabs.active_index(), None);
    }

    #[test]
    fn closing_a_tab_before_the_active_one_shifts_the_active_index_left() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.open("b.postino", request());
        tabs.set_active(1);

        tabs.close(0);

        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.open_tabs()[0].id, "b.postino");
    }

    #[test]
    fn closing_an_out_of_range_index_does_nothing() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());

        tabs.close(5);

        assert_eq!(tabs.open_tabs().len(), 1);
    }

    #[test]
    fn mark_saved_clears_the_dirty_flag() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.mark_dirty(0);

        tabs.mark_saved(0);

        assert!(!tabs.open_tabs()[0].dirty);
    }

    #[test]
    fn mark_dirty_sets_the_dirty_flag() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());

        tabs.mark_dirty(0);

        assert!(tabs.open_tabs()[0].dirty);
    }

    #[test]
    fn mark_dirty_on_an_out_of_range_index_does_nothing() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());

        tabs.mark_dirty(5);

        assert!(!tabs.open_tabs()[0].dirty);
    }

    #[test]
    fn get_mut_returns_the_tab_at_index() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.open("b.postino", request());

        let tab = tabs.get_mut(0).expect("index 0 is open");
        assert_eq!(tab.id, "a.postino");
    }

    #[test]
    fn rename_prefix_updates_an_exact_match() {
        let mut tabs = TabsState::default();
        tabs.open("auth/login.postino", request());

        tabs.rename_prefix("auth/login.postino", "auth/sign-in.postino");

        assert_eq!(tabs.open_tabs()[0].id, "auth/sign-in.postino");
    }

    #[test]
    fn rename_prefix_updates_descendants_of_a_renamed_folder() {
        let mut tabs = TabsState::default();
        tabs.open("auth/login.postino", request());
        tabs.open("other.postino", request());

        tabs.rename_prefix("auth", "identity");

        assert_eq!(tabs.open_tabs()[0].id, "identity/login.postino");
        assert_eq!(tabs.open_tabs()[1].id, "other.postino");
    }

    #[test]
    fn rename_prefix_does_not_match_a_sibling_with_a_shared_prefix() {
        let mut tabs = TabsState::default();
        tabs.open("auth-extra.postino", request());

        tabs.rename_prefix("auth", "identity");

        assert_eq!(tabs.open_tabs()[0].id, "auth-extra.postino");
    }

    #[test]
    fn close_prefix_closes_an_exact_match_and_descendants() {
        let mut tabs = TabsState::default();
        tabs.open("auth/login.postino", request());
        tabs.open("auth/logout.postino", request());
        tabs.open("other.postino", request());

        tabs.close_prefix("auth");

        assert_eq!(tabs.open_tabs().len(), 1);
        assert_eq!(tabs.open_tabs()[0].id, "other.postino");
    }
}
