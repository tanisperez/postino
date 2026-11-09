//! The diff tab (designs 2a, 2b, 7a): a header with the file, its kind of change, the change
//! counter with Prev and Next, the layout switch and "Open file", the column titles (`HEAD` and
//! its commit, the working tree) and the lines, side by side or unified, with syntax colors, the
//! changed words marked and the Revert and Stage buttons of each block of changes. The patch is
//! read, parsed and highlighted on the background executor whenever the tab opens a file or the
//! git status changes, never in render; the lines are a virtualized list.
//!
//! Its keys work like IntelliJ's (#224), only while the tab has the focus: F7 and Shift+F7 step
//! through the changes and, past the last (or first) one, to the next (or previous) file, and F4
//! opens the file at the current change.

use gpui_kit::assets::IconName;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::Button;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_core::Method;
use postino_format::highlight::TokenKind;
use postino_format::{DiffRowKind, RequestDiff, RequestPart, RowNote, diff_requests};
use rust_i18n::t;
use telaio_git::{ApplyTarget, DiffSource, FileVersion, LineKind, Repo, block_patch};

use crate::actions::{NextDiffChange, OpenDiffFile, PreviousDiffChange};
use crate::state::diff_view::DiffModel;
use crate::state::git::{ChangeRowKind, change_rows};
use crate::state::git_diff::{
    DiffSummary, DiffTab, DiffTarget, HunkStage, block_at_row, block_new_line, neighbour,
    request_location, summarize,
};
use crate::state::git_log::Detail;
use crate::state::locale::plural;
use crate::state::number::format_integer;
use crate::state::settings::GitDiffMode;
use crate::theme::metrics::{RAIL_WIDTH, SIDEBAR_WIDTH};
use crate::theme::{Palette, PaletteExt, PostinoColors};
use crate::views::components::{
    ConfirmDialog, EmptyState, NeutralButtonExt, SecondaryButton, SegmentedControl, SegmentedItem,
    open_confirm_dialog,
};

use super::super::root::AppView;

/// Below this width of the main area the diff is unified, whatever the setting says: two columns
/// of code would be too narrow to read (design 7a).
const SPLIT_MIN_WIDTH: f32 = 720.0;

/// Above this many change blocks the per block buttons are not offered: finding whether each one
/// is staged runs git twice per block.
const MAX_BLOCK_ACTIONS: usize = 100;

/// The height of a line of the diff (design 7a).
const LINE_HEIGHT: f32 = 20.0;
/// The width of a line number column (design 7a).
const GUTTER_WIDTH: f32 = 44.0;
/// The group that shows a row's block buttons while the pointer is over it.
const ROW_GROUP: &str = "git-diff-row";

/// The key context of the diff tab, where `main.rs` binds F7, Shift+F7 and F4.
pub(crate) const DIFF_KEY_CONTEXT: &str = "GitDiff";

/// How long the "press F7 again" hint stays.
const EDGE_HINT_DURATION: Duration = Duration::from_secs(3);

/// A patch read in the background, ready for the viewer.
struct LoadedDiff {
    summary: DiffSummary,
    model: Arc<DiffModel>,
    patch: Arc<str>,
    /// Per change block, whether it is staged; `None` for a patch that is not the work tree's
    /// (no buttons).
    blocks: Vec<Option<HunkStage>>,
    semantic: Option<Result<RequestDiff, String>>,
}

impl LoadedDiff {
    /// Reads `repo_path`'s patch from `source` with the whole file as context, parses and
    /// highlights it, and for a work tree diff finds out whether each change block is staged.
    fn read(
        repo: &Repo,
        repo_path: &str,
        source: &DiffSource,
    ) -> Result<Self, telaio_git::GitError> {
        let patch = repo.diff_full(repo_path, source)?;
        let model = DiffModel::build(&patch, repo_path);
        let summary = summarize(&model.diff);
        let with_actions = matches!(source, DiffSource::HeadToWorkTree)
            && model.diff.blocks.len() <= MAX_BLOCK_ACTIONS;
        let blocks = (0..model.diff.blocks.len())
            .map(|index| {
                with_actions
                    .then(|| block_patch(&patch, &model.diff, index))
                    .flatten()
                    .map(|block| hunk_stage(repo, &block))
            })
            .collect();
        let semantic = repo_path
            .ends_with(".postino")
            .then(|| semantic_diff(repo, repo_path, source));
        Ok(LoadedDiff {
            summary,
            model,
            patch: patch.into(),
            blocks,
            semantic,
        })
    }
}

