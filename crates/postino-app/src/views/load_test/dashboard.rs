//! The load test tab's right dashboard: the empty state and history list before a first run, and,
//! once a snapshot exists, the header, KPI strip, throughput/latency chart, latency distribution,
//! status breakdown, per-request table and "Compare with" panel.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::LineChart;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::Method;
use postino_load::history::RunRecordHeader;
use postino_load::{LoadSnapshot, StatusKey};

use crate::state::format::relative_day;
use crate::state::load_test::{
    DeltaTone, LoadTestFailure, LoadTestStatus, LoadTestTab, delta_tone, format_delta,
};
use crate::state::number::{format_decimal, format_integer};
use crate::theme::metrics::RADIUS_LG;
use crate::theme::{Palette, PaletteExt};
use crate::views::components::{Card, InlineMessage, InlineMessageKind, MethodBadge};

use super::run::unix_now;
use crate::views::root::AppView;

/// Height of the throughput/latency chart's plot area, and of the latency distribution's bars.
const CHART_HEIGHT: f32 = 150.0;
const HISTOGRAM_HEIGHT: f32 = 110.0;
/// A latency, in milliseconds, at or above which a latency distribution bar (and a per-request
/// error rate) switches from `accent_text`/normal to `warning`.
const HISTOGRAM_WARNING_MS: usize = 110;
/// The histogram bucket width, in milliseconds (`postino_load::LoadSnapshot::histogram`'s docs).
const HISTOGRAM_BUCKET_MS: usize = 10;
/// Base width (flex basis) of one half of a two-column dashboard row (latency distribution + status
/// breakdown, per-request table + compare with): when two do not fit side by side, `flex_wrap` (not
/// a window-width measurement) stacks them vertically instead of squashing them, and each then
/// grows to the row's width. A basis rather than a minimum width, so a pane narrower than this
/// still shrinks the column instead of overflowing.
const TWO_COLUMN_MIN_WIDTH: f32 = 320.0;
/// A per-request error rate at or above which its "Errors" cell turns `warning`
/// (1%).
const PER_REQUEST_WARNING_ERROR_RATE: f64 = 0.01;

impl AppView {
    /// Renders the dashboard for the load test tab `tab_id`: the empty state (and history) before
    /// a first run, or the full metrics view once a snapshot exists.
    pub(crate) fn render_load_test_dashboard(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(load_test) = self
            .state
            .tabs
            .active()
            .and_then(|tab| tab.load_test())
            .cloned()
        else {
            return div().flex_1().into_any_element();
        };
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let weak = cx.weak_entity();

        let Some(snapshot) = load_test.snapshot.clone() else {
            return render_empty_state(weak, tab_id, &palette, &load_test);
        };

        v_flex()
            .id("load-test-dashboard")
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_y_scrollbar()
            .px(px(20.0))
            .py(px(16.0))
            .gap(px(16.0))
            .child(render_header(&palette, &mono_font, &load_test, &snapshot))
            .child(render_kpi_strip(&palette, &snapshot))
            .child(render_throughput_card(
                &palette, &mono_font, &snapshot, &load_test,
            ))
            .child(
                h_flex()
                    .min_w_0()
                    .flex_wrap()
                    .gap(px(16.0))
                    .items_start()
                    .child(
                        div()
                            .flex_grow(1.3)
                            .flex_basis(px(TWO_COLUMN_MIN_WIDTH))
                            .min_w_0()
                            .child(render_latency_distribution(&palette, &snapshot)),
                    )
                    .child(
                        div()
                            .flex_grow(1.0)
                            .flex_basis(px(TWO_COLUMN_MIN_WIDTH))
                            .min_w_0()
                            .child(render_status_breakdown(&palette, &mono_font, &snapshot)),
                    ),
            )
            .child(render_bottom_row(
                weak, tab_id, &palette, &mono_font, &load_test, &snapshot,
            ))
            .into_any_element()
    }
}

