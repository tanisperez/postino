//! The Source Control panel of the sidebar (design 2b): the workspace's changed files as a tree
//! with checkboxes that mirror the git index, and the empty states (6a git missing, 6b not a
//! repository, 6c no changes). The rows come from `state::git::change_rows`, rebuilt by
//! [`AppView::rebuild_git_rows`] when the status changes, never in render.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::git::{
    ChangeGroup, ChangeRow, ChangeRowKind, Check, GitAvailability, RepoState, change_rows,
    fill_methods, staged_outside,
};
use crate::state::locale::plural;
use crate::state::number::format_integer;
use crate::state::settings::SettingsCategory;
use crate::theme::metrics::ICON_BUTTON_HEADER;
use crate::theme::{Palette, PaletteExt};
use telaio_ui::components::{CheckState, TriCheckbox};

use crate::views::components::{
    ConfirmDialog, EmptyState, IconButton, MethodBadge, PrimaryButton, SecondaryButton,
    SectionLabel, TreeRow, open_confirm_dialog,
};

use super::super::root::AppView;

/// Where the "How to install Git" link points.
pub(crate) const INSTALL_GIT_URL: &str = "https://postino.tanis.codes/docs/git.html#installing-git";

/// What a click on a row's checkbox does.
#[derive(Clone, Copy)]
enum IndexAction {
    Stage,
    Unstage,
}

impl AppView {
    /// Rebuilds [`crate::state::git::GitState::rows`] from the last status, the folded folders
    /// and the collection tree (for the method badges). Call when any of them changes.
    pub(crate) fn rebuild_git_rows(&mut self) {
        let (rows, outside) = match self.git.status() {
            Some((repo, status)) => {
                let mut rows = change_rows(status, repo.prefix(), &self.git.collapsed);
                if let Some(workspace) = self.state.workspace.as_ref() {
                    fill_methods(&mut rows, workspace.tree());
                }
                (rows, staged_outside(status, repo.prefix()))
            }
            None => (Vec::new(), Vec::new()),
        };
        self.git.rows = Arc::new(rows);
        self.git.staged_outside = Arc::new(outside);
    }

    /// Folds or unfolds a group or folder of the panel.
    fn toggle_git_row(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.git.collapsed.remove(&key) {
            self.git.collapsed.insert(key);
        }
        self.rebuild_git_rows();
        cx.notify();
    }

    /// Stages or unstages `paths` (repository relative) on the background executor, then reads
    /// the status again.
    fn change_index(&mut self, action: IndexAction, paths: Vec<String>, cx: &mut Context<Self>) {
        let Some(repo) = self.git.repo().cloned() else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        self.run_git_write(cx, move || match action {
            IndexAction::Stage => repo.stage(&paths),
            IndexAction::Unstage => repo.unstage(&paths),
        });
    }

    /// Discards the changes of `paths`, after a confirmation when the setting asks for one.
    fn discard_paths(
        &mut self,
        label: String,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.git.repo().cloned() else {
            return;
        };
        // An empty path would cover the whole repository.
        if paths.is_empty() || paths.iter().any(|path| path.is_empty()) {
            return;
        }
        if !self.state.settings.git_confirm_discard {
            self.run_git_write(cx, move || repo.discard(&paths));
            return;
        }
        let weak = cx.weak_entity();
        open_confirm_dialog(
            window,
            cx,
            ConfirmDialog {
                title: t!("git.discard.title").into(),
                text: t!("git.discard.text", name = label).into(),
                confirm_label: t!("git.discard.confirm").into(),
                destructive: true,
                show_cancel: true,
                on_confirm: Rc::new(move |_, cx| {
                    let repo = repo.clone();
                    let paths = paths.clone();
                    let _ = weak.update(cx, |view, cx| {
                        view.run_git_write(cx, move || repo.discard(&paths));
                    });
                }),
            },
        );
    }

