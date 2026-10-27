//! The diff tab (designs 2a, 2b, 7a): a header with the file, its kind of change, the change
//! counter with previous and next, the layout switch and "Open request", over gpui-component's
//! diff viewer. The patch is read and parsed on the background executor whenever the tab opens
//! a file or the git status changes, never in render.

use gpui_kit::assets::IconName;
use gpui_kit::component::diff::{
    Diff, DiffFile, DiffHoverHighlight, DiffHunkSeparator, DiffMode, DiffState,
};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_git::DiffSource;
use rust_i18n::t;

use crate::state::git_diff::{DiffSummary, DiffTab, DiffTarget, summarize};
use crate::state::number::format_integer;
use crate::state::settings::GitDiffMode;
use crate::theme::metrics::{RAIL_WIDTH, SIDEBAR_WIDTH};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{
    EmptyState, IconButton, SecondaryButton, SegmentedControl, SegmentedItem,
};

use super::super::root::AppView;

/// Below this width of the main area the diff is unified, whatever the setting says: two columns
/// of code would be too narrow to read (design 7a).
const SPLIT_MIN_WIDTH: f32 = 720.0;

/// The live viewer of the diff tab.
pub(crate) struct DiffView {
    /// The viewer's state: files, layout, scroll.
    state: Entity<DiffState>,
    /// The tab generation the state shows.
    generation: u64,
    /// The change the header counts from, 0 based.
    current: usize,
}

impl AppView {
    /// Shows the changes of `repo_path` (shown as `path`) in the diff tab.
    pub(crate) fn open_git_diff(
        &mut self,
        repo_path: String,
        path: String,
        target: DiffTarget,
        letter: char,
        cx: &mut Context<Self>,
    ) {
        log::debug!("open diff tab");
        self.state
            .tabs
            .open_diff(DiffTab::new(repo_path, path, target, letter));
        self.load_git_diff(cx);
        cx.notify();
    }