/// The empty state before a first run: an explanatory placeholder, plus a Failed run's reason, plus
/// the target's run history if any (`click to view` is wired by the history rows' own click
/// handler, see [`render_history_list`]).
fn render_empty_state(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
) -> AnyElement {
    let mut column = v_flex()
        .id("load-test-empty-state")
        .flex_1()
        .h_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scrollbar()
        .px(px(20.0))
        .py(px(16.0))
        .gap(px(16.0));

    if let LoadTestStatus::Failed(failure) = &load_test.status {
        column = column.child(InlineMessage::new(
            InlineMessageKind::Danger,
            failure_message(failure),
        ));
    }

    column = column.child(
        div()
            .py(px(40.0))
            .text_center()
            .text_color(palette.fg_subtle)
            .child(t!("load_test.dashboard.empty")),
    );

    if !load_test.history.is_empty() {
        column = column.child(render_history_list(weak, tab_id, palette, load_test));
    }

    column.into_any_element()
}

/// The text shown for a run that could not be started, in the current language.
fn failure_message(failure: &LoadTestFailure) -> String {
    match failure {
        LoadTestFailure::NoWorkspace => t!("load_test.failure.no_workspace").into_owned(),
        LoadTestFailure::TabClosed => t!("load_test.failure.tab_closed").into_owned(),
        LoadTestFailure::NoTarget => t!("load_test.failure.no_target").into_owned(),
        LoadTestFailure::EmptyFolder => t!("load_test.failure.empty_folder").into_owned(),
        LoadTestFailure::Load(error) => t!("load_test.failure.load", error = error).into_owned(),
    }
}

/// `"Run #N"`, the number only.
fn run_title(number: u32) -> String {
    t!("load_test.run.numbered", number = number).into_owned()
}

/// `"Run #N \u{b7} today"`, for the history list and the "Compare with" dropdown.
fn run_with_day(number: u32, started_at_unix: u64, now: i64) -> String {
    format!(
        "{} \u{b7} {}",
        run_title(number),
        relative_day(started_at_unix as i64, now)
    )
}

/// The history list of previous runs for this tab's target, click to view.
fn render_history_list(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
) -> AnyElement {
    let now = unix_now() as i64;

    Card::new()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("load_test.history.title")),
        )
        .children(load_test.history.iter().map(|header| {
            let number = header.number;
            let tab_id = tab_id.to_string();
            let row_weak = weak.clone();
            h_flex()
                .id(("load-test-history-row", number as usize))
                .h(px(30.0))
                .items_center()
                .gap(px(10.0))
                .cursor_pointer()
                .rounded(px(6.0))
                .px(px(6.0))
                .hover(|style| style.bg(palette.hover))
                .child(div().flex_1().min_w_0().child(run_with_day(
                    number,
                    header.started_at_unix,
                    now,
                )))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(format!("{} req/s", format_decimal(header.rps, 0))),
                )
                .on_click(move |_, _window, cx| {
                    let _ = row_weak.update(cx, |view, cx| {
                        view.view_load_test_history(&tab_id, number, cx);
                    });
                })
        }))
        .into_any_element()
}

/// Minimum width of the header's progress bar, see [`render_header`].
const PROGRESS_MIN_WIDTH: f32 = 160.0;

/// The run header: "Run #N", the status badge, the progress bar, and `mm:ss / mm:ss` elapsed.
fn render_header(
    palette: &Palette,
    mono_font: &SharedString,
    load_test: &LoadTestTab,
    snapshot: &LoadSnapshot,
) -> AnyElement {
    let run_label = load_test
        .run_number
        .map(run_title)
        .unwrap_or_else(|| t!("load_test.run.plain").into_owned());
    let (badge_label, badge_fg, badge_bg) = status_badge_colors(palette, &load_test.status);
    let duration_secs = load_test.config.resolved().duration_secs;
    let total = Duration::from_secs(duration_secs);
    let progress = if duration_secs > 0 {
        (snapshot.elapsed.as_secs_f64() / duration_secs as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    h_flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(12.0))
        .gap_y(px(8.0))
        .child(
            div()
                .flex_none()
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(run_label),
        )
        .child(
            h_flex()
                .flex_none()
                .items_center()
                .gap(px(6.0))
                .h(px(22.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .bg(badge_bg)
                .text_color(badge_fg)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().size(px(6.0)).rounded_full().bg(badge_fg))
                .child(badge_label),
        )
        .child(
            // With a minimum width, a narrow dashboard wraps the bar and the elapsed time onto
            // a second line instead of squeezing the bar to a stub.
            div()
                .flex_1()
                .min_w(px(PROGRESS_MIN_WIDTH))
                .h(px(6.0))
                .rounded(px(3.0))
                .bg(palette.surface)
                .border_1()
                .border_color(palette.border)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(progress as f32))
                        .bg(palette.accent),
                ),
        )
        .child(
            div()
                .flex_none()
                .font_family(mono_font.clone())
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(format!(
                    "{} / {}",
                    format_mmss(snapshot.elapsed),
                    format_mmss(total)
                )),
        )
        .into_any_element()
}

