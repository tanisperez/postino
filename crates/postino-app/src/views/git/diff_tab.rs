//! The diff tab (designs 2a, 2b, 7a): a header with the file, its kind of change, the change
//! counter with previous and next, the layout switch and "Open request", over gpui-component's
//! diff viewer. The patch is read and parsed on the background executor whenever the tab opens
//! a file or the git status changes, never in render.

use gpui_kit::assets::IconName;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::diff::{
    Diff, DiffAnnotation, DiffFile, DiffHoverHighlight, DiffHunkSeparator, DiffLinePosition,
    DiffMode, DiffSide, DiffState,
};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_format::{DiffRowKind, RequestDiff, RequestPart, RowNote, diff_requests};
use postino_git::{ApplyTarget, DiffSource, FileVersion, Repo, hunk_patch};
use rust_i18n::t;

use crate::state::git_diff::{
    DiffSummary, DiffTab, DiffTarget, HunkAnchor, HunkStage, hunk_anchors, summarize,
};
use crate::state::locale::plural;
use crate::state::number::format_integer;
use crate::state::settings::GitDiffMode;
use crate::theme::metrics::{RAIL_WIDTH, SIDEBAR_WIDTH};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{
    ConfirmDialog, EmptyState, GhostButton, IconButton, SecondaryButton, SegmentedControl,
    SegmentedItem, open_confirm_dialog,
};

use super::super::root::AppView;

/// Below this width of the main area the diff is unified, whatever the setting says: two columns
/// of code would be too narrow to read (design 7a).
const SPLIT_MIN_WIDTH: f32 = 720.0;

/// Above this many hunks the per hunk buttons are not offered: finding whether each one is
/// staged runs git twice per hunk.
const MAX_HUNK_ACTIONS: usize = 100;

/// One hunk of the shown patch, with what its buttons do.
#[derive(Clone)]
struct HunkInfo {
    anchor: HunkAnchor,
    /// `None` for a patch that is not the work tree's (no buttons).
    stage: Option<HunkStage>,
}

/// A patch read in the background, ready for the viewer.
struct LoadedDiff {
    summary: DiffSummary,
    files: Vec<DiffFile>,
    patch: Arc<str>,
    hunks: Vec<HunkInfo>,
    semantic: Option<Result<RequestDiff, String>>,
}

impl LoadedDiff {
    /// Reads `repo_path`'s patch from `source`, parses it, and for a work tree diff finds out
    /// whether each hunk is staged.
    fn read(
        repo: &Repo,
        repo_path: &str,
        source: &DiffSource,
    ) -> Result<Self, postino_git::GitError> {
        let patch = repo.diff(repo_path, source)?;
        let summary = summarize(&patch);
        let files = DiffFile::parse(&patch).unwrap_or_default();
        let anchors = hunk_anchors(&patch);
        let with_actions =
            matches!(source, DiffSource::HeadToWorkTree) && anchors.len() <= MAX_HUNK_ACTIONS;
        let hunks = anchors
            .into_iter()
            .enumerate()
            .map(|(index, anchor)| {
                let stage = with_actions
                    .then(|| hunk_patch(&patch, index))
                    .flatten()
                    .map(|hunk| hunk_stage(repo, &hunk));
                HunkInfo { anchor, stage }
            })
            .collect();
        let semantic = repo_path
            .ends_with(".postino")
            .then(|| semantic_diff(repo, repo_path));
        Ok(LoadedDiff {
            summary,
            files,
            patch: patch.into(),
            hunks,
            semantic,
        })
    }
}

/// The request diff of `repo_path` between `HEAD` and the file on disk, or why it cannot be
/// made (a side that does not parse: the text modes still work).
fn semantic_diff(repo: &Repo, repo_path: &str) -> Result<RequestDiff, String> {
    let old = repo
        .file_at(repo_path, &FileVersion::Head)
        .map_err(|error| error.to_string())?;
    let new = std::fs::read_to_string(repo.root().join(repo_path)).ok();
    diff_requests(old.as_deref(), new.as_deref()).map_err(|error| error.to_string())
}

/// Whether `hunk` (from HEAD to the work tree) is in the index: it applies to the index forward
/// when it is not, backward when it is.
fn hunk_stage(repo: &Repo, hunk: &str) -> HunkStage {
    if repo
        .can_apply(hunk, ApplyTarget::Index, false)
        .unwrap_or(false)
    {
        HunkStage::Unstaged
    } else if repo
        .can_apply(hunk, ApplyTarget::Index, true)
        .unwrap_or(false)
    {
        HunkStage::Staged
    } else {
        HunkStage::Mixed
    }
}

