//! The command palette (`Ctrl K` / `Cmd K`, and the title bar's search trigger,
//! `plans/ui-redesign.md` phase 7 item 1): a centered overlay listing every open request,
//! environment and built-in action, fuzzy-filtered as the user types.
//!
//! Built on gpui-component's `Command` (`plans/ui-redesign-spikes.md` section 5) with its own
//! local matching turned off (`filterable(false)`): every item this module hands it is kept as
//! given, already ranked and grouped by `state::palette::fuzzy_filter`. The palette's own
//! `CommandState` entity is a real child of the render tree (`Command::render` returns it), so
//! gpui redraws it on its own whenever the query changes; this module's content closure simply
//! reads that query fresh on every repaint (the same "re-read on every repaint" pattern
//! `views/settings.rs` documents) and rebuilds the filtered, grouped item list from it. Because
//! we never use `Command::item`/`.items()` (only `.group()`, always all three, even when a group
//! ends up empty), a matched item's `IndexPath.section` is always 0 for Requests, 1 for
//! Environments, 2 for Actions, which is what lets `on_confirm` map straight back into
//! `PaletteItemKind` without any extra bookkeeping.

use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_core::Method;
use rust_i18n::t;

use crate::state::palette::{self, ActionId, PaletteItem, PaletteItemKind};
use crate::theme::Palette;
use crate::theme::PaletteExt;
use crate::views::components::{MethodBadge, edit_menu};

use super::root::AppView;
use super::{import_menu, sidebar};

/// Right margin of a row's shortcut hint, so the list's scrollbar does not sit on top of it.
const SCROLLBAR_CLEARANCE: f32 = 6.0;

/// Width of the palette overlay.
const PALETTE_WIDTH: f32 = 560.0;
/// Max height of the command list itself (excluding the search field). Generous enough that a
/// small workspace like the sample one (a handful of requests, a couple of environments, the ten
/// built-in actions) never needs to scroll at all, so the list's last row never ends up sitting
/// flush against the dialog's own rounded bottom corner (see the bottom padding below for the
/// case where a larger workspace still does need to scroll).
const PALETTE_LIST_HEIGHT: f32 = 480.0;
/// Bottom padding kept between the command list and the dialog's rounded bottom corner, so a cut
/// (or merely the last) row never touches it directly.
const PALETTE_BOTTOM_PADDING: f32 = 8.0;
/// Height of the search row. `Command`'s own search field is `h_8` (2rem, so 26px at the 13px
/// UI font) and cannot be resized, so the palette draws its own (see [`render_search_row`]).
const SEARCH_ROW_HEIGHT: f32 = 36.0;
/// The three group headings, in the fixed order they are always shown (`Requests` first even
/// when empty, matching `IndexPath.section` 0/1/2, see the module doc comment). Built on every
/// repaint, so they follow the current language.
fn group_labels() -> [String; 3] {
    [
        t!("common.requests").into_owned(),
        t!("shell.palette.environments").into_owned(),
        t!("shell.palette.actions").into_owned(),
    ]
}