    /// Opens Settings on its Git category.
    pub(crate) fn open_git_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_category = SettingsCategory::Git;
        self.open_settings(window, cx);
    }

    /// Renders the Source Control panel.
    pub(crate) fn render_git_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let header = h_flex()
            .justify_between()
            .items_center()
            .h(px(36.0))
            .pt(px(10.0))
            .pr(px(10.0))
            .pb(px(6.0))
            .pl(px(14.0))
            .child(SectionLabel::new(t!("git.panel.title")))
            .children(self.git.repo().map(|_| {
                let weak = weak.clone();
                IconButton::new("git-refresh", IconName::RefreshCw)
                    .box_size(ICON_BUTTON_HEADER)
                    .tooltip(t!("git.panel.refresh"))
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |view, cx| view.refresh_git_status(cx));
                    })
            }));

        v_flex()
            .size_full()
            .bg(palette.surface)
            .border_r_1()
            .border_color(palette.border)
            .child(header)
            .child(self.render_git_body(weak, &palette, cx))
            .into_any_element()
    }

    fn render_git_body(
        &self,
        weak: WeakEntity<Self>,
        palette: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        if self.state.workspace.is_none() {
            return panel_empty(EmptyState::new(t!("git.empty.no_workspace")));
        }
        match &self.git.availability {
            GitAvailability::Detecting => return div().into_any_element(),
            GitAvailability::Missing(_) => return render_git_missing(weak, palette),
            GitAvailability::Ready(_) => {}
        }
        match &self.git.repo {
            RepoState::Unknown | RepoState::Repo { status: None, .. } => div().into_any_element(),
            RepoState::NotARepo => self.render_not_a_repo(weak),
            RepoState::Error(message) => panel_empty(
                EmptyState::new(t!("git.empty.error"))
                    .icon(Icon::new(IconName::CircleAlert))
                    .description(message.clone()),
            ),
            RepoState::Repo {
                status: Some(status),
                ..
            } => {
                if self.git.rows.is_empty() {
                    let description = if status.ahead > 0 {
                        plural("git.empty.ahead", status.ahead as usize)
                    } else {
                        t!("git.empty.clean").into_owned()
                    };
                    return v_flex()
                        .flex_1()
                        .min_h_0()
                        .children(self.render_outside_notice(weak.clone(), palette))
                        .child(panel_empty(
                            EmptyState::new(t!("git.empty.no_changes"))
                                .icon(Icon::new(IconName::CircleCheck))
                                .description(description),
                        ))
                        .into_any_element();
                }
                let rows = self.git.rows.clone();
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .children(self.render_outside_notice(weak.clone(), palette))
                    .child(
                        div()
                            .id("git-panel-rows")
                            .flex_1()
                            .min_h_0()
                            .px(px(6.0))
                            .overflow_y_scroll()
                            .child(v_flex().gap(px(1.0)).children(rows.iter().enumerate().map(
                                |(index, row)| {
                                    render_change_row(index, row, weak.clone(), palette, cx)
                                },
                            ))),
                    )
                    .into_any_element()
            }
        }
    }

    /// 6b: the workspace is not in a repository.
    fn render_not_a_repo(&self, weak: WeakEntity<Self>) -> AnyElement {
        panel_empty(
            EmptyState::new(t!("git.empty.not_a_repo"))
                .icon(Icon::new(IconName::GitBranch))
                .description(t!("git.empty.not_a_repo_text"))
                .action(
                    PrimaryButton::new("git-init", t!("git.empty.init"))
                        .icon(Icon::new(IconName::Plus))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |view, cx| view.init_git_repository(cx));
                        }),
                ),
        )
    }

    /// The notice over the rows when files outside the workspace are staged: they would go into
    /// the next commit although the panel does not list them.
    fn render_outside_notice(
        &self,
        weak: WeakEntity<Self>,
        palette: &Palette,
    ) -> Option<AnyElement> {
        let outside = self.git.staged_outside.clone();
        if outside.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .mx(px(8.0))
                .mb(px(6.0))
                .p(px(8.0))
                .gap(px(6.0))
                .rounded(px(6.0))
                .bg(palette.warning.opacity(0.10))
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(plural("git.panel.staged_outside", outside.len()))
                .child(
                    div().child(
                        SecondaryButton::new("git-unstage-outside", t!("git.panel.unstage_them"))
                            .on_click(move |_, _, cx| {
                                let paths = outside.to_vec();
                                let _ = weak.update(cx, |view, cx| {
                                    view.change_index(IndexAction::Unstage, paths, cx);
                                });
                            }),
                    ),
                )
                .into_any_element(),
        )
    }
}

/// 6a: git is not installed, too old or failed to run.
fn render_git_missing(weak: WeakEntity<AppView>, palette: &Palette) -> AnyElement {
    panel_empty(
        EmptyState::new(t!("git.empty.missing"))
            .icon(Icon::new(IconName::CircleAlert))
            .description(t!("git.empty.missing_text"))
            .action(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        SecondaryButton::new("git-open-settings", t!("git.empty.open_settings"))
                            .icon(Icon::new(IconName::Settings))
                            .on_click(move |_, window, cx| {
                                let _ =
                                    weak.update(cx, |view, cx| view.open_git_settings(window, cx));
                            }),
                    )
                    .child(
                        div()
                            .id("git-install-link")
                            .text_size(px(12.0))
                            .text_color(palette.accent_text)
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            .child(t!("git.empty.install"))
                            .on_click(|_, _, cx| cx.open_url(INSTALL_GIT_URL)),
                    ),
            ),
    )
}