    /// Reads the open diff tab's patch again, if there is a diff tab. Called when it opens a file
    /// and when the git status changes.
    pub(crate) fn load_git_diff(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.git.repo().cloned() else {
            return;
        };
        let Some(index) = self.state.tabs.index_of(crate::state::git_diff::TAB_ID) else {
            self.git_tasks.diff_view = None;
            return;
        };
        let Some(tab) = self
            .state
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.diff_mut())
        else {
            return;
        };
        tab.generation += 1;
        let generation = tab.generation;
        let repo_path = tab.repo_path.clone();
        let source = match &tab.target {
            DiffTarget::WorkTree => DiffSource::HeadToWorkTree,
        };
        self.git_tasks.diff_load = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let patch = repo.diff(&repo_path, &source)?;
                    let summary = summarize(&patch);
                    let files = DiffFile::parse(&patch).unwrap_or_default();
                    Ok::<_, postino_git::GitError>((summary, files))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.finish_loading_diff(generation, result, cx);
            });
        }));
    }

    /// Puts a read patch into the tab and its viewer, unless the tab moved on meanwhile.
    fn finish_loading_diff(
        &mut self,
        generation: u64,
        result: Result<(DiffSummary, Vec<DiffFile>), postino_git::GitError>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.state.tabs.index_of(crate::state::git_diff::TAB_ID) else {
            return;
        };
        let Some(tab) = self
            .state
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.diff_mut())
        else {
            return;
        };
        if tab.generation != generation {
            return;
        }
        let files = match result {
            Ok((summary, files)) => {
                tab.summary = Some(summary);
                files
            }
            Err(error) => {
                log::warn!("git diff failed: {error}");
                tab.summary = Some(DiffSummary {
                    error: Some(error.to_string()),
                    ..DiffSummary::default()
                });
                Vec::new()
            }
        };
        let mode = to_diff_mode(self.state.settings.git_diff_mode);
        match &mut self.git_tasks.diff_view {
            Some(view) => {
                view.state
                    .update(cx, |state, cx| state.set_files(files, cx));
                view.generation = generation;
                view.current = 0;
            }
            None => {
                let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
                self.git_tasks.diff_view = Some(DiffView {
                    state,
                    generation,
                    current: 0,
                });
            }
        }
        cx.notify();
    }

    /// Switches the diff layout and remembers it.
    fn set_git_diff_mode(
        &mut self,
        mode: GitDiffMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.settings.git_diff_mode == mode {
            return;
        }
        self.state.settings.git_diff_mode = mode;
        self.apply_settings_live(window, cx);
    }

    /// Moves to the next (or previous) change of the diff.
    fn step_git_diff(&mut self, forward: bool, cx: &mut Context<Self>) {
        let hunks = self
            .state
            .tabs
            .active()
            .and_then(|tab| tab.diff())
            .and_then(|tab| tab.summary.as_ref())
            .map_or(0, |summary| summary.hunks);
        let Some(view) = self.git_tasks.diff_view.as_mut() else {
            return;
        };
        if hunks == 0 {
            return;
        }
        view.current = if forward {
            (view.current + 1).min(hunks - 1)
        } else {
            view.current.saturating_sub(1)
        };
        view.state.update(cx, |state, cx| {
            if forward {
                state.next_change(cx);
            } else {
                state.previous_change(cx);
            }
        });
        cx.notify();
    }

    /// Renders the active diff tab.
    pub(crate) fn render_diff_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let Some(tab) = self.state.tabs.active().and_then(|tab| tab.diff()).cloned() else {
            return div().into_any_element();
        };
        // The main area's width, without measuring a frame: the window less the rail and the
        // sidebar when it shows.
        let sidebar = if self.nav.sidebar_visible() {
            SIDEBAR_WIDTH
        } else {
            0.0
        };
        let narrow =
            f32::from(window.viewport_size().width) - RAIL_WIDTH - sidebar < SPLIT_MIN_WIDTH;
        let chosen = self.state.settings.git_diff_mode;
        let effective = if narrow { GitDiffMode::Unified } else { chosen };
        if let Some(view) = &self.git_tasks.diff_view {
            let mode = to_diff_mode(effective);
            if view.state.read(cx).mode() != mode {
                view.state.update(cx, |state, cx| state.set_mode(mode, cx));
            }
        }

        let header = self.render_diff_header(&tab, chosen, narrow, &palette, cx);
        let body = match (&tab.summary, &self.git_tasks.diff_view) {
            (None, _) | (_, None) => div().flex_1().into_any_element(),
            (Some(summary), _) if summary.error.is_some() => EmptyState::new(t!("git.diff.failed"))
                .icon(Icon::new(IconName::CircleAlert))
                .description(summary.error.clone().unwrap_or_default())
                .into_any_element(),
            (Some(summary), _) if summary.hunks == 0 => EmptyState::new(t!("git.diff.no_changes"))
                .description(t!("git.diff.no_changes_text"))
                .into_any_element(),
            (Some(_), Some(view)) => Diff::new(&view.state)
                .header_visible(false)
                .hover_highlight(DiffHoverHighlight::Line)
                .hunk_separator(DiffHunkSeparator::Simple)
                .size_full()
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(palette.bg)
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn render_diff_header(
        &self,
        tab: &DiffTab,
        chosen: GitDiffMode,
        narrow: bool,
        palette: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let mono = cx.theme().mono_font_family.clone();
        let summary = tab.summary.clone().unwrap_or_default();
        let current = self
            .git_tasks
            .diff_view
            .as_ref()
            .map_or(0, |view| view.current);

        let file = h_flex()
            .min_w_0()
            .items_center()
            .gap(px(8.0))
            .child(
                h_flex()
                    .min_w_0()
                    .font_family(mono.clone())
                    .text_size(px(12.5))
                    .child(
                        div()
                            .text_color(palette.fg_subtle)
                            .child(tab.folder().to_string()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_color(palette.fg)
                            .child(tab.label().to_string()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.5))
                    .text_color(super::panel::letter_color(tab.letter, palette))
                    .child(status_label(tab.letter, &tab.target)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(6.0))
                    .font_family(mono)
                    .text_size(px(11.5))
                    .child(
                        div()
                            .text_color(palette.success)
                            .child(format!("+{}", format_integer(summary.additions as u64))),
                    )
                    .child(div().text_color(palette.danger).child(format!(
                        "\u{2212}{}",
                        format_integer(summary.deletions as u64)
                    ))),
            );

        let navigation = (summary.hunks > 0).then(|| {
            let (previous, next) = (weak.clone(), weak.clone());
            h_flex()
                .flex_none()
                .items_center()
                .gap(px(2.0))
                .child(
                    IconButton::new("git-diff-previous", IconName::ArrowUp)
                        .tooltip(t!("git.diff.previous"))
                        .on_click(move |_, _, cx| {
                            let _ = previous.update(cx, |view, cx| view.step_git_diff(false, cx));
                        }),
                )
                .child(
                    IconButton::new("git-diff-next", IconName::ArrowDown)
                        .tooltip(t!("git.diff.next"))
                        .on_click(move |_, _, cx| {
                            let _ = next.update(cx, |view, cx| view.step_git_diff(true, cx));
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(palette.fg_subtle)
                        .child(t!(
                            "git.diff.position",
                            current = format_integer((current + 1) as u64),
                            total = format_integer(summary.hunks as u64)
                        )),
                )
        });

        let mut modes = SegmentedControl::new("git-diff-mode");
        for (mode, label) in [
            (GitDiffMode::Split, t!("git.diff.split")),
            (GitDiffMode::Unified, t!("git.diff.unified")),
        ] {
            let weak = weak.clone();
            // Narrow, the split choice is kept but Unified is what shows.
            let selected = if narrow {
                mode == GitDiffMode::Unified
            } else {
                mode == chosen
            };
            modes = modes.item(SegmentedItem::new(label).selected(selected).on_click(
                move |window, cx| {
                    let _ = weak.update(cx, |view, cx| view.set_git_diff_mode(mode, window, cx));
                },
            ));
        }

        let open_request = tab.request_id().filter(|_| tab.letter != 'D').map(|id| {
            let id = id.to_string();
            let weak = weak.clone();
            SecondaryButton::new("git-diff-open-request", t!("git.diff.open_request")).on_click(
                move |_, _, cx| {
                    let id = id.clone();
                    let _ = weak.update(cx, |view, cx| view.open_request(id, cx));
                },
            )
        });

        h_flex()
            .flex_none()
            .h(px(44.0))
            .px(px(14.0))
            .gap(px(14.0))
            .items_center()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.surface)
            .child(div().flex_1().min_w_0().child(file))
            .children(navigation)
            .child(modes)
            .children(open_request)
            .into_any_element()
    }
}

/// The viewer layout of a [`GitDiffMode`].
fn to_diff_mode(mode: GitDiffMode) -> DiffMode {
    match mode {
        GitDiffMode::Split => DiffMode::Split,
        GitDiffMode::Unified => DiffMode::Unified,
    }
}

/// The header's word for a status letter.
fn status_label(letter: char, target: &DiffTarget) -> String {
    let key = match letter {
        'A' => "git.diff.status_added",
        'D' => "git.diff.status_deleted",
        'R' => "git.diff.status_renamed",
        'U' => "git.diff.status_untracked",
        '!' => "git.diff.status_conflict",
        _ => "git.diff.status_modified",
    };
    match target {
        DiffTarget::WorkTree => t!(key).into_owned(),
    }
}