impl AppView {
    /// Opens the command palette. A no-op when a dialog is already open, so `Ctrl K` and the
    /// title bar trigger never stack a second one (same guard as `open_settings`).
    pub(crate) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let weak = cx.weak_entity();
        let command_state = cx.new(|cx| CommandState::new(window, cx));
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.title_bar.search_hint")));
        // Typing in the palette's own search field drives `Command`'s query, as its built-in
        // field would. Owned by the dialog's builder below, so it ends with the dialog.
        let query_subscription = window.subscribe(&search_input, cx, {
            let command_state = command_state.clone();
            move |input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value();
                    command_state.update(cx, |state, cx| state.set_query(query, window, cx));
                }
            }
        });
        let focus_search_input = search_input.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let _ = &query_subscription;
            let palette = cx.palette();
            let weak = weak.clone();
            let command_state = command_state.clone();
            let search_input = search_input.clone();
            // Top-anchored rather than dead center (unlike the Settings modal):
            // the design system does not cover the palette, and every well-known command
            // palette (VS Code, Raycast, and our own "Ctrl K" search trigger in the title bar)
            // opens near the top of the window, not centered.
            let viewport_height: f32 = window.viewport_size().height.into();
            let margin_top = (viewport_height * 0.14).max(48.0);
            dialog
                .w(px(PALETTE_WIDTH))
                .margin_top(px(margin_top))
                .p_0()
                .border_0()
                .bg(palette.overlay)
                .close_button(false)
                .content(move |content, _window, cx| {
                    content
                        .min_h_0()
                        .child(
                            div()
                                .pb(px(PALETTE_BOTTOM_PADDING))
                                .child(render_palette_content(
                                    weak.clone(),
                                    command_state.clone(),
                                    search_input.clone(),
                                    cx,
                                )),
                        )
                })
        });
        // Focus the search field only *after* `open_dialog` above: `Root::open_dialog` captures
        // `window.focused(cx)` at the moment it runs as the handle to restore once the dialog
        // closes. Focusing the search field first would make that capture point at the search
        // field itself instead of whatever was focused before opening the palette, and closing
        // the dialog would then try to restore focus to a handle that no longer renders anything
        // (the `CommandState` is gone with the dialog), leaving nothing focused and every
        // window-level shortcut (`Ctrl K`, `Ctrl S`, `Ctrl ,`, ...) dead until the next manual
        // click. Doing it here keeps `Root`'s own capture correct while still autofocusing the
        // field for the first keystroke.
        focus_search_input.update(cx, |state, cx| state.focus(window, cx));
    }
}

/// Reads the palette's current query, rebuilds the ranked, grouped item list from it, and
/// renders the `Command`.
fn render_palette_content(
    weak: WeakEntity<AppView>,
    command_state: Entity<CommandState>,
    search_input: Entity<InputState>,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let all_items = build_all_items(&view, cx);
    let query = command_state.read(cx).query(cx).to_string();
    let ranked = palette::fuzzy_filter(&query, &all_items);

    // One ranked, ordered list of (item, matched char indices) per group, in `group_labels`'
    // fixed order, keeping each match's relative rank inside its own group.
    let mut sections: [Vec<(PaletteItem, Vec<usize>)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (index, _score, matched) in ranked {
        let item = all_items[index].clone();
        let section = match item.kind {
            PaletteItemKind::Request(_) => 0,
            PaletteItemKind::Environment(_) => 1,
            PaletteItemKind::Action(_) => 2,
        };
        sections[section].push((item, matched));
    }

    let active_environment = view.read(cx).state.active_environment.clone();
    let palette = cx.palette();
    let mono_font = cx.theme().mono_font_family.clone();

    // Captured by `on_confirm` below, in the same order and shape as the groups pushed onto
    // `Command`, so `IndexPath{section, row}` maps straight back to a `PaletteItemKind` (see the
    // module doc comment).
    let confirm_kinds: Vec<Vec<PaletteItemKind>> = sections
        .iter()
        .map(|group| group.iter().map(|(item, _)| item.kind.clone()).collect())
        .collect();

    // `Command` paints its own background, square unless bordered, over the dialog's rounded
    // top corners; the bottom ones sit below the list's padding, on the dialog itself.
    let mut command = Command::new(&command_state)
        .bordered(false)
        .filterable(false)
        .searchable(false)
        .rounded_t(cx.theme().radius_lg)
        .max_h(px(PALETTE_LIST_HEIGHT))
        .header(move |_, _, cx| render_search_row(&search_input, cx));

    for (group_items, heading) in sections.iter().zip(group_labels()) {
        let mut group = CommandGroup::new().label(heading);
        for (item, matched) in group_items {
            let item = item.clone();
            let matched = matched.clone();
            let is_active_environment = matches!(
                &item.kind,
                PaletteItemKind::Environment(name) if name.as_deref() == active_environment.as_deref()
            );
            let row_palette = palette.clone();
            let row_mono_font = mono_font.clone();
            group = group.item(CommandItem::new().child(move |_, _| {
                render_item_row(
                    &item,
                    &matched,
                    is_active_environment,
                    &row_palette,
                    row_mono_font.clone(),
                )
            }));
        }
        command = command.group(group);
    }

    command
        .on_confirm(move |index_path, window, cx| {
            let Some(kind) = confirm_kinds
                .get(index_path.section)
                .and_then(|group| group.get(index_path.row))
                .cloned()
            else {
                return;
            };
            window.close_dialog(cx);
            execute_palette_item(weak.clone(), kind, window, cx);
        })
        .into_any_element()
}

/// The search row: the same icon and borderless field as `Command`'s built-in one, at
/// [`SEARCH_ROW_HEIGHT`]. Rendered as `Command`'s header, inside its key context, so Up, Down,
/// Enter and Escape typed in the field still move the selection, confirm and cancel.
fn render_search_row(search_input: &Entity<InputState>, cx: &App) -> AnyElement {
    h_flex()
        .flex_none()
        .h(px(SEARCH_ROW_HEIGHT))
        .px_3()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(Icon::new(IconName::Search).text_color(cx.theme().muted_foreground))
        .child(
            Input::new(search_input)
                .context_menu(edit_menu(search_input, cx))
                .appearance(false)
                .flex_1()
                .p_0(),
        )
        .into_any_element()
}

/// Builds the full, unranked item list: every request in the open workspace, every environment
/// (plus "No environment"), and every built-in action.
fn build_all_items(view: &Entity<AppView>, cx: &mut App) -> Vec<PaletteItem> {
    let read = view.read(cx);
    let requests = read
        .state
        .workspace
        .as_ref()
        .map(|workspace| palette::request_items(workspace.tree()))
        .unwrap_or_default();
    let environments = read
        .state
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.list_environments().ok())
        .unwrap_or_default();
    let mut items = requests;
    items.extend(palette::environment_items(&environments));
    items.extend(palette::action_items());
    items
}

