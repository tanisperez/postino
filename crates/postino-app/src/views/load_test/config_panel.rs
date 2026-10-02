//! The load test tab's left config panel (`plans/ui-redesign.md` phase 8 item 3): the target
//! segmented control and picker, the numeric fields, the "Stop on errors" switch, and the
//! Start/Stop run button.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::state::load_test::{LoadTestConfigInputs, LoadTestTab, LoadTestTarget, TargetKind};
use crate::state::locale;
use crate::theme::metrics::{CONTROL_HEIGHT, RADIUS_MD};
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{
    DangerButton, PrimaryButton, SegmentedControl, SegmentedItem, edit_menu,
};

use crate::views::root::AppView;

/// Width of the config panel (`plans/ui-redesign.md` phase 8 item 3: "280 wide").
const PANEL_WIDTH: f32 = 280.0;

/// The `gpui` entities behind a load test tab's editable numeric fields, rebuilt only when the
/// active tab id changes, the same reasoning as `views/request_editor.rs`'s
/// `RequestEditorEntities`: typing into one of these must never lose focus on an unrelated
/// re-render. The target picker needs no entity of its own (a plain dropdown menu, like
/// `views/define_variable.rs`'s environment select).
#[derive(Default)]
pub(crate) struct LoadTestEntities {
    built_for: Option<String>,
    vus: Option<Entity<InputState>>,
    duration_secs: Option<Entity<InputState>>,
    ramp_up_secs: Option<Entity<InputState>>,
    think_time_ms: Option<Entity<InputState>>,
}

impl LoadTestEntities {
    /// Rebuilds every field entity when `tab_id` differs from what they were last built for.
    fn sync(
        &mut self,
        tab_id: &str,
        config: &LoadTestConfigInputs,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) {
        if self.built_for.as_deref() == Some(tab_id) {
            return;
        }
        self.vus = Some(build_field(
            tab_id,
            &config.vus,
            window,
            cx,
            |view, tab_id, value, cx| view.set_load_test_vus(tab_id, value, cx),
        ));
        self.duration_secs = Some(build_field(
            tab_id,
            &config.duration_secs,
            window,
            cx,
            |view, tab_id, value, cx| view.set_load_test_duration_secs(tab_id, value, cx),
        ));
        self.ramp_up_secs = Some(build_field(
            tab_id,
            &config.ramp_up_secs,
            window,
            cx,
            |view, tab_id, value, cx| view.set_load_test_ramp_up_secs(tab_id, value, cx),
        ));
        self.think_time_ms = Some(build_field(
            tab_id,
            &config.think_time_ms,
            window,
            cx,
            |view, tab_id, value, cx| view.set_load_test_think_time_ms(tab_id, value, cx),
        ));
        self.built_for = Some(tab_id.to_string());
    }
}

/// Builds one numeric field's `InputState`, seeded with `initial`, calling `on_change` with its
/// new text on every edit.
fn build_field(
    tab_id: &str,
    initial: &str,
    window: &mut Window,
    cx: &mut Context<AppView>,
    on_change: impl Fn(&mut AppView, &str, String, &mut Context<AppView>) + 'static,
) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx));
    input.update(cx, |state, cx| {
        state.set_value(initial.to_string(), window, cx);
    });
    let tab_id = tab_id.to_string();
    cx.subscribe(&input, move |view, entity, event: &InputEvent, cx| {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = entity.read(cx).value().to_string();
        on_change(view, &tab_id, value, cx);
    })
    .detach();
    input
}

impl AppView {
    /// Renders the config panel for the load test tab `tab_id`.
    pub(crate) fn render_load_test_config_panel(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(load_test) = self
            .state
            .tabs
            .active()
            .and_then(|tab| tab.load_test())
            .cloned()
        else {
            return div().into_any_element();
        };
        self.load_test_entities
            .sync(tab_id, &load_test.config, window, cx);

        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let ui_font = cx.theme().font_family.clone();
        let is_running = load_test.is_running();
        let weak = cx.weak_entity();
        let tree = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.tree().to_vec());