/// The status badge's label and colors, : running in
/// accent, finished in success, stopped in warning, failed in danger.
fn status_badge_colors(palette: &Palette, status: &LoadTestStatus) -> (String, Hsla, Hsla) {
    let (label, fg, bg) = match status {
        LoadTestStatus::NotStarted => (
            t!("load_test.status.not_started"),
            palette.fg_muted,
            palette.hover,
        ),
        LoadTestStatus::Running => (
            t!("load_test.status.running"),
            palette.accent_text,
            palette.accent_subtle,
        ),
        LoadTestStatus::Finished => (
            t!("load_test.status.finished"),
            palette.success,
            palette.success_subtle,
        ),
        LoadTestStatus::Stopped => (
            t!("load_test.status.stopped"),
            palette.warning,
            palette.warning_subtle,
        ),
        LoadTestStatus::Failed(_) => (
            t!("load_test.status.failed"),
            palette.danger,
            palette.danger_subtle,
        ),
    };
    (label.into_owned(), fg, bg)
}

/// `"mm:ss"`.
fn format_mmss(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// The display name of a "Compare with" row, given the engine's English label
/// ([`postino_load::Delta::label`]), which stays the key for the formatting below.
fn compare_label(label: &str) -> String {
    match label {
        "Requests/s" => t!("load_test.kpi.rps").into_owned(),
        "Errors" => t!("load_test.kpi.errors").into_owned(),
        other => other.to_string(),
    }
}

/// Formats one "Compare with" cell's raw value per its [`postino_load::Delta::label`]: latency
/// labels ("p95", "p99") are microseconds, converted to whole milliseconds with a unit; "Errors"
/// is a `0.0..=1.0` fraction, shown as a percentage; anything else (`"Requests/s"`) is shown as
/// given.
fn format_compare_value(label: &str, value: f64) -> String {
    match label {
        "p95" | "p99" => format!("{} ms", format_decimal(value / 1000.0, 0)),
        "Errors" => format!("{}%", format_decimal(value * 100.0, 1)),
        _ => format_decimal(value, 0),
    }
}

/// Minimum width of one KPI cell. A group of three needs three of them, so the two groups of
/// [`render_kpi_strip`] wrap as whole groups (3 + 3, never 4 + 2) by flex layout alone, rather
/// than by measuring the window.
const KPI_CELL_MIN_WIDTH: f32 = 96.0;

/// The KPI strip: Requests/s, p50, p95, p99, Errors, Total. Two groups of three cells: side by side
/// in a single row when wide, stacked as 3 + 3 when narrow. The container paints the `border` color
/// through a 1 px gap between the groups (and its own 1 px border), so the separator is right in
/// both layouts.
fn render_kpi_strip(palette: &Palette, snapshot: &LoadSnapshot) -> AnyElement {
    let kpis: [(String, String, &str, bool); 6] = [
        (
            t!("load_test.kpi.rps").into_owned(),
            format_decimal(snapshot.rps, 0),
            "",
            false,
        ),
        (
            "p50".into(),
            format_integer(snapshot.p50 / 1000),
            "ms",
            false,
        ),
        (
            "p95".into(),
            format_integer(snapshot.p95 / 1000),
            "ms",
            false,
        ),
        (
            "p99".into(),
            format_integer(snapshot.p99 / 1000),
            "ms",
            false,
        ),
        (
            t!("load_test.kpi.errors").into_owned(),
            format_decimal(snapshot.error_rate * 100.0, 1),
            "%",
            snapshot.error_rate > 0.0,
        ),
        (
            t!("load_test.kpi.total").into_owned(),
            format_integer(snapshot.total),
            "",
            false,
        ),
    ];

    let mut cells = kpis.into_iter().enumerate();
    let mut groups: Vec<AnyElement> = Vec::new();
    for _ in 0..2 {
        let mut group = h_flex()
            .flex_1()
            .min_w(px(KPI_CELL_MIN_WIDTH * 3.0))
            .bg(palette.raised);
        for (index, (label, value, unit, warn)) in cells.by_ref().take(3) {
            let mut cell = v_flex()
                .flex_1()
                .min_w(px(KPI_CELL_MIN_WIDTH))
                .gap(px(4.0))
                .px(px(14.0))
                .py(px(12.0));
            if index % 3 != 0 {
                cell = cell.border_l_1().border_color(palette.border);
            }
            group = group.child(
                cell.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_muted)
                        .child(label),
                )
                .child(
                    h_flex()
                        .items_baseline()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(22.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if warn { palette.warning } else { palette.fg })
                                .child(value),
                        )
                        .when(!unit.is_empty(), |row| {
                            row.child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(palette.fg_subtle)
                                    .child(unit),
                            )
                        }),
                ),
            );
        }
        groups.push(group.into_any_element());
    }

    h_flex()
        .flex_wrap()
        .gap(px(1.0))
        .border_1()
        .border_color(palette.border)
        .bg(palette.border)
        .rounded(px(RADIUS_LG))
        .overflow_hidden()
        .children(groups)
        .into_any_element()
}