/// The request diff of `repo_path` between `HEAD` and the file on disk, or between a commit's
/// parent and the commit, or why it cannot be made (a side that does not parse: the text modes
/// still work).
fn semantic_diff(repo: &Repo, repo_path: &str, source: &DiffSource) -> Result<RequestDiff, String> {
    let (old, new) = match source {
        DiffSource::Commit(oid) => {
            // A root commit has no parent: the file is new.
            let old = repo
                .file_at(repo_path, &FileVersion::Commit(format!("{oid}^")))
                .unwrap_or(None);
            let new = repo
                .file_at(repo_path, &FileVersion::Commit(oid.clone()))
                .map_err(|error| error.to_string())?;
            (old, new)
        }
        _ => {
            let old = repo
                .file_at(repo_path, &FileVersion::Head)
                .map_err(|error| error.to_string())?;
            (
                old,
                std::fs::read_to_string(repo.root().join(repo_path)).ok(),
            )
        }
    };
    diff_requests(old.as_deref(), new.as_deref()).map_err(|error| error.to_string())
}

/// Whether `block` (from HEAD to the work tree) is in the index: it applies to the index forward
/// when it is not, backward when it is.
fn hunk_stage(repo: &Repo, block: &str) -> HunkStage {
    if repo
        .can_apply(block, ApplyTarget::Index, false)
        .unwrap_or(false)
    {
        HunkStage::Unstaged
    } else if repo
        .can_apply(block, ApplyTarget::Index, true)
        .unwrap_or(false)
    {
        HunkStage::Staged
    } else {
        HunkStage::Mixed
    }
}

/// The live viewer of the diff tab.
pub(crate) struct DiffView {
    /// The tab generation the viewer shows.
    generation: u64,
    /// The change block the header counts from, 0 based.
    current: usize,
    /// The patch shown, to keep the scroll position when a reread finds nothing new.
    patch: Arc<str>,
    /// The parsed and highlighted lines.
    model: Arc<DiffModel>,
    /// Whether each change block is staged, for its buttons.
    blocks: Rc<Vec<Option<HunkStage>>>,
    /// The request diff of a `.postino` file, `None` for other files.
    semantic: Option<Rc<Result<RequestDiff, String>>>,
    /// The scroll position of the lines.
    scroll: UniformListScrollHandle,
    /// Scroll to the current change on the next render, where the layout is known: a new patch
    /// was read.
    scroll_pending: bool,
    /// The direction of the "press F7 again to go to the next file" hint while it shows: `true`
    /// past the last change, `false` before the first one.
    edge_hint: Option<bool>,
}