        // Split in two: a scrollable fields area (`flex_1().min_h_0().overflow_y_scroll()`) and a
        // `flex_none` footer holding the Start/Stop button and the note below it. Previously
        // everything (including the button) was one `v_flex` with a `div().flex_1()` spacer
        // pushing the button down; with no scroll anywhere, a short window simply clipped the
        // button and note below the visible area instead of keeping them reachable
        // (`plans/ui-redesign.md` phase 8's responsiveness fix).
        v_flex()
            .flex_none()
            .w(px(PANEL_WIDTH))
            .h_full()
            .min_h_0()
            .border_r_1()
            .border_color(palette.border)
            .child(
                v_flex()
                    .id("load-test-config-fields")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p(px(16.0))
                    .gap(px(16.0))
                    .child(render_target_section(
                        weak.clone(),
                        tab_id,
                        &palette,
                        &load_test,
                        tree.as_deref(),
                        is_running,
                    ))
                    .child(render_field(
                        &palette,
                        &mono_font,
                        &ui_font,
                        t!("load_test.config.vus").into_owned(),
                        None,
                        self.load_test_entities.vus.as_ref(),
                        "VUs",
                        is_running,
                        cx,
                    ))
                    .child(render_field(
                        &palette,
                        &mono_font,
                        &ui_font,
                        t!("load_test.config.duration").into_owned(),
                        None,
                        self.load_test_entities.duration_secs.as_ref(),
                        "s",
                        is_running,
                        cx,
                    ))
                    .child(render_field(
                        &palette,
                        &mono_font,
                        &ui_font,
                        t!("load_test.config.ramp_up").into_owned(),
                        Some(format!("0 \u{2192} {} VUs", load_test.config.vus.trim())),
                        self.load_test_entities.ramp_up_secs.as_ref(),
                        "s",
                        is_running,
                        cx,
                    ))
                    .child(render_field(
                        &palette,
                        &mono_font,
                        &ui_font,
                        t!("load_test.config.think_time").into_owned(),
                        Some(t!("load_test.config.per_iteration").into_owned()),
                        self.load_test_entities.think_time_ms.as_ref(),
                        "ms",
                        is_running,
                        cx,
                    ))
                    .child(render_stop_on_error_row(
                        weak.clone(),
                        tab_id,
                        &palette,
                        &load_test,
                        is_running,
                    )),
            )
            .child(
                v_flex()
                    .flex_none()
                    .px(px(16.0))
                    .pb(px(16.0))
                    .gap(px(16.0))
                    .child(render_start_stop_button(weak.clone(), tab_id, is_running))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(palette.fg_subtle)
                            .text_center()
                            .w_full()
                            .child(div().w_full().child(t!("load_test.config.footer")))
                            .child(div().font_family(mono_font).child(".postino/runs/")),
                    ),
            )
            .into_any_element()
    }
}

/// The "Target" label, segmented control (Request/Collection) and the target picker dropdown.
fn render_target_section(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
    tree: Option<&[postino_workspace::Node]>,
    is_running: bool,
) -> AnyElement {
    let request_weak = weak.clone();
    let request_tab_id = tab_id.to_string();
    let collection_weak = weak.clone();
    let collection_tab_id = tab_id.to_string();

    v_flex()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.fg_muted)
                .child(t!("load_test.config.target")),
        )
        .child(
            SegmentedControl::new("load-test-target-kind")
                .item(
                    SegmentedItem::new(t!("common.request"))
                        .selected(load_test.target_kind == TargetKind::Request)
                        .on_click(move |_, cx| {
                            let _ = request_weak.update(cx, |view, cx| {
                                view.set_load_test_target_kind(
                                    &request_tab_id,
                                    TargetKind::Request,
                                    cx,
                                );
                            });
                        }),
                )
                .item(
                    SegmentedItem::new(t!("load_test.config.kind_collection"))
                        .selected(load_test.target_kind == TargetKind::Collection)
                        .on_click(move |_, cx| {
                            let _ = collection_weak.update(cx, |view, cx| {
                                view.set_load_test_target_kind(
                                    &collection_tab_id,
                                    TargetKind::Collection,
                                    cx,
                                );
                            });
                        }),
                ),
        )
        .child(render_target_picker(
            weak, tab_id, palette, load_test, tree, is_running,
        ))
        .into_any_element()
}