/// Renders one row's content: a leading marker (method text for a request, a dot/ring/check for
/// an environment, an icon for an action), the label with its matched characters highlighted in
/// `accent_text`, and the shortcut hint on the right.
fn render_item_row(
    item: &PaletteItem,
    matched: &[usize],
    is_active_environment: bool,
    palette: &Palette,
    mono_font: SharedString,
) -> AnyElement {
    let leading = render_leading(item, is_active_environment, palette);
    let label = highlighted_label(&item.label, matched, palette.accent_text);

    let mut row = h_flex()
        .w_full()
        .items_center()
        .gap(px(8.0))
        .child(leading)
        .child(div().flex_1().min_w_0().truncate().child(label));
    if let Some(shortcut) = &item.shortcut {
        row = row.child(
            div()
                .flex_none()
                .mr(px(SCROLLBAR_CLEARANCE))
                .font_family(mono_font)
                .text_size(px(10.5))
                .text_color(palette.fg_subtle)
                .child(shortcut.clone()),
        );
    }
    row.into_any_element()
}

/// The leading marker of one row, per `item.kind`.
fn render_leading(
    item: &PaletteItem,
    is_active_environment: bool,
    palette: &Palette,
) -> AnyElement {
    match &item.kind {
        // The sidebar tree's colored badge, at its natural width: a fixed 34px column wrapped
        // `DELETE` and `OPTIONS` onto two lines. `detail` holds the method's own text, and
        // parsing it back is lossless (a custom method is kept verbatim).
        PaletteItemKind::Request(_) => match item.detail.as_deref().map(str::parse::<Method>) {
            Some(Ok(method)) => MethodBadge::inline(method).into_any_element(),
            _ => div().into_any_element(),
        },
        PaletteItemKind::Environment(name) => {
            let mut leading = div().flex_none().w(px(14.0)).flex().justify_center();
            leading = if is_active_environment {
                leading.child(
                    Icon::new(IconName::Check)
                        .small()
                        .text_color(palette.accent_text),
                )
            } else {
                match name.as_deref().map(crate::state::env_color::env_color) {
                    Some(color) => leading.child(
                        div()
                            .size(px(7.0))
                            .rounded_full()
                            .bg(palette.env_color(color)),
                    ),
                    None => leading.child(
                        div()
                            .size(px(7.0))
                            .rounded_full()
                            .border_1()
                            .border_color(palette.fg_subtle),
                    ),
                }
            };
            leading.into_any_element()
        }
        PaletteItemKind::Action(action) => div()
            .flex_none()
            .w(px(14.0))
            .flex()
            .justify_center()
            .child(
                Icon::new(action_icon(*action))
                    .small()
                    .text_color(palette.fg_muted),
            )
            .into_any_element(),
    }
}