impl DiffView {
    /// The list row where change block `block` starts in the `unified` or side by side layout.
    fn block_row(&self, block: usize, unified: bool) -> Option<usize> {
        let block = self.model.diff.blocks.get(block)?;
        Some(if unified {
            block.first_line
        } else {
            block.first_split_row
        })
    }
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
        if self.git_tasks.diff_focus.is_none() {
            self.git_tasks.diff_focus = Some(cx.focus_handle());
        }
        self.git_tasks.focus_diff = true;
        if let Some(view) = self.git_tasks.diff_view.as_mut() {
            view.edge_hint = None;
        }
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
            DiffTarget::Commit { oid, .. } => DiffSource::Commit(oid.clone()),
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
        result: Result<LoadedDiff, telaio_git::GitError>,
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
                    model: Arc::default(),
                    patch: Arc::from(""),
                    blocks: Vec::new(),
                    semantic: None,
                }
            }
        };
        tab.summary = Some(loaded.summary);
        let blocks = Rc::new(loaded.blocks);
        let semantic = loaded.semantic.map(Rc::new);
        // A new file starts on its first change, or on its last one after Shift+F7.
        let first = if std::mem::take(&mut self.git_tasks.diff_land_last) {
            loaded.model.diff.blocks.len().saturating_sub(1)
        } else {
            0
        };
        match &mut self.git_tasks.diff_view {
            Some(view) => {
                if view.patch != loaded.patch {
                    view.current = first;
                    view.scroll = UniformListScrollHandle::new();
                    view.scroll_pending = true;
                }
                view.generation = generation;
                view.patch = loaded.patch;
                view.model = loaded.model;
                view.blocks = blocks;
                view.semantic = semantic;
            }
            None => {
                self.git_tasks.diff_view = Some(DiffView {
                    generation,
                    current: first,
                    patch: loaded.patch,
                    model: loaded.model,
                    blocks,
                    semantic,
                    scroll: UniformListScrollHandle::new(),
                    scroll_pending: true,
                    edge_hint: None,
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
        if let Some(view) = self.git_tasks.diff_view.as_ref()
            && let Some(row) = view.block_row(view.current, mode == GitDiffMode::Unified)
        {
            view.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
    }

    /// The layout the diff tab shows: the chosen one, unless the main area is too narrow for two
    /// columns or the file has no request diff.
    fn shown_git_diff_mode(&self, window: &Window) -> GitDiffMode {
        // The main area's width, without measuring a frame: the window less the rail and the
        // sidebar when it shows.
        let sidebar = if self.nav.sidebar_visible() {
            SIDEBAR_WIDTH
        } else {
            0.0
        };
        let narrow =
            f32::from(window.viewport_size().width) - RAIL_WIDTH - sidebar < SPLIT_MIN_WIDTH;
        let semantic = self
            .git_tasks
            .diff_view
            .as_ref()
            .and_then(|view| view.semantic.as_deref());
        shown_mode(self.state.settings.git_diff_mode, narrow, semantic)
    }

    /// Moves to the next (or previous) change block of the diff, in the layout that shows. Past
    /// the last (or before the first) one, a first press shows a hint and a second one opens the
    /// next (or previous) file's diff.
    fn step_git_diff(&mut self, forward: bool, window: &Window, cx: &mut Context<Self>) {
        let unified = match self.shown_git_diff_mode(window) {
            GitDiffMode::Request => return,
            mode => mode == GitDiffMode::Unified,
        };
        let has_neighbour = self.neighbour_diff_file(forward).is_some();
        let Some(view) = self.git_tasks.diff_view.as_mut() else {
            return;
        };
        let count = view.model.diff.blocks.len();
        let at_edge = count == 0
            || if forward {
                view.current + 1 >= count
            } else {
                view.current == 0
            };
        if !at_edge {
            view.edge_hint = None;
            view.current = if forward {
                view.current + 1
            } else {
                view.current - 1
            };
            if let Some(row) = view.block_row(view.current, unified) {
                view.scroll.scroll_to_item(row, ScrollStrategy::Center);
            }
            cx.notify();
            return;
        }
        if !has_neighbour {
            return;
        }
        if view.edge_hint == Some(forward) {
            view.edge_hint = None;
            self.open_neighbour_diff(forward, cx);
            return;
        }
        view.edge_hint = Some(forward);
        self.git_tasks.diff_hint = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(EDGE_HINT_DURATION).await;
            let _ = this.update(cx, |view, cx| {
                if let Some(diff) = view.git_tasks.diff_view.as_mut() {
                    diff.edge_hint = None;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Makes the change block that list row `row` is part of the current one, so the counter,
    /// F7 and F4 start from where the user clicked.
    fn select_git_diff_row(&mut self, row: usize, unified: bool, cx: &mut Context<Self>) {
        let Some(view) = self.git_tasks.diff_view.as_mut() else {
            return;
        };
        if let Some(block) = block_at_row(&view.model.diff, row, unified)
            && block != view.current
        {
            view.current = block;
            view.edge_hint = None;
            cx.notify();
        }
    }

    /// The file before or after the diff tab's one: in the Source Control tree for the work tree,
    /// in the selected commit's file list for a commit. As `(repo_path, path, letter)`.
    fn neighbour_diff_file(&self, forward: bool) -> Option<(String, String, char)> {
        let tab = self.diff_tab()?;
        let files: Vec<(String, String, char)> = match &tab.target {
            DiffTarget::WorkTree => {
                let (repo, status) = self.git.status()?;
                // Every file in tree order, folded folders included.
                change_rows(status, repo.prefix(), &HashSet::new(), self.git.flat)
                    .into_iter()
                    .filter_map(|row| match row.kind {
                        ChangeRowKind::File { letter, .. } => {
                            Some((row.repo_paths.first()?.clone(), row.path, letter))
                        }
                        _ => None,
                    })
                    .collect()
            }
            DiffTarget::Commit { oid, .. } => {
                let Detail::Ready(detail) = &self.log_tab()?.detail else {
                    return None;
                };
                if detail.oid != *oid {
                    return None;
                }
                detail
                    .files
                    .iter()
                    .map(|file| (file.repo_path.clone(), file.path.clone(), file.letter))
                    .collect()
            }
        };
        let index = files.iter().position(|file| file.0 == tab.repo_path)?;
        neighbour(&files, index, forward).cloned()
    }

    /// Opens the diff of the file after (or before) the diff tab's one, on its first (or last)
    /// change.
    fn open_neighbour_diff(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some((repo_path, path, letter)) = self.neighbour_diff_file(forward) else {
            return;
        };
        let Some(target) = self.diff_tab().map(|tab| tab.target.clone()) else {
            return;
        };
        self.git_tasks.diff_land_last = !forward;
        self.open_git_diff(repo_path, path, target, letter, cx);
    }

    /// Called when the main area shows another tab: the diff tab takes the focus again when it
    /// is back.
    pub(crate) fn note_diff_tab_hidden(&mut self) {
        self.git_tasks.diff_shown = false;
    }

    /// The open diff tab.
    fn diff_tab(&self) -> Option<&DiffTab> {
        let index = self.state.tabs.index_of(crate::state::git_diff::TAB_ID)?;
        self.state.tabs.open_tabs().get(index)?.diff()
    }

    /// Opens the diff tab's file (F4, "Open file"): a request on the tab of its current change,
    /// with the cursor on the changed line in a code editor; an environment on its tab; any
    /// other file in the system's application. Nothing for a deleted file.
    fn open_diff_file(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.diff_tab().cloned() else {
            return;
        };
        if tab.letter == 'D' {
            return;
        }
        if let Some(id) = tab.request_id() {
            let location = self.git_tasks.diff_view.as_ref().and_then(|view| {
                let line = block_new_line(&view.model.diff, view.current)?;
                let lines = view
                    .model
                    .diff
                    .lines
                    .iter()
                    .filter(|line| line.kind != LineKind::Removed)
                    .map(|line| line.text.as_str());
                Some(request_location(lines, line))
            });
            self.open_request(id.to_string(), cx);
            if let Some(location) = location
                && let Some(request_tab) = location.tab
            {
                self.active_request_tab = request_tab;
                self.request_editor.reveal_line = location.row.map(|row| (id.to_string(), row));
            }
        } else if let Some(name) = tab.environment_name() {
            self.open_environment_tab(name.to_string(), cx);
        } else if let Some(repo) = self.git.repo() {
            cx.open_with_system(&repo.root().join(&tab.repo_path));
        }
    }

    /// The patch of change block `index` of the diff tab, with the repository it applies to.
    fn git_block(&self, index: usize) -> Option<(Repo, String, Option<HunkStage>)> {
        let repo = self.git.repo()?.clone();
        let view = self.git_tasks.diff_view.as_ref()?;
        let stage = *view.blocks.get(index)?;
        Some((
            repo,
            block_patch(&view.patch, &view.model.diff, index)?,
            stage,
        ))
    }

    /// Stages change block `index` of the diff tab, or unstages it when `reverse`.
    fn stage_git_block(&mut self, index: usize, reverse: bool, cx: &mut Context<Self>) {
        let Some((repo, block, _)) = self.git_block(index) else {
            return;
        };
        self.run_git_write(cx, move || repo.apply(&block, ApplyTarget::Index, reverse));
    }

    /// Puts change block `index` of the diff tab back as it was in the last commit, in the work
    /// tree and, when it is staged, in the index too. Asks first when the setting says so.
    fn revert_git_block(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((repo, block, stage)) = self.git_block(index) else {
            return;
        };
        let target = if stage == Some(HunkStage::Staged) {
            ApplyTarget::Both
        } else {
            ApplyTarget::WorkTree
        };
        if !self.state.settings.git_confirm_discard {
            self.run_git_work_tree_write(cx, move || repo.apply(&block, target, true));
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
                    let (repo, block) = (repo.clone(), block.clone());
                    let _ = weak.update(cx, |view, cx| {
                        view.run_git_work_tree_write(cx, move || repo.apply(&block, target, true));
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
        let semantic = self
            .git_tasks
            .diff_view
            .as_ref()
            .and_then(|view| view.semantic.clone());
        let shows = self.shown_git_diff_mode(window);
        let focus = self
            .git_tasks
            .diff_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // It takes the focus when it opens a file and when it becomes the active tab, so its
        // keys work right away.
        let shown_before = std::mem::replace(&mut self.git_tasks.diff_shown, true);
        if std::mem::take(&mut self.git_tasks.focus_diff) || !shown_before {
            window.focus(&focus, cx);
        }
        if let Some(view) = self.git_tasks.diff_view.as_mut()
            && shows != GitDiffMode::Request
            && std::mem::take(&mut view.scroll_pending)
            && let Some(row) = view.block_row(view.current, shows == GitDiffMode::Unified)
        {
            view.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }

        let header = self.render_diff_header(&tab, shows, semantic.is_some(), &palette, cx);
        let body = match (&tab.summary, &self.git_tasks.diff_view) {
            (None, _) | (_, None) => div().flex_1().into_any_element(),
            (Some(summary), _) if summary.error.is_some() => EmptyState::new(t!("git.diff.failed"))
                .icon(Icon::new(IconName::CircleAlert))
                .description(summary.error.clone().unwrap_or_default())
                .into_any_element(),
            (Some(_), Some(view)) if view.model.diff.binary => {
                EmptyState::new(t!("git.diff.binary"))
                    .icon(Icon::new(IconName::FileDiff))
                    .description(t!("git.diff.binary_text"))
                    .into_any_element()
            }
            (Some(summary), _) if summary.blocks == 0 => EmptyState::new(t!("git.diff.no_changes"))
                .description(t!("git.diff.no_changes_text"))
                .into_any_element(),
            (Some(_), Some(_)) if shows == GitDiffMode::Request => match semantic.as_deref() {
                Some(Ok(diff)) => render_request_diff(diff, &palette, cx),
                _ => div().into_any_element(),
            },
            (Some(_), Some(view)) => {
                let titles = match &tab.target {
                    DiffTarget::WorkTree => ColumnTitles::WorkTree(
                        self.git
                            .status()
                            .and_then(|(_, status)| status.head_oid.clone()),
                    ),
                    DiffTarget::Commit { short, .. } => ColumnTitles::Commit(short.clone()),
                };
                v_flex()
                    .size_full()
                    .child(render_column_titles(
                        titles,
                        shows == GitDiffMode::Unified,
                        &palette,
                        cx,
                    ))
                    .child(self.render_diff_lines(
                        view,
                        shows == GitDiffMode::Unified,
                        &palette,
                        cx,
                    ))
                    .into_any_element()
            }
        };
        // A click anywhere in the tab focuses it (gpui focuses a tracked handle on mouse down).
        v_flex()
            .size_full()
            .track_focus(&focus)
            .key_context(DIFF_KEY_CONTEXT)
            .on_action(cx.listener(|view, _: &NextDiffChange, window, cx| {
                view.step_git_diff(true, window, cx);
            }))
            .on_action(cx.listener(|view, _: &PreviousDiffChange, window, cx| {
                view.step_git_diff(false, window, cx);
            }))
            .on_action(cx.listener(|view, _: &OpenDiffFile, _, cx| {
                view.open_diff_file(cx);
            }))
            .bg(palette.bg)
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// The virtualized lines of the diff. Each row is built from the model only: no parsing.
    fn render_diff_lines(
        &self,
        view: &DiffView,
        unified: bool,
        palette: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let rows = RowContext {
            unified,
            model: view.model.clone(),
            blocks: view.blocks.clone(),
            palette: palette.clone(),
            ui_font: cx.theme().font_family.clone(),
            weak: cx.weak_entity(),
        };
        let count = if unified {
            view.model.diff.lines.len()
        } else {
            view.model.diff.split.len()
        };
        uniform_list("git-diff-lines", count, move |range, window, _| {
            // The filler stripes are drawn in device pixels, see [`filler`].
            let scale = window.scale_factor();
            range
                .map(|index| {
                    if unified {
                        rows.unified_row(index)
                    } else {
                        rows.split_row(index, scale)
                    }
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&view.scroll)
        .size_full()
        .pt(px(6.0))
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(12.5))
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

        let edge_hint = self
            .git_tasks
            .diff_view
            .as_ref()
            .and_then(|view| view.edge_hint);
        let navigation = (summary.blocks > 0 && shows != GitDiffMode::Request).then(|| {
            let step = |id: &'static str,
                        icon: IconName,
                        label: String,
                        tooltip: String,
                        action: &dyn Action,
                        forward: bool| {
                let weak = weak.clone();
                Button::new(id)
                    .neutral_ghost(cx, palette.fg_muted)
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(0.0))
                    .icon(Icon::new(icon).size(px(14.0)))
                    .label(label)
                    .tooltip_with_action(tooltip, action, Some(DIFF_KEY_CONTEXT))
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| view.step_git_diff(forward, window, cx));
                    })
            };
            // Deferred, so it paints over the column titles below the header.
            let hint = edge_hint.map(|forward| {
                let text = if forward {
                    t!("git.diff.next_file_hint", key = "F7")
                } else {
                    t!("git.diff.previous_file_hint", key = "Shift+F7")
                };
                deferred(
                    div()
                        .absolute()
                        .top(px(34.0))
                        .left_0()
                        .px(px(8.0))
                        .py(px(5.0))
                        .rounded(px(6.0))
                        .bg(palette.raised)
                        .border_1()
                        .border_color(palette.border_strong)
                        .shadow_md()
                        .text_size(px(12.0))
                        .text_color(palette.fg)
                        .whitespace_nowrap()
                        .child(text.into_owned()),
                )
                .with_priority(1)
            });
            h_flex()
                .relative()
                .flex_none()
                .items_center()
                .gap(px(8.0))
                .child(
                    h_flex()
                        .h(px(28.0))
                        .items_center()
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette.border_strong)
                        .bg(palette.raised)
                        .overflow_hidden()
                        .child(step(
                            "git-diff-previous",
                            IconName::ChevronUp,
                            t!("git.diff.prev_button").into_owned(),
                            t!("git.diff.previous").into_owned(),
                            &PreviousDiffChange,
                            false,
                        ))
                        .child(div().w(px(1.0)).h_full().bg(palette.border_strong))
                        .child(step(
                            "git-diff-next",
                            IconName::ChevronDown,
                            t!("git.diff.next_button").into_owned(),
                            t!("git.diff.next").into_owned(),
                            &NextDiffChange,
                            true,
                        )),
                )
                .child(
                    h_flex()
                        .gap(px(4.0))
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .whitespace_nowrap()
                        .child(
                            div()
                                .text_color(palette.fg)
                                .child(format_integer((current + 1) as u64)),
                        )
                        .child(plural("git.diff.of_changes", summary.blocks)),
                )
                .children(hint)
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

        let open_file = (tab.letter != 'D').then(|| {
            let weak = weak.clone();
            SecondaryButton::new("git-diff-open-file", t!("git.diff.open_file"))
                .icon(Icon::new(IconName::ExternalLink).size(px(12.0)))
                .key_hint("F4")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |view, cx| view.open_diff_file(cx));
                })
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
            .children(open_file)
            .into_any_element()
    }
}

/// What the two columns compare.
enum ColumnTitles {
    /// `HEAD` (with its short id when known) against the working tree.
    WorkTree(Option<String>),
    /// A commit, by short id, against its parent.
    Commit(String),
}

/// The titles over the lines: `HEAD` and its short commit id on the old side, the working tree on
/// the new one; for a commit, its parent and the commit. Unified, both on one line.
fn render_column_titles(
    titles: ColumnTitles,
    unified: bool,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let mono = cx.theme().mono_font_family.clone();
    let (old, new) = match titles {
        ColumnTitles::WorkTree(head) => (
            h_flex()
                .gap(px(6.0))
                .child(div().text_color(palette.fg_muted).child("HEAD"))
                .children(head.map(|oid| div().font_family(mono.clone()).child(oid))),
            h_flex()
                .text_color(palette.fg_muted)
                .child(t!("git.diff.working_tree")),
        ),
        ColumnTitles::Commit(short) => (
            h_flex()
                .gap(px(6.0))
                .child(
                    div()
                        .text_color(palette.fg_muted)
                        .child(t!("git.diff.parent")),
                )
                .child(div().font_family(mono.clone()).child(format!("{short}^"))),
            h_flex()
                .gap(px(6.0))
                .child(
                    div()
                        .text_color(palette.fg_muted)
                        .child(t!("git.diff.commit")),
                )
                .child(div().font_family(mono).child(short)),
        ),
    };
    let bar = h_flex()
        .flex_none()
        .h(px(28.0))
        .border_b_1()
        .border_color(palette.border)
        .text_size(px(12.0))
        .text_color(palette.fg_subtle);
    let cell = || h_flex().h_full().px(px(14.0)).items_center();
    if unified {
        bar.child(
            cell()
                .gap(px(8.0))
                .child(old)
                .child(Icon::new(IconName::ArrowRight).size(px(12.0)))
                .child(new),
        )
        .into_any_element()
    } else {
        bar.child(cell().flex_1().min_w_0().child(old))
            .child(
                cell()
                    .flex_1()
                    .min_w_0()
                    .border_l_1()
                    .border_color(palette.border)
                    .child(new),
            )
            .into_any_element()
    }
}

/// What every row of the list reads, cloned once per frame into the list's closure.
struct RowContext {
    /// Whether the rows are the unified layout's.
    unified: bool,
    model: Arc<DiffModel>,
    blocks: Rc<Vec<Option<HunkStage>>>,
    palette: Palette,
    /// The interface font, for the block buttons over the monospaced lines.
    ui_font: SharedString,
    weak: WeakEntity<AppView>,
}

impl RowContext {
    /// Row `index` of the side by side layout: the old line (or a filler) and the new one.
    fn split_row(&self, index: usize, scale: f32) -> AnyElement {
        let row = self.model.diff.split[index];
        let block = self.block_starting_at(index, false);
        self.row(index)
            .child(self.side(index, scale, row.left, false, None))
            .child(self.side(index, scale, row.right, true, block))
            .into_any_element()
    }

    /// Row `index` of the unified layout: both line numbers and the text.
    fn unified_row(&self, index: usize) -> AnyElement {
        let line = &self.model.diff.lines[index];
        let palette = &self.palette;
        let tone = Tone::of(line.kind, palette);
        let number = |number: Option<u32>| gutter(number, &tone, palette);
        self.row(index)
            .child(number(line.old_number))
            .child(number(line.new_number))
            .child(self.content(Some(index), &tone, self.block_starting_at(index, true)))
            .into_any_element()
    }

    /// The frame of list row `index`. A click on it makes its change block the current one.
    fn row(&self, index: usize) -> Stateful<Div> {
        let (weak, unified) = (self.weak.clone(), self.unified);
        h_flex()
            .id(("git-diff-row", index))
            .group(ROW_GROUP)
            .h(px(LINE_HEIGHT))
            .w_full()
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |view, cx| view.select_git_diff_row(index, unified, cx));
            })
    }

    /// One half of side by side row `row`: the line number and the text of `line` (`new` picks
    /// the number of the new file), or an empty filler. `scale` is the window's, for the filler.
    fn side(
        &self,
        row: usize,
        scale: f32,
        line: Option<usize>,
        new: bool,
        block: Option<usize>,
    ) -> AnyElement {
        let palette = &self.palette;
        let diff_line = line.map(|index| &self.model.diff.lines[index]);
        let tone = match diff_line {
            Some(diff_line) => Tone::of(diff_line.kind, palette),
            // A filler's gutter is plain; its text area is the striped [`filler`].
            None => Tone::of(LineKind::Context, palette),
        };
        let number = diff_line.and_then(|diff_line| {
            if new {
                diff_line.new_number
            } else {
                diff_line.old_number
            }
        });
        h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .when(new, |this| this.border_l_1().border_color(palette.border))
            .child(gutter(number, &tone, palette))
            .child(match line {
                Some(_) => self.content(line, &tone, block),
                None => filler(row, scale, palette),
            })
            .into_any_element()
    }

    /// The text of `line` with its syntax colors and changed words, and the buttons of `block`
    /// when the block starts on this row.
    fn content(&self, line: Option<usize>, tone: &Tone, block: Option<usize>) -> AnyElement {
        let palette = &self.palette;
        let text = line.map(|index| {
            let text = self.model.texts[index].clone();
            let highlights: Vec<_> = self.model.runs[index]
                .iter()
                .filter(|run| run.kind.is_some() || run.emphasis)
                .map(|run| {
                    let color = run
                        .kind
                        .map(|kind| token_color(kind, &text[run.range.clone()], palette));
                    let style = HighlightStyle {
                        color,
                        background_color: run.emphasis.then_some(tone.emphasis),
                        ..HighlightStyle::default()
                    };
                    (run.range.clone(), style)
                })
                .collect();
            StyledText::new(text).with_highlights(highlights)
        });
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .pl(px(8.0))
            .bg(tone.background)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .line_height(px(LINE_HEIGHT))
            .text_color(palette.fg)
            .children(text)
            .children(block.and_then(|block| self.block_actions(block)))
            .into_any_element()
    }

    /// The change block that starts on list row `index`, if any.
    fn block_starting_at(&self, index: usize, unified: bool) -> Option<usize> {
        let start = |block: &telaio_git::ChangeBlock| {
            if unified {
                block.first_line
            } else {
                block.first_split_row
            }
        };
        self.model
            .diff
            .blocks
            .binary_search_by_key(&index, start)
            .ok()
    }

    /// The Revert and Stage (or Unstage) buttons of change block `block`, shown while the
    /// pointer is over its first row (design 7a). `None` when the block has no buttons.
    fn block_actions(&self, block: usize) -> Option<AnyElement> {
        let stage = (*self.blocks.get(block)?)?;
        let palette = &self.palette;
        let pill = |id: &'static str, icon: IconName, label: String| {
            h_flex()
                .id((id, block))
                .h_full()
                .px(px(5.0))
                .gap(px(4.0))
                .items_center()
                .rounded(px(4.0))
                .cursor_pointer()
                .child(Icon::new(icon).size(px(11.0)))
                .child(label)
        };
        let weak = self.weak.clone();
        let revert = pill(
            "git-block-revert",
            IconName::Undo2,
            t!("git.diff.revert_hunk").into_owned(),
        )
        .hover(|style| style.bg(palette.hover).text_color(palette.fg))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.revert_git_block(block, window, cx));
        });
        let stage_button = match stage {
            HunkStage::Unstaged => Some((IconName::Plus, t!("git.diff.stage_hunk"), false)),
            HunkStage::Staged => Some((IconName::Minus, t!("git.diff.unstage_hunk"), true)),
            HunkStage::Mixed => None,
        }
        .map(|(icon, label, reverse)| {
            let weak = self.weak.clone();
            pill("git-block-stage", icon, label.into_owned())
                .bg(palette.accent_subtle)
                .text_color(palette.accent_text)
                .hover(|style| style.bg(palette.list_active))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |view, cx| view.stage_git_block(block, reverse, cx));
                })
        });
        Some(
            h_flex()
                .invisible()
                .group_hover(ROW_GROUP, |style| style.visible())
                .absolute()
                .right(px(8.0))
                .top(px(1.0))
                .h(px(LINE_HEIGHT - 2.0))
                .px(px(2.0))
                .gap(px(2.0))
                .items_center()
                .rounded(px(5.0))
                .bg(palette.raised)
                .border_1()
                .border_color(palette.border_strong)
                .font_family(self.ui_font.clone())
                .text_size(px(11.0))
                .text_color(palette.fg_muted)
                .child(revert)
                .children(stage_button)
                .into_any_element(),
        )
    }
}