/// The throughput and latency card: two polylines (Requests/s in `accent_text`, p95 in
/// `warning`), each normalized to its own max (`LineChart`'s y-scale always starts at 0 and fits
/// its own data, see `docs/gpui-notes.md`), four horizontal grid lines, and time
/// labels from `0` to the run's configured duration.
fn render_throughput_card(
    palette: &Palette,
    mono_font: &SharedString,
    snapshot: &LoadSnapshot,
    load_test: &LoadTestTab,
) -> AnyElement {
    let duration_secs = load_test.config.resolved().duration_secs;
    let rps_data: Vec<(usize, f64)> = snapshot
        .series
        .iter()
        .enumerate()
        .map(|(index, point)| (index, point.rps))
        .collect();
    let p95_data: Vec<(usize, f64)> = snapshot
        .series
        .iter()
        .enumerate()
        .map(|(index, point)| (index, f64::from(point.p95) / 1000.0))
        .collect();

    let chart = div()
        .relative()
        .size_full()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .flex_col()
                .justify_between()
                .children((0..4).map(|_| div().h(px(1.0)).w_full().bg(palette.border))),
        )
        .child(
            div().absolute().top_0().left_0().size_full().child(
                LineChart::new(rps_data)
                    .x(|point: &(usize, f64)| SharedString::from(point.0.to_string()))
                    .y(|point: &(usize, f64)| point.1)
                    .stroke(palette.accent_text)
                    .x_axis(false)
                    .grid(false),
            ),
        )
        .child(
            div().absolute().top_0().left_0().size_full().child(
                LineChart::new(p95_data)
                    .x(|point: &(usize, f64)| SharedString::from(point.0.to_string()))
                    .y(|point: &(usize, f64)| point.1)
                    .stroke(palette.warning)
                    .x_axis(false)
                    .grid(false),
            ),
        );

    Card::new()
        .child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(16.0))
                .gap_y(px(4.0))
                .text_size(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(170.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(13.0))
                        .child(t!("load_test.chart.title")),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .gap(px(16.0))
                        .child(legend_item(
                            palette,
                            palette.accent_text,
                            t!("load_test.kpi.rps").into_owned(),
                        ))
                        .child(legend_item(palette, palette.warning, "p95 ms".to_string())),
                ),
        )
        .child(div().h(px(CHART_HEIGHT)).w_full().child(chart))
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .font_family(mono_font.clone())
                .text_size(px(11.0))
                .text_color(palette.fg_subtle)
                .children((0..=4).map(|step| {
                    div().child(format!("{}s", format_integer(duration_secs * step / 4)))
                })),
        )
        .into_any_element()
}

/// One legend entry: a short colored line and a muted label.
fn legend_item(palette: &Palette, color: Hsla, label: String) -> AnyElement {
    h_flex()
        .items_center()
        .gap(px(6.0))
        .text_color(palette.fg_muted)
        .child(div().w(px(10.0)).h(px(2.0)).bg(color))
        .child(label)
        .into_any_element()
}