/// An empty state laid out inside the panel, with side padding so its text wraps.
fn panel_empty(state: EmptyState) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .px(px(16.0))
        .child(state)
        .into_any_element()
}

/// The status letter's color: modified amber, added and untracked green, deleted red, renamed
/// blue, conflicts red.
fn letter_color(letter: char, palette: &Palette) -> Hsla {
    match letter {
        'A' | 'U' => palette.success,
        'D' | '!' => palette.danger,
        'R' => palette.info,
        _ => palette.warning,
    }
}

/// The checkbox state of a [`Check`].
fn check_state(check: Check) -> CheckState {
    match check {
        Check::On => CheckState::Checked,
        Check::Off => CheckState::Unchecked,
        Check::Partial => CheckState::Mixed,
    }
}

/// One row of the panel: a group header, a folder or a file.
fn render_change_row(
    index: usize,
    row: &ChangeRow,
    weak: WeakEntity<AppView>,
    palette: &Palette,
    cx: &Context<AppView>,
) -> AnyElement {
    let check = row.check;
    let paths = row.repo_paths.clone();
    let checkbox = {
        let weak = weak.clone();
        let paths = paths.clone();
        TriCheckbox::new(("git-check", index), check_state(check)).on_click(move |_, _, cx| {
            let action = match check {
                Check::On => IndexAction::Unstage,
                Check::Off | Check::Partial => IndexAction::Stage,
            };
            let paths = paths.clone();
            let _ = weak.update(cx, |view, cx| view.change_index(action, paths, cx));
        })
    };
    let discard_label = match &row.kind {
        ChangeRowKind::Group(_) => t!("git.panel.all_changes").into_owned(),
        _ => row.path.clone(),
    };
    let discard = {
        let weak = weak.clone();
        let paths = paths.clone();
        IconButton::new(("git-discard", index), IconName::Undo2)
            .box_size(22.0)
            .tooltip(t!("git.panel.discard"))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                let paths = paths.clone();
                let label = discard_label.clone();
                let _ = weak.update(cx, |view, cx| view.discard_paths(label, paths, window, cx));
            })
    };
    let mono = cx.theme().mono_font_family.clone();
    let count = |count: usize| {
        div()
            .font_family(mono.clone())
            .text_size(px(11.0))
            .text_color(palette.fg_subtle)
            .child(format_integer(count as u64))
    };

    match &row.kind {
        ChangeRowKind::Group(group) => {
            let label = match group {
                ChangeGroup::Changes => t!("git.panel.changes"),
                ChangeGroup::Unversioned => t!("git.panel.unversioned"),
            };
            let key = row.key.clone();
            TreeRow::new(("git-row", index), label)
                .folder(row.expanded)
                .leading(checkbox)
                .trailing(discard)
                .suffix(count(row.count))
                .on_click(move |_, _, cx| {
                    let key = key.clone();
                    let _ = weak.update(cx, |view, cx| view.toggle_git_row(key, cx));
                })
                .into_any_element()
        }
        ChangeRowKind::Folder => {
            let key = row.key.clone();
            TreeRow::new(("git-row", index), row.label.clone())
                .depth(row.depth)
                .folder(row.expanded)
                .leading(
                    h_flex().gap(px(6.0)).items_center().child(checkbox).child(
                        Icon::new(IconName::Folder)
                            .small()
                            .text_color(palette.fg_subtle),
                    ),
                )
                .trailing(discard)
                .suffix(count(row.count))
                .on_click(move |_, _, cx| {
                    let key = key.clone();
                    let _ = weak.update(cx, |view, cx| view.toggle_git_row(key, cx));
                })
                .into_any_element()
        }
        ChangeRowKind::File { letter, deleted } => {
            let letter_element = div()
                .w(px(12.0))
                .font_family(mono.clone())
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(letter_color(*letter, palette))
                .child(letter.to_string());
            let leading = h_flex()
                .gap(px(6.0))
                .items_center()
                .child(checkbox)
                .children(row.method.clone().map(MethodBadge::inline));
            let request_id = row.path.ends_with(".postino").then(|| row.path.clone());
            let deleted = *deleted;
            TreeRow::new(("git-row", index), row.label.clone())
                .depth(row.depth)
                .strikethrough(deleted)
                .leading(leading)
                .trailing(discard)
                .suffix(letter_element)
                .on_click(move |_, _, cx| {
                    // Until the diff tab (#168): a click opens the request.
                    if let (Some(id), false) = (request_id.clone(), deleted) {
                        let _ = weak.update(cx, |view, cx| view.open_request(id, cx));
                    }
                })
                .into_any_element()
        }
    }
}