/// The colors of a line by its kind (design 7a).
struct Tone {
    background: Background,
    gutter: Hsla,
    number: Hsla,
    emphasis: Hsla,
}

impl Tone {
    fn of(kind: LineKind, palette: &Palette) -> Tone {
        let changed = |color: Hsla| Tone {
            background: color.opacity(0.09).into(),
            gutter: color.opacity(0.14),
            number: color,
            emphasis: color.opacity(0.30),
        };
        match kind {
            LineKind::Removed => changed(palette.danger),
            LineKind::Added => changed(palette.success),
            LineKind::Context => Tone {
                background: transparent_black().into(),
                gutter: transparent_black(),
                number: palette.fg_subtle,
                emphasis: transparent_black(),
            },
        }
    }
}

/// The distance between two filler stripes, across them, and the width of a stripe: the design's
/// `repeating-linear-gradient(135deg, transparent 0 5px, ... 5px 10px)`.
const FILLER_PERIOD: f32 = 10.0;
/// gpui's `pattern_slash` takes the stripe and the gap measured along the vertical and draws them
/// at 45 degrees, so both are the across width divided by sin 45.
const FILLER_PATTERN: f32 = FILLER_PERIOD / 2.0 / std::f32::consts::FRAC_1_SQRT_2;
/// How far apart the stripes are down a column: the across period divided by sin 45.
const FILLER_VERTICAL_PERIOD: f32 = FILLER_PERIOD / std::f32::consts::FRAC_1_SQRT_2;