/// The latency distribution: 26 bars from the histogram, `accent_text`, bars at 110 ms and above in
/// `warning`.
fn render_latency_distribution(palette: &Palette, snapshot: &LoadSnapshot) -> AnyElement {
    let max = snapshot.histogram.iter().copied().max().unwrap_or(0).max(1);

    Card::new()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("load_test.distribution.title")),
        )
        .child(
            h_flex()
                .h(px(HISTOGRAM_HEIGHT))
                .items_end()
                .gap(px(3.0))
                .children(snapshot.histogram.iter().enumerate().map(|(index, count)| {
                    let fraction = (*count as f32 / max as f32)
                        .clamp(if *count > 0 { 0.03 } else { 0.0 }, 1.0);
                    let color = if index * HISTOGRAM_BUCKET_MS >= HISTOGRAM_WARNING_MS {
                        palette.warning
                    } else {
                        palette.accent_text
                    };
                    div()
                        .flex_1()
                        .h(relative(fraction))
                        .rounded(px(3.0))
                        .bg(color)
                })),
        )
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .text_size(px(11.0))
                .text_color(palette.fg_subtle)
                .child("0")
                .child("50")
                .child("100")
                .child("150")
                .child("200")
                .child("250+ ms"),
        )
        .into_any_element()
}

/// The color for one [`StatusKey`]'s bar and text, reusing
/// [`crate::theme::Palette::status_colors`]'s HTTP status range mapping for
/// [`StatusKey::Code`], and `danger` for a timeout or script/send failure.
fn status_key_color(palette: &Palette, status: &StatusKey) -> Hsla {
    match status {
        StatusKey::Code(code) => palette.status_colors(Some(*code)).0,
        StatusKey::Timeout | StatusKey::Failed => palette.danger,
    }
}

/// The display label for one [`StatusKey`] row.
fn status_key_label(status: &StatusKey) -> String {
    match status {
        StatusKey::Code(code) => code.to_string(),
        StatusKey::Timeout => t!("load_test.status_key.timeout").into_owned(),
        StatusKey::Failed => t!("load_test.status_key.failed").into_owned(),
    }
}

/// "Responses by status": one row per status key with a bar and a percentage.
fn render_status_breakdown(
    palette: &Palette,
    mono_font: &SharedString,
    snapshot: &LoadSnapshot,
) -> AnyElement {
    let total = snapshot.total.max(1) as f64;

    Card::new()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("load_test.status_breakdown.title")),
        )
        .children(snapshot.status_counts.iter().map(|(status, count)| {
            let color = status_key_color(palette, status);
            let fraction = (*count as f64 / total).clamp(0.0, 1.0);
            h_flex()
                .items_center()
                .gap(px(10.0))
                .text_size(px(12.0))
                .child(
                    div()
                        .w(px(62.0))
                        .font_family(mono_font.clone())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(status_key_label(status)),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(8.0))
                        .rounded(px(4.0))
                        .bg(palette.surface)
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(relative(fraction as f32))
                                .rounded(px(4.0))
                                .bg(color),
                        ),
                )
                .child(
                    div()
                        .w(px(56.0))
                        .text_right()
                        .text_color(palette.fg_muted)
                        .child(format!("{}%", format_decimal(fraction * 100.0, 1))),
                )
        }))
        .into_any_element()
}

/// The per-request table and "Compare with" panel, side by side (hidden if there is no previous
/// run for this target).
fn render_bottom_row(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    mono_font: &SharedString,
    load_test: &LoadTestTab,
    snapshot: &LoadSnapshot,
) -> AnyElement {
    let compare_available: Vec<&RunRecordHeader> = load_test
        .history
        .iter()
        .filter(|header| Some(header.number) != load_test.run_number)
        .collect();

    // The table's own minimum width as its basis, so the row wraps before the table has to
    // scroll sideways, which it only does when the pane itself is narrower than the table.
    let per_request = div()
        .flex_grow(1.3)
        .flex_basis(px(PER_REQUEST_TABLE_MIN_WIDTH))
        .min_w_0()
        .child(render_per_request_table(
            palette, mono_font, load_test, snapshot,
        ));

    if compare_available.is_empty() {
        return h_flex()
            .min_w_0()
            .flex_wrap()
            .gap(px(16.0))
            .items_start()
            .child(per_request)
            .into_any_element();
    }

    h_flex()
        .min_w_0()
        .flex_wrap()
        .gap(px(16.0))
        .items_start()
        .child(per_request)
        .child(
            div()
                .flex_grow(1.0)
                .flex_basis(px(TWO_COLUMN_MIN_WIDTH))
                .min_w_0()
                .child(render_compare_with(
                    weak,
                    tab_id,
                    palette,
                    load_test,
                    snapshot,
                    &compare_available,
                )),
        )
        .into_any_element()
}