/// The icon shown for a built-in action's row.
fn action_icon(action: ActionId) -> gpui_kit::assets::IconName {
    use gpui_kit::assets::IconName as Lucide;
    match action {
        ActionId::Send => Lucide::SendHorizontal,
        ActionId::Save => Lucide::Save,
        ActionId::NewRequest => Lucide::Plus,
        ActionId::NewFolder => Lucide::FolderPlus,
        ActionId::ImportCollection | ActionId::ImportEnvironment => Lucide::Download,
        ActionId::OpenSettings => Lucide::Settings,
        ActionId::OpenShortcuts => Lucide::Keyboard,
        ActionId::OpenWorkspace => Lucide::Folder,
        ActionId::NewLoadTest => Lucide::Gauge,
        ActionId::ToggleTheme => Lucide::Palette,
        ActionId::CheckForUpdates => Lucide::RefreshCw,
    }
}

/// Renders `label` with the characters at `matched` (char indices, as `fuzzy_filter` returns
/// them) colored `match_color`, everything else in the ambient text color.
fn highlighted_label(label: &str, matched: &[usize], match_color: Hsla) -> AnyElement {
    let runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = label
        .char_indices()
        .enumerate()
        .filter(|(char_index, _)| matched.contains(char_index))
        .map(|(_, (byte_start, character))| {
            let byte_end = byte_start + character.len_utf8();
            (
                byte_start..byte_end,
                HighlightStyle {
                    color: Some(match_color),
                    ..Default::default()
                },
            )
        })
        .collect();
    StyledText::new(label.to_string())
        .with_highlights(runs)
        .into_any_element()
}

/// Executes a confirmed item: opens a request, switches environment, or runs a built-in action.
fn execute_palette_item(
    weak: WeakEntity<AppView>,
    kind: PaletteItemKind,
    window: &mut Window,
    cx: &mut App,
) {
    match kind {
        PaletteItemKind::Request(id) => {
            let _ = weak.update(cx, |view, cx| view.open_request(id, cx));
        }
        PaletteItemKind::Environment(name) => {
            let _ = weak.update(cx, |view, cx| view.select_environment(name, cx));
        }
        PaletteItemKind::Action(action) => execute_action(weak, action, window, cx),
    }
}

/// Runs one [`ActionId`], reusing the same entry points the title bar, sidebar and command
/// palette all share.
fn execute_action(weak: WeakEntity<AppView>, action: ActionId, window: &mut Window, cx: &mut App) {
    match action {
        ActionId::Send => {
            let _ = weak.update(cx, |view, cx| view.send_active_tab(cx));
        }
        ActionId::Save => {
            let _ = weak.update(cx, |view, cx| view.save_active_tab(cx));
        }
        ActionId::NewRequest => sidebar::open_new_request_dialog(weak, None, window, cx),
        ActionId::NewFolder => sidebar::open_new_folder_dialog(weak, None, window, cx),
        ActionId::ImportCollection => {
            import_menu::pick_and_import(weak, import_menu::ImportKind::Collection, window, cx);
        }
        ActionId::ImportEnvironment => {
            import_menu::pick_and_import(weak, import_menu::ImportKind::Environment, window, cx);
        }
        ActionId::OpenSettings => {
            let _ = weak.update(cx, |view, cx| view.open_settings(window, cx));
        }
        ActionId::OpenShortcuts => {
            let _ = weak.update(cx, |view, cx| view.open_shortcuts(window, cx));
        }
        ActionId::OpenWorkspace => sidebar::pick_workspace_folder(weak, window, cx),
        ActionId::NewLoadTest => {
            let _ = weak.update(cx, |view, cx| view.open_new_load_test_tab(cx));
        }
        ActionId::ToggleTheme => {
            let _ = weak.update(cx, |view, cx| view.toggle_theme(window, cx));
        }
        ActionId::CheckForUpdates => {
            let _ = weak.update(cx, |view, cx| view.check_for_updates_manually(window, cx));
        }
    }
}
