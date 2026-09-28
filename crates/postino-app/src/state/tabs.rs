//! Open tabs: requests being edited and load test tabs (`plans/ui-redesign.md` phase 8 item 1),
//! in which order, which one is active, and whether each has unsaved changes. No `gpui` types
//! here, so this is unit-tested directly.

use postino_core::Request;

use super::load_test::LoadTestTab;

/// What one open tab shows: a request being edited, or a load test in progress or finished.
/// Dirty state and saving only apply to [`TabKind::Request`] (`plans/ui-redesign.md` phase 8
/// item 1).
#[derive(Debug, Clone, PartialEq)]
pub enum TabKind {
    /// A request open for editing.
    Request(Request),
    /// A load test tab. Boxed: `LoadTestTab` is far larger than `Request` (it carries the run
    /// history and the latest snapshot), and clippy's `large_enum_variant` flags the gap between
    /// the two otherwise.
    LoadTest(Box<LoadTestTab>),
}

/// One open tab.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenTab {
    /// This tab's id: a request's workspace path (matching
    /// [`postino_workspace::RequestEntry::id`]) for [`TabKind::Request`], or a generated,
    /// otherwise-unused id for [`TabKind::LoadTest`] (`state::tabs::TabsState::open_load_test`).
    pub id: String,
    /// What this tab shows.
    pub kind: TabKind,
    /// Whether the tab has unsaved changes. Always `false` for a [`TabKind::LoadTest`] tab: it
    /// has nothing to save.
    pub dirty: bool,
}

impl OpenTab {
    /// This tab's request, if it is a [`TabKind::Request`].
    pub fn request(&self) -> Option<&Request> {
        match &self.kind {
            TabKind::Request(request) => Some(request),
            TabKind::LoadTest(_) => None,
        }
    }

    /// This tab's request, mutably, if it is a [`TabKind::Request`].
    pub fn request_mut(&mut self) -> Option<&mut Request> {
        match &mut self.kind {
            TabKind::Request(request) => Some(request),
            TabKind::LoadTest(_) => None,
        }
    }

    /// This tab's load test state, if it is a [`TabKind::LoadTest`].
    pub fn load_test(&self) -> Option<&LoadTestTab> {
        match &self.kind {
            TabKind::LoadTest(load_test) => Some(load_test.as_ref()),
            TabKind::Request(_) => None,
        }
    }

    /// This tab's load test state, mutably, if it is a [`TabKind::LoadTest`].
    pub fn load_test_mut(&mut self) -> Option<&mut LoadTestTab> {
        match &mut self.kind {
            TabKind::LoadTest(load_test) => Some(load_test.as_mut()),
            TabKind::Request(_) => None,
        }
    }
}

/// The set of open tabs and which one is active, if any.
#[derive(Debug, Default)]
pub struct TabsState {
    open: Vec<OpenTab>,
    active: Option<usize>,
    /// The next id [`Self::open_load_test`] hands out, so every load test tab gets a fresh,
    /// never-reused id even after earlier ones are closed (unlike a request's id, a load test
    /// tab's id has nothing to naturally dedupe on).
    next_load_test_id: u32,
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

    /// The open tab at `index`. `None` if `index` is out of range.
    pub fn get(&self, index: usize) -> Option<&OpenTab> {
        self.open.get(index)
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
        self.push_and_activate(id, TabKind::Request(request))
    }

    /// Opens a new load test tab and makes it active (`plans/ui-redesign.md` phase 8 item 2).
    /// Always a fresh tab, never deduplicated: unlike a request, several load test tabs can
    /// target the same request or collection at once. Returns the tab's id and index.
    pub fn open_load_test(&mut self, load_test: LoadTestTab) -> (String, usize) {
        let id = format!("load-test:{}", self.next_load_test_id);
        self.next_load_test_id += 1;
        let index = self.push_and_activate(id.clone(), TabKind::LoadTest(Box::new(load_test)));
        (id, index)
    }

    /// Appends a new tab and makes it active. Shared by [`Self::open`] and
    /// [`Self::open_load_test`].
    fn push_and_activate(&mut self, id: String, kind: TabKind) -> usize {
        self.open.push(OpenTab {
            id,
            kind,
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
    fn get_returns_the_tab_at_index() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.open("b.postino", request());

        let tab = tabs.get(1).expect("index 1 is open");
        assert_eq!(tab.id, "b.postino");
        assert!(tabs.get(5).is_none());
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

    #[test]
    fn open_load_test_opens_a_new_tab_and_makes_it_active() {
        let mut tabs = TabsState::default();
        let (id, index) = tabs.open_load_test(LoadTestTab::unset());
        assert_eq!(index, 0);
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.open_tabs()[0].id, id);
        assert!(tabs.open_tabs()[0].load_test().is_some());
        assert!(!tabs.open_tabs()[0].dirty);
    }

    #[test]
    fn open_load_test_never_deduplicates_even_with_the_same_target() {
        let mut tabs = TabsState::default();
        let (first_id, _) = tabs.open_load_test(LoadTestTab::unset());
        let (second_id, _) = tabs.open_load_test(LoadTestTab::unset());
        assert_ne!(first_id, second_id);
        assert_eq!(tabs.open_tabs().len(), 2);
    }

    #[test]
    fn a_request_tabs_kind_exposes_its_request_but_not_a_load_test() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        let tab = tabs.active().expect("just opened");
        assert!(tab.request().is_some());
        assert!(tab.load_test().is_none());
    }

    #[test]
    fn a_load_test_tabs_kind_exposes_its_load_test_but_not_a_request() {
        let mut tabs = TabsState::default();
        tabs.open_load_test(LoadTestTab::unset());
        let tab = tabs.active().expect("just opened");
        assert!(tab.load_test().is_some());
        assert!(tab.request().is_none());
    }

    #[test]
    fn rename_prefix_does_not_touch_a_load_test_tab() {
        let mut tabs = TabsState::default();
        let (load_test_id, _) = tabs.open_load_test(LoadTestTab::unset());

        tabs.rename_prefix("load-test", "renamed");

        assert_eq!(tabs.open_tabs()[0].id, load_test_id);
    }

    #[test]
    fn closing_a_tab_leaves_other_tabs_kind_untouched() {
        let mut tabs = TabsState::default();
        tabs.open("a.postino", request());
        tabs.open_load_test(LoadTestTab::unset());

        tabs.close(0);

        assert_eq!(tabs.open_tabs().len(), 1);
        assert!(tabs.open_tabs()[0].load_test().is_some());
    }
}