/// Fixed width of one of the per-request table's numeric columns (Count, p50, p95, p99, Errors):
/// wide enough for their widest realistic value ("999 ms") without letting them shrink into each
/// other at a narrow window, which `flex_grow` alone (with no floor) allowed.
const PER_REQUEST_NUMERIC_COL_WIDTH: f32 = 64.0;
/// Horizontal gap between the per-request table's columns.
const PER_REQUEST_COL_GAP: f32 = 10.0;
/// Minimum width of the per-request table: its 16 px side padding, the 60 px name column and five
/// numeric columns with their gaps.
const PER_REQUEST_TABLE_MIN_WIDTH: f32 =
    2.0 * 16.0 + 60.0 + 5.0 * (PER_REQUEST_NUMERIC_COL_WIDTH + PER_REQUEST_COL_GAP);

/// The per-request table: method, name, Count, p50, p95, p99, Errors.
fn render_per_request_table(
    palette: &Palette,
    mono_font: &SharedString,
    load_test: &LoadTestTab,
    snapshot: &LoadSnapshot,
) -> AnyElement {
    let header = h_flex()
        .h(px(34.0))
        .items_center()
        .gap(px(PER_REQUEST_COL_GAP))
        .px(px(16.0))
        .border_b_1()
        .border_color(palette.border)
        .text_size(px(11.5))
        .text_color(palette.fg_subtle)
        .font_weight(FontWeight::MEDIUM)
        .child(div().flex_1().min_w(px(60.0)).child(t!("common.request")))
        .child(
            div()
                .flex_none()
                .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                .text_right()
                .child(t!("load_test.table.count")),
        )
        .child(
            div()
                .flex_none()
                .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                .text_right()
                .child("p50"),
        )
        .child(
            div()
                .flex_none()
                .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                .text_right()
                .child("p95"),
        )
        .child(
            div()
                .flex_none()
                .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                .text_right()
                .child("p99"),
        )
        .child(
            div()
                .flex_none()
                .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                .text_right()
                .child(t!("load_test.kpi.errors")),
        );

    let rows = snapshot
        .per_target
        .iter()
        .enumerate()
        .map(|(index, stats)| {
            let (method, name) = target_row_label(load_test, index);
            let error_color = if stats.error_rate > PER_REQUEST_WARNING_ERROR_RATE {
                palette.warning
            } else {
                palette.fg
            };
            h_flex()
                .h(px(34.0))
                .items_center()
                .gap(px(PER_REQUEST_COL_GAP))
                .px(px(16.0))
                .border_b_1()
                .border_color(palette.border)
                .text_size(px(12.5))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(60.0))
                        .items_center()
                        .gap(px(8.0))
                        .children(method.map(MethodBadge::label))
                        .child(div().min_w_0().truncate().child(name)),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                        .text_right()
                        .child(format_integer(stats.count)),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                        .text_right()
                        .child(format!("{} ms", format_integer(stats.p50 / 1000))),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                        .text_right()
                        .child(format!("{} ms", format_integer(stats.p95 / 1000))),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                        .text_right()
                        .child(format!("{} ms", format_integer(stats.p99 / 1000))),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(PER_REQUEST_NUMERIC_COL_WIDTH))
                        .text_right()
                        .text_color(error_color)
                        .child(format!("{}%", format_decimal(stats.error_rate * 100.0, 1))),
                )
        });

    // Below its minimum width the table scrolls sideways inside its card rather than pushing
    // the whole dashboard wider than its pane. Restricted to the x axis, so the wheel keeps
    // scrolling the dashboard vertically.
    v_flex()
        .border_1()
        .border_color(palette.border)
        .rounded(px(RADIUS_LG))
        .bg(palette.raised)
        .overflow_hidden()
        .font_family(mono_font.clone())
        .child(
            div()
                .id("load-test-per-request-table")
                .w_full()
                .restrict_scroll_to_axis()
                .overflow_x_scrollbar()
                .child(
                    v_flex()
                        .min_w(px(PER_REQUEST_TABLE_MIN_WIDTH))
                        .child(header)
                        .children(rows),
                ),
        )
        .into_any_element()
}