/// The text area of the empty side of side by side row `row`: diagonal stripes. Two things of
/// gpui's shader shape it. It measures the pattern in device pixels, so its sizes are multiplied
/// by the window's `scale` to keep the design's 10 px at any display scale. And it starts the
/// pattern at each element's origin, so the stripes are painted on a layer pulled up by how far
/// the row is into the pattern, which lines consecutive filler rows up into one pattern instead
/// of restarting at every row.
fn filler(row: usize, scale: f32, palette: &Palette) -> AnyElement {
    let offset = (row as f32 * LINE_HEIGHT) % FILLER_VERTICAL_PERIOD;
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(-offset))
                .h(px(LINE_HEIGHT + offset))
                .bg(pattern_slash(
                    palette.fg.opacity(0.04),
                    FILLER_PATTERN * scale,
                    FILLER_PATTERN * scale,
                )),
        )
        .into_any_element()
}

/// A line number column.
fn gutter(number: Option<u32>, tone: &Tone, palette: &Palette) -> AnyElement {
    div()
        .flex_none()
        .w(px(GUTTER_WIDTH))
        .h_full()
        .pr(px(10.0))
        .text_right()
        .line_height(px(LINE_HEIGHT))
        .bg(tone.gutter)
        .text_color(if number.is_some() {
            tone.number
        } else {
            palette.fg_subtle
        })
        .children(number.map(|number| number.to_string()))
        .into_any_element()
}

/// The color of a highlighted piece of a line. `text` is the piece, for a method's color.
fn token_color(kind: TokenKind, text: &str, palette: &Palette) -> Hsla {
    match kind {
        TokenKind::Method => text
            .parse::<Method>()
            .map_or(palette.fg, |method| palette.method_color(&method)),
        TokenKind::Url | TokenKind::Punctuation => palette.fg,
        TokenKind::Variable => palette.accent_text,
        TokenKind::HeaderName => palette.syntax_color("function"),
        TokenKind::Section | TokenKind::Keyword => palette.syntax_color("keyword"),
        TokenKind::Comment | TokenKind::Disabled => palette.syntax_color("comment"),
        TokenKind::Key => palette.syntax_color("property"),
        TokenKind::String => palette.syntax_color("string"),
        TokenKind::Number => palette.syntax_color("number"),
        TokenKind::Boolean => palette.syntax_color("boolean"),
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
        DiffTarget::WorkTree | DiffTarget::Commit { .. } => t!(key).into_owned(),
    }
}