/// The bordered row showing the currently picked target (or a placeholder), opening a dropdown of
/// every request or folder (depending on [`LoadTestTab::target_kind`]) on click.
fn render_target_picker(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
    tree: Option<&[postino_workspace::Node]>,
    is_running: bool,
) -> AnyElement {
    use crate::state::load_test::{folders_flat, requests_flat};

    let is_collection = load_test.target_kind == TargetKind::Collection;
    let icon = if is_collection {
        Some(IconName::Folder)
    } else {
        None
    };
    let label = if load_test.target_label.is_empty() {
        if is_collection {
            t!("load_test.config.choose_folder").into_owned()
        } else {
            t!("load_test.config.choose_request").into_owned()
        }
    } else {
        load_test.target_label.clone()
    };
    let hint = if load_test.target.is_some() && is_collection {
        Some(request_count_label(load_test.target_request_count))
    } else {
        None
    };

    let mut content = h_flex()
        .w_full()
        .items_center()
        .gap(px(8.0))
        .when_some(icon, |row, icon| {
            row.child(Icon::new(icon).small().text_color(palette.fg_subtle))
        })
        .child(div().flex_1().min_w_0().truncate().child(label));
    if let Some(hint) = hint {
        content = content.child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg_subtle)
                .child(hint),
        );
    }

    // `DropdownMenu` (`gpui-component`'s trait for opening a `PopupMenu` on click) is only
    // implemented for `Button` (`gpui-component-0.6.6/src/menu/dropdown_menu.rs`, `impl
    // DropdownMenu for Button {}`), not for a plain styled `div`/`h_flex`: this trigger is a
    // `Button` for that reason, ghosted and re-bordered to look like the design's bordered pill
    // row, the same trick `views/define_variable.rs`'s `render_environment_select` and
    // `views/settings.rs`'s `render_font_picker` already use for their own dropdown triggers.
    let trigger = Button::new("load-test-target-picker")
        .ghost()
        .w_full()
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .disabled(is_running)
        .when(is_running, |button| button.opacity(0.5))
        .child(content);
    if is_running {
        return trigger.into_any_element();
    }

    let Some(tree) = tree else {
        return trigger.into_any_element();
    };

    if is_collection {
        let folders = folders_flat(tree);
        let tab_id = tab_id.to_string();
        trigger
            .dropdown_menu(move |mut menu, _, _| {
                for (id, name, count) in &folders {
                    let target_weak = weak.clone();
                    let tab_id = tab_id.clone();
                    let id = id.clone();
                    let name = name.clone();
                    let count = *count;
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        let id = id.clone();
                        let name = name.clone();
                        let _ = target_weak.update(cx, |view, cx| {
                            view.pick_load_test_target(
                                &tab_id,
                                LoadTestTarget::Collection(id),
                                name,
                                count,
                                cx,
                            );
                        });
                    }));
                }
                menu
            })
            .into_any_element()
    } else {
        let requests = requests_flat(tree);
        let tab_id = tab_id.to_string();
        trigger
            .dropdown_menu(move |mut menu, _, _| {
                for (id, label) in &requests {
                    let target_weak = weak.clone();
                    let tab_id = tab_id.clone();
                    let id = id.clone();
                    let label = label.clone();
                    menu =
                        menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                            let id = id.clone();
                            let label = label.clone();
                            let _ = target_weak.update(cx, |view, cx| {
                                view.pick_load_test_target(
                                    &tab_id,
                                    LoadTestTarget::Request(id),
                                    label,
                                    1,
                                    cx,
                                );
                            });
                        }));
                }
                menu
            })
            .into_any_element()
    }
}