/// Each target's HTTP method and display name for the per-request table, in the same order as
/// [`LoadSnapshot::per_target`]. `None` method, `"Target N"` name for an index beyond
/// [`LoadTestTab::target_rows`]: a run saved before
/// [`postino_load::history::RunRecord::target_labels`] existed has no labels to restore (that
/// field defaults to empty for those older files), and a target list that no longer lines up
/// (e.g. the collection was edited after the run) degrades the same way, to a generic label
/// instead of a wrong one.
fn target_row_label(load_test: &LoadTestTab, index: usize) -> (Option<Method>, String) {
    match load_test.target_rows.get(index) {
        Some((method, name)) => (Some(method.clone()), name.clone()),
        None => (
            None,
            t!("load_test.table.target_n", number = index + 1).into_owned(),
        ),
    }
}

/// The "Compare with" panel: a dropdown of previous runs of the same target, and rows for
/// Requests/s, p95, p99 and Errors with previous, current and a colored delta.
fn render_compare_with(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
    snapshot: &LoadSnapshot,
    available: &[&RunRecordHeader],
) -> AnyElement {
    let now = unix_now() as i64;
    let selected = load_test
        .compare_with
        .and_then(|number| available.iter().find(|header| header.number == number));
    let trigger_label = selected
        .map(|header| run_with_day(header.number, header.started_at_unix, now))
        .unwrap_or_else(|| t!("load_test.compare.select").into_owned());

    let header_row = h_flex()
        .h(px(34.0))
        .items_center()
        .justify_between()
        .px(px(16.0))
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("load_test.compare.title")),
        )
        .child({
            let tab_id = tab_id.to_string();
            let available: Vec<RunRecordHeader> =
                available.iter().map(|header| (*header).clone()).collect();
            Button::new("load-test-compare-with")
                .ghost()
                .text_color(palette.fg_muted)
                .text_size(px(12.0))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(trigger_label)
                        .child(Icon::new(IconName::ChevronDown).small()),
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for header in &available {
                        let target_weak = weak.clone();
                        let tab_id = tab_id.clone();
                        let number = header.number;
                        let label = run_with_day(number, header.started_at_unix, now);
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            let _ = target_weak.update(cx, |view, cx| {
                                view.set_load_test_compare_with(&tab_id, Some(number), cx);
                            });
                        }));
                    }
                    menu
                })
        });

    let mut card = v_flex()
        .border_1()
        .border_color(palette.border)
        .rounded(px(RADIUS_LG))
        .bg(palette.raised)
        .overflow_hidden()
        .child(header_row);

    if let Some(previous) = &load_test.compare_snapshot {
        for delta in postino_load::compare(previous, snapshot) {
            let delta_color = match delta_tone(delta.change, delta.label == "Requests/s") {
                DeltaTone::Better => palette.success,
                DeltaTone::Worse => palette.danger,
                DeltaTone::Neutral => palette.fg_muted,
            };
            let unit = match delta.unit {
                postino_load::DeltaUnit::Percent => "%",
                postino_load::DeltaUnit::PercentagePoints => " pt",
            };
            card = card.child(
                h_flex()
                    .h(px(34.0))
                    .items_center()
                    .px(px(16.0))
                    .border_b_1()
                    .border_color(palette.border)
                    .text_size(px(12.5))
                    .child(
                        div()
                            .flex_1()
                            .text_color(palette.fg_muted)
                            .child(compare_label(delta.label)),
                    )
                    .child(
                        div()
                            .w(px(70.0))
                            .text_right()
                            .text_color(palette.fg_subtle)
                            .child(format_compare_value(delta.label, delta.previous)),
                    )
                    .child(
                        div()
                            .w(px(70.0))
                            .text_right()
                            .child(format_compare_value(delta.label, delta.current)),
                    )
                    .child(
                        div()
                            .w(px(64.0))
                            .text_right()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(delta_color)
                            .child(format_delta(delta.change, unit)),
                    ),
            );
        }
    }

    card.into_any_element()
}