/// The live viewer of the diff tab.
pub(crate) struct DiffView {
    /// The viewer's state: files, layout, scroll.
    state: Entity<DiffState>,
    /// The tab generation the state shows.
    generation: u64,
    /// The change the header counts from, 0 based.
    current: usize,
    /// The patch shown, to skip reloading the viewer (and losing its scroll) when nothing
    /// changed.
    patch: Arc<str>,
    /// The file's path as the viewer knows it, for the hunk annotations.
    file_path: SharedString,
    /// The hunks, for their buttons.
    hunks: Rc<Vec<HunkInfo>>,
    /// The request diff of a `.postino` file, `None` for other files.
    semantic: Option<Rc<Result<RequestDiff, String>>>,
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
                .background_spawn(async move { LoadedDiff::read(&repo, &repo_path, &source) })
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
        result: Result<LoadedDiff, postino_git::GitError>,
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
        let loaded = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                log::warn!("git diff failed: {error}");
                LoadedDiff {
                    summary: DiffSummary {
                        error: Some(error.to_string()),
                        ..DiffSummary::default()
                    },
                    files: Vec::new(),
                    patch: Arc::from(""),
                    hunks: Vec::new(),
                    semantic: None,
                }
            }
        };
        tab.summary = Some(loaded.summary);
        let file_path = loaded
            .files
            .first()
            .map(|file| file.path().clone())
            .unwrap_or_default();
        let hunks = Rc::new(loaded.hunks);
        let semantic = loaded.semantic.map(Rc::new);
        let mode = to_diff_mode(self.state.settings.git_diff_mode);
        match &mut self.git_tasks.diff_view {
            Some(view) => {
                if view.patch != loaded.patch {
                    view.state
                        .update(cx, |state, cx| state.set_files(loaded.files, cx));
                    view.current = 0;
                }
                view.generation = generation;
                view.patch = loaded.patch;
                view.file_path = file_path;
                view.hunks = hunks;
                view.semantic = semantic;
            }
            None => {
                let state = cx.new(|cx| DiffState::new(loaded.files, cx).with_mode(mode));
                self.git_tasks.diff_view = Some(DiffView {
                    state,
                    generation,
                    current: 0,
                    patch: loaded.patch,
                    file_path,
                    hunks,
                    semantic,
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

    /// The patch of hunk `index` of the diff tab, with the repository it applies to.
    fn git_hunk(&self, index: usize) -> Option<(Repo, String, Option<HunkStage>)> {
        let repo = self.git.repo()?.clone();
        let view = self.git_tasks.diff_view.as_ref()?;
        let stage = view.hunks.get(index)?.stage;
        Some((repo, hunk_patch(&view.patch, index)?, stage))
    }

    /// Stages hunk `index` of the diff tab, or unstages it when `reverse`.
    fn stage_git_hunk(&mut self, index: usize, reverse: bool, cx: &mut Context<Self>) {
        let Some((repo, hunk, _)) = self.git_hunk(index) else {
            return;
        };
        self.run_git_write(cx, move || repo.apply(&hunk, ApplyTarget::Index, reverse));
    }

    /// Puts hunk `index` of the diff tab back as it was in the last commit, in the work tree and,
    /// when it is staged, in the index too. Asks first when the setting says so.
    fn revert_git_hunk(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((repo, hunk, stage)) = self.git_hunk(index) else {
            return;
        };
        let target = if stage == Some(HunkStage::Staged) {
            ApplyTarget::Both
        } else {
            ApplyTarget::WorkTree
        };
        if !self.state.settings.git_confirm_discard {
            self.run_git_write(cx, move || repo.apply(&hunk, target, true));
            return;
        }
        let weak = cx.weak_entity();
        open_confirm_dialog(
            window,
            cx,
            ConfirmDialog {
                title: t!("git.diff.revert_title").into(),
                text: t!("git.diff.revert_text").into(),
                confirm_label: t!("git.diff.revert_hunk").into(),
                destructive: true,
                show_cancel: true,
                on_confirm: Rc::new(move |_, cx| {
                    let (repo, hunk) = (repo.clone(), hunk.clone());
                    let _ = weak.update(cx, |view, cx| {
                        view.run_git_write(cx, move || repo.apply(&hunk, target, true));
                    });
                }),
            },
        );
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
        let semantic = self
            .git_tasks
            .diff_view
            .as_ref()
            .and_then(|view| view.semantic.clone());
        let shows = shown_mode(chosen, narrow, semantic.as_deref());
        let effective = if shows == GitDiffMode::Request {
            chosen
        } else {
            shows
        };
        if let Some(view) = &self.git_tasks.diff_view {
            let mode = to_diff_mode(effective);
            if view.state.read(cx).mode() != mode {
                view.state.update(cx, |state, cx| state.set_mode(mode, cx));
            }
        }

        let header = self.render_diff_header(&tab, shows, semantic.is_some(), &palette, cx);
        let body = match (&tab.summary, &self.git_tasks.diff_view) {
            (None, _) | (_, None) => div().flex_1().into_any_element(),
            (Some(summary), _) if summary.error.is_some() => EmptyState::new(t!("git.diff.failed"))
                .icon(Icon::new(IconName::CircleAlert))
                .description(summary.error.clone().unwrap_or_default())
                .into_any_element(),
            (Some(summary), _) if summary.hunks == 0 => EmptyState::new(t!("git.diff.no_changes"))
                .description(t!("git.diff.no_changes_text"))
                .into_any_element(),
            (Some(_), Some(_)) if shows == GitDiffMode::Request => match semantic.as_deref() {
                Some(Ok(diff)) => render_request_diff(diff, &palette, cx),
                _ => div().into_any_element(),
            },
            (Some(_), Some(view)) => Diff::new(&view.state)
                .annotations(hunk_annotations(view))
                .render_annotation({
                    let hunks = view.hunks.clone();
                    let weak = cx.weak_entity();
                    move |annotation, _, cx| render_hunk_actions(annotation, &hunks, &weak, cx)
                })
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
        shows: GitDiffMode,
        has_request_mode: bool,
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

        let navigation = (summary.hunks > 0 && shows != GitDiffMode::Request).then(|| {
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
        let request_mode = has_request_mode.then(|| (GitDiffMode::Request, t!("git.diff.request")));
        for (mode, label) in request_mode.into_iter().chain([
            (GitDiffMode::Split, t!("git.diff.split")),
            (GitDiffMode::Unified, t!("git.diff.unified")),
        ]) {
            let weak = weak.clone();
            // The mode that shows: narrow, a split choice is kept but Unified is what shows.
            let selected = mode == shows;
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

/// One annotation per hunk that has buttons, at its anchor line.
fn hunk_annotations(view: &DiffView) -> Vec<DiffAnnotation> {
    view.hunks
        .iter()
        .enumerate()
        .filter(|(_, hunk)| hunk.stage.is_some())
        .map(|(index, hunk)| {
            DiffAnnotation::line(
                ("git-hunk", index),
                DiffLinePosition::new(
                    view.file_path.clone(),
                    side_of(hunk.anchor),
                    hunk.anchor.line,
                ),
            )
        })
        .collect()
}

fn side_of(anchor: HunkAnchor) -> DiffSide {
    if anchor.modified_side {
        DiffSide::Modified
    } else {
        DiffSide::Original
    }
}

/// The Stage (or Unstage) and Revert buttons of the hunk an annotation stands for.
fn render_hunk_actions(
    annotation: &DiffAnnotation,
    hunks: &Rc<Vec<HunkInfo>>,
    weak: &WeakEntity<AppView>,
    cx: &App,
) -> AnyElement {
    let palette = cx.palette();
    let Some(position) = annotation.position() else {
        return div().into_any_element();
    };
    let Some((index, hunk)) = hunks.iter().enumerate().find(|(_, hunk)| {
        hunk.anchor.line == position.line() && side_of(hunk.anchor) == position.side()
    }) else {
        return div().into_any_element();
    };
    let stage = hunk.stage;
    let mut row = h_flex()
        .py(px(4.0))
        .px(px(8.0))
        .gap(px(6.0))
        .bg(palette.surface);
    let stage_label = match stage {
        Some(HunkStage::Unstaged) => Some((t!("git.diff.stage_hunk"), false)),
        Some(HunkStage::Staged) => Some((t!("git.diff.unstage_hunk"), true)),
        _ => None,
    };
    if let Some((label, reverse)) = stage_label {
        let weak = weak.clone();
        row = row.child(
            GhostButton::new(("git-hunk-stage", index), label)
                .height(24.0)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |view, cx| view.stage_git_hunk(index, reverse, cx));
                }),
        );
    }
    let weak = weak.clone();
    row = row.child(
        GhostButton::new(("git-hunk-revert", index), t!("git.diff.revert_hunk"))
            .height(24.0)
            .on_click(move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| view.revert_git_hunk(index, window, cx));
            }),
    );
    row.into_any_element()
}

/// The viewer layout of a [`GitDiffMode`].
fn to_diff_mode(mode: GitDiffMode) -> DiffMode {
    match mode {
        GitDiffMode::Split | GitDiffMode::Request => DiffMode::Split,
        GitDiffMode::Unified => DiffMode::Unified,
    }
}

/// The mode the tab shows for the `chosen` one: Request only for a request whose two versions
/// parse, and Unified when the area is too narrow for two columns.
fn shown_mode(
    chosen: GitDiffMode,
    narrow: bool,
    semantic: Option<&Result<RequestDiff, String>>,
) -> GitDiffMode {
    match chosen {
        GitDiffMode::Request if matches!(semantic, Some(Ok(_))) => GitDiffMode::Request,
        _ if narrow => GitDiffMode::Unified,
        GitDiffMode::Request => GitDiffMode::Split,
        mode => mode,
    }
}

/// The label of a part of a request.
fn part_label(part: RequestPart) -> String {
    match part {
        RequestPart::Request => t!("git.request.request"),
        RequestPart::Headers => t!("git.request.headers"),
        RequestPart::Query => t!("git.request.query"),
        RequestPart::Auth => t!("git.request.auth"),
        RequestPart::Body => t!("git.request.body"),
        RequestPart::PreScript => t!("git.request.pre_script"),
        RequestPart::PostScript => t!("git.request.post_script"),
        RequestPart::Docs => t!("git.request.docs"),
    }
    .into_owned()
}

/// The short note at the end of a row.
fn note_label(note: RowNote) -> String {
    match note {
        RowNote::NewHeader => t!("git.request.new_header"),
        RowNote::RemovedHeader => t!("git.request.removed_header"),
        RowNote::ChangedHeader => t!("git.request.changed_header"),
        RowNote::NewField => t!("git.request.new_field"),
        RowNote::RemovedField => t!("git.request.removed_field"),
        RowNote::ChangedField => t!("git.request.changed_field"),
    }
    .into_owned()
}

/// The Request mode (design 2c): one block per changed part, its rows, and the unchanged parts in
/// one line.
fn render_request_diff(diff: &RequestDiff, palette: &Palette, cx: &App) -> AnyElement {
    let mono = cx.theme().mono_font_family.clone();
    let mut column = v_flex().gap(px(14.0)).p(px(16.0));
    for part in &diff.changed {
        let mut counts = Vec::new();
        for (count, key) in [
            (part.summary.changed, "git.request.changed"),
            (part.summary.added, "git.request.added"),
            (part.summary.removed, "git.request.removed"),
        ] {
            if count > 0 {
                counts.push(plural(key, count));
            }
        }
        let rows = part.rows.iter().map(|row| {
            let (sign, color, background) = match row.kind {
                DiffRowKind::Added => ("+", palette.success, palette.success.opacity(0.09)),
                DiffRowKind::Removed => ("\u{2212}", palette.danger, palette.danger.opacity(0.09)),
                DiffRowKind::Unchanged => (" ", palette.fg_subtle, palette.bg.opacity(0.0)),
            };
            h_flex()
                .min_h(px(22.0))
                .px(px(10.0))
                .gap(px(10.0))
                .items_center()
                .bg(background)
                .font_family(mono.clone())
                .text_size(px(12.0))
                .child(div().w(px(10.0)).flex_none().text_color(color).child(sign))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(palette.fg)
                        .child(row.text.clone()),
                )
                .children(row.note.map(|note| {
                    div()
                        .flex_none()
                        .text_size(px(11.0))
                        .text_color(palette.fg_subtle)
                        .child(note_label(note))
                }))
        });
        column = column.child(
            v_flex()
                .rounded(px(8.0))
                .border_1()
                .border_color(palette.border)
                .overflow_hidden()
                .child(
                    h_flex()
                        .px(px(12.0))
                        .py(px(8.0))
                        .gap(px(8.0))
                        .bg(palette.surface)
                        .border_b_1()
                        .border_color(palette.border)
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(part_label(part.part)),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(palette.fg_subtle)
                                .child(counts.join(", ")),
                        ),
                )
                .child(v_flex().py(px(4.0)).children(rows)),
        );
    }
    if !diff.unchanged.is_empty() {
        let names: Vec<String> = diff
            .unchanged
            .iter()
            .map(|part| part_label(*part))
            .collect();
        column = column.child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(t!("git.request.unchanged", parts = names.join(", "))),
        );
    }
    div()
        .id("git-request-diff")
        .size_full()
        .overflow_y_scroll()
        .child(column)
        .into_any_element()
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