/// `"1 request"` or `"N requests"`, in the current language.
fn request_count_label(count: usize) -> String {
    locale::plural("load_test.config.requests", count)
}

/// One numeric field row: a label (with an optional muted hint on the right), and a bordered pill
/// with the input and its unit suffix.
#[allow(clippy::too_many_arguments)]
fn render_field(
    palette: &Palette,
    mono_font: &SharedString,
    ui_font: &SharedString,
    label: String,
    hint: Option<String>,
    entity: Option<&Entity<InputState>>,
    unit: &'static str,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    let mut row = h_flex().justify_between().text_size(px(12.0)).child(
        div()
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette.fg_muted)
            .child(label),
    );
    if let Some(hint) = hint {
        row = row.child(div().text_color(palette.fg_subtle).child(hint));
    }

    let mut pill = h_flex()
        .h(px(CONTROL_HEIGHT))
        .items_center()
        .px(px(10.0))
        .rounded(px(RADIUS_MD - 1.0))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.raised)
        .font_family(mono_font.clone())
        .text_size(px(12.5));
    if let Some(entity) = entity {
        pill = pill.child(
            Input::new(entity)
                .context_menu(edit_menu(entity, cx))
                .flex_1()
                .h(px(CONTROL_HEIGHT))
                .bordered(false)
                .disabled(disabled)
                .bg(palette.bg.opacity(0.0)),
        );
    }
    // The interface font, not `mono_font` (`Performance.dc.html` draws the unit suffix in the UI
    // font while the value itself is mono, and `pill`'s own `font_family` above would otherwise
    // cascade to this child too).
    pill = pill.child(
        div()
            .font_family(ui_font.clone())
            .text_size(px(12.0))
            .text_color(palette.fg_subtle)
            .child(unit),
    );

    v_flex()
        .gap(px(6.0))
        .child(row)
        .child(pill)
        .into_any_element()
}

/// The "Stop on errors" switch row.
fn render_stop_on_error_row(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
    is_running: bool,
) -> AnyElement {
    let tab_id = tab_id.to_string();
    h_flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .when(is_running, |row| row.opacity(0.5))
        .child(
            v_flex()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(t!("load_test.config.stop_on_errors")),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(t!("load_test.config.stop_on_errors_hint")),
                ),
        )
        .child(
            Switch::new("load-test-stop-on-error")
                .checked(load_test.config.stop_on_error)
                .on_click(move |checked, _, cx| {
                    let checked = *checked;
                    let tab_id = tab_id.clone();
                    let _ = weak.update(cx, |view, cx| {
                        view.set_load_test_stop_on_error(&tab_id, checked, cx);
                    });
                }),
        )
        .into_any_element()
}

/// The primary "Start run" button, replaced by a danger-text "Stop run" secondary button while
/// running (`plans/ui-redesign.md` phase 8 item 3).
fn render_start_stop_button(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    is_running: bool,
) -> AnyElement {
    let tab_id = tab_id.to_string();
    if is_running {
        let stop_id = tab_id.clone();
        return DangerButton::new("load-test-stop", t!("load_test.config.stop_run"))
            .on_click(move |_, _, cx| {
                let stop_id = stop_id.clone();
                let _ = weak.update(cx, |view, cx| {
                    view.stop_load_test(&stop_id);
                    cx.notify();
                });
            })
            .into_any_element();
    }
    PrimaryButton::new("load-test-start", t!("load_test.config.start_run"))
        .on_click(move |_, _, cx| {
            let tab_id = tab_id.clone();
            let _ = weak.update(cx, |view, cx| view.start_load_test(tab_id, cx));
        })
        .into_any_element()
}
