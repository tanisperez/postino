//! The Summary tab of the load test results: the KPIs (with the change against the compared run
//! and the threshold goals), the "Performance over time" chart (hover, legend toggles and window
//! selection), the latency distribution, the responses by status and the per request table.

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::Method;
use postino_load::{LoadSnapshot, StatusKey, Threshold, ThresholdMetric};

use crate::state::load_results::{
    ChartSeriesKey, SummaryData, format_bucket_width, format_latency_ms, ms_axis, rps_axis,
    second_range_label, x_labels,
};
use crate::state::load_test::{
    DeltaTone, LoadTestTab, delta_tone, format_delta, format_trimmed, threshold_metric_label,
};
use crate::state::number::{format_decimal, format_integer};
use crate::theme::metrics::RADIUS_LG;
use crate::theme::{Palette, PaletteExt, PostinoColors};
use crate::views::components::{
    AxisSide, Card, ChartAxis, ChartSeries, DataCell, DataColumn, DataTable, MethodBadge,
    ReferenceLine, SeriesStyle, TimeSeriesChart,
};
use crate::views::root::AppView;

/// Height of the chart, from the design (a 188 px plot and its labels).
const CHART_HEIGHT: f32 = 220.0;
/// Height of the latency distribution's bars.
const HISTOGRAM_HEIGHT: f32 = 110.0;
/// Base width of one half of the distribution and statuses row: two that do not fit side by side
/// wrap, by flex layout, instead of squashing.
const TWO_COLUMN_MIN_WIDTH: f32 = 320.0;
/// Minimum width of one KPI cell; the two groups of three wrap as whole groups.
const KPI_CELL_MIN_WIDTH: f32 = 96.0;
/// A per request error rate above which its cell turns `warning` (1 %).
const PER_REQUEST_WARNING_ERROR_RATE: f64 = 0.01;
/// The virtual users area reaches this fraction of the chart's height at its peak, like the
/// design, so it never touches the top.
const VUS_HEIGHT_FRACTION: f64 = 0.9;

impl AppView {
    /// Renders the Summary tab of the load test tab `tab_id`.
    pub(crate) fn render_load_test_summary(
        &mut self,
        tab_id: &str,
        load_test: &LoadTestTab,
        snapshot: &LoadSnapshot,
        summary: &SummaryData,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let mono_font = cx.theme().mono_font_family.clone();
        let weak = cx.weak_entity();

        v_flex()
            .gap(px(14.0))
            .child(render_kpis(&palette, load_test, summary))
            .child(render_chart(weak, tab_id, &palette, load_test, snapshot))
            .child(
                h_flex()
                    .min_w_0()
                    .flex_wrap()
                    .gap(px(14.0))
                    .items_start()
                    .child(
                        div()
                            .flex_grow(1.3)
                            .flex_basis(px(TWO_COLUMN_MIN_WIDTH))
                            .min_w_0()
                            .child(render_distribution(
                                &palette, &mono_font, load_test, summary,
                            )),
                    )
                    .child(
                        div()
                            .flex_grow(1.0)
                            .flex_basis(px(TWO_COLUMN_MIN_WIDTH))
                            .min_w_0()
                            .child(render_statuses(&palette, &mono_font, summary)),
                    ),
            )
            .child(render_per_request(&palette, &mono_font, load_test, summary))
            .into_any_element()
    }
}

/// The threshold of `metric` the run shown is checked against, if any.
fn threshold_for(load_test: &LoadTestTab, metric: ThresholdMetric) -> Option<Threshold> {
    load_test
        .run_thresholds
        .iter()
        .find(|threshold| threshold.metric == metric)
        .copied()
}

/// The goal line under a KPI ("goal < 50 ms") and whether the value meets it.
fn kpi_goal(threshold: &Threshold, value: f64) -> (String, bool) {
    let limit = match threshold.metric {
        ThresholdMetric::ErrorRate => format!("{} %", format_trimmed(threshold.limit * 100.0)),
        ThresholdMetric::Rps => format_trimmed(threshold.limit),
        _ => format!("{} ms", format_trimmed(threshold.limit)),
    };
    let text = t!(
        "load_test.kpi.goal",
        limit = format!("{} {limit}", threshold.metric.operator())
    )
    .into_owned();
    (text, threshold.metric.passes(value, threshold.limit))
}

/// One KPI cell's content.
struct Kpi {
    label: String,
    value: String,
    unit: &'static str,
    delta: Option<(String, DeltaTone)>,
    goal: Option<(String, bool)>,
}

/// The change from `previous` to `current` as a KPI delta: a percent change, or percentage points
/// for the error rate; `None` when there is nothing to compare against.
fn kpi_delta(
    previous: Option<f64>,
    current: f64,
    higher_is_better: bool,
    points: bool,
) -> Option<(String, DeltaTone)> {
    let previous = previous?;
    let change = if points {
        (current - previous) * 100.0
    } else if previous.abs() < f64::EPSILON {
        return None;
    } else {
        (current - previous) / previous * 100.0
    };
    let unit = if points { " pt" } else { " %" };
    Some((
        format_delta(change, unit),
        delta_tone(change, higher_is_better),
    ))
}

/// The six KPIs: Requests/s, p50, p95, p99, Errors and Total, each with its delta against the
/// compared run and its threshold goal.
fn render_kpis(palette: &Palette, load_test: &LoadTestTab, summary: &SummaryData) -> AnyElement {
    let previous = load_test.compare_snapshot.as_ref();
    let ms = |us: u32| f64::from(us) / 1_000.0;
    let latency_kpi =
        |label: &str, metric: ThresholdMetric, current: u32, before: Option<u32>| Kpi {
            label: label.to_string(),
            value: format_decimal(ms(current), if current < 10_000 { 1 } else { 0 }),
            unit: "ms",
            delta: kpi_delta(before.map(ms), ms(current), false, false),
            goal: threshold_for(load_test, metric)
                .map(|threshold| kpi_goal(&threshold, ms(current))),
        };
    let kpis = [
        Kpi {
            label: t!("load_test.kpi.rps").into_owned(),
            value: format_integer(summary.rps.round() as u64),
            unit: "",
            delta: kpi_delta(previous.map(|p| p.rps), summary.rps, true, false),
            goal: threshold_for(load_test, ThresholdMetric::Rps)
                .map(|threshold| kpi_goal(&threshold, summary.rps)),
        },
        latency_kpi(
            "p50",
            ThresholdMetric::P50,
            summary.p50,
            previous.map(|p| p.p50),
        ),
        latency_kpi(
            "p95",
            ThresholdMetric::P95,
            summary.p95,
            previous.map(|p| p.p95),
        ),
        latency_kpi(
            "p99",
            ThresholdMetric::P99,
            summary.p99,
            previous.map(|p| p.p99),
        ),
        Kpi {
            label: t!("load_test.kpi.errors").into_owned(),
            value: format_decimal(summary.error_rate * 100.0, 1),
            unit: "%",
            delta: kpi_delta(
                previous.map(|p| p.error_rate),
                summary.error_rate,
                false,
                true,
            ),
            goal: threshold_for(load_test, ThresholdMetric::ErrorRate)
                .map(|threshold| kpi_goal(&threshold, summary.error_rate)),
        },
        Kpi {
            label: t!("load_test.kpi.total").into_owned(),
            value: format_integer(summary.total),
            unit: "",
            delta: kpi_delta(
                previous.map(|p| p.total as f64),
                summary.total as f64,
                true,
                false,
            ),
            goal: None,
        },
    ];
    let any_goal = kpis.iter().any(|kpi| kpi.goal.is_some());

    let mut cells = kpis.into_iter().enumerate();
    let mut groups: Vec<AnyElement> = Vec::new();
    for _ in 0..2 {
        let mut group = h_flex()
            .flex_1()
            .min_w(px(KPI_CELL_MIN_WIDTH * 3.0))
            .bg(palette.raised);
        for (index, kpi) in cells.by_ref().take(3) {
            let failing = kpi.goal.as_ref().is_some_and(|(_, met)| !met);
            let mut cell = v_flex()
                .flex_1()
                .min_w(px(KPI_CELL_MIN_WIDTH))
                .gap(px(4.0))
                .px(px(14.0))
                .py(px(12.0));
            if index % 3 != 0 {
                cell = cell.border_l_1().border_color(palette.border);
            }
            let mut value_row = h_flex()
                .items_baseline()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if failing { palette.danger } else { palette.fg })
                        .child(kpi.value),
                )
                .when(!kpi.unit.is_empty(), |row| {
                    row.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(palette.fg_subtle)
                            .child(kpi.unit),
                    )
                });
            if let Some((delta, tone)) = kpi.delta {
                let color = match tone {
                    DeltaTone::Better => palette.success,
                    DeltaTone::Worse => palette.danger,
                    DeltaTone::Neutral => palette.fg_subtle,
                };
                value_row = value_row.child(
                    div()
                        .ml_auto()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color)
                        .child(delta),
                );
            }
            cell = cell
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(palette.fg_muted)
                        .child(kpi.label),
                )
                .child(value_row);
            if any_goal {
                let mut goal_row = h_flex().h(px(16.0)).items_center().gap(px(4.0));
                if let Some((text, met)) = kpi.goal {
                    let color = if met { palette.success } else { palette.danger };
                    let icon = if met {
                        gpui_kit::assets::IconName::Check
                    } else {
                        gpui_kit::assets::IconName::X
                    };
                    goal_row = goal_row
                        .text_size(px(11.5))
                        .text_color(color)
                        .child(Icon::new(icon).with_size(px(11.0)))
                        .child(text);
                }
                cell = cell.child(goal_row);
            }
            group = group.child(cell);
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

/// The label of a chart series, as the legend and the tooltip show it.
fn series_label(key: ChartSeriesKey, load_test: &LoadTestTab) -> String {
    match key {
        ChartSeriesKey::Vus => "VUs".to_string(),
        ChartSeriesKey::Rps => t!("load_test.kpi.rps").into_owned(),
        ChartSeriesKey::P50 => "p50".to_string(),
        ChartSeriesKey::P90 => "p90".to_string(),
        ChartSeriesKey::P99 => "p99".to_string(),
        ChartSeriesKey::Baseline => format!(
            "{} \u{b7} p99",
            load_test
                .compare_with
                .map(super::dashboard::run_title)
                .unwrap_or_default()
        ),
    }
}

/// The "Performance over time" card: the title, the hint, the legend toggles and the chart.
fn render_chart(
    weak: WeakEntity<AppView>,
    tab_id: &str,
    palette: &Palette,
    load_test: &LoadTestTab,
    snapshot: &LoadSnapshot,
) -> AnyElement {
    let Some(data) = load_test.chart.clone() else {
        return div().into_any_element();
    };
    let selectable = !load_test.is_running() && load_test.log.is_some();
    let hint = if load_test.window.is_some() {
        t!("load_test.chart.hint_clear")
    } else {
        t!("load_test.chart.hint_drag")
    };

    let legend_keys: Vec<ChartSeriesKey> = ChartSeriesKey::LEGEND
        .into_iter()
        .filter(|key| *key != ChartSeriesKey::Baseline || data.baseline.is_some())
        .collect();
    let legend = legend_keys.iter().map(|key| {
        let key = *key;
        let hidden = load_test.hidden_series.contains(&key);
        let color = palette.series_color(key);
        let swatch = if key == ChartSeriesKey::Baseline {
            h_flex()
                .w(px(12.0))
                .gap(px(2.0))
                .child(div().w(px(5.0)).h(px(2.0)).bg(color))
                .child(div().w(px(5.0)).h(px(2.0)).bg(color))
        } else {
            h_flex().child(div().w(px(12.0)).h(px(2.0)).bg(color))
        };
        let item_weak = weak.clone();
        let tab_id = tab_id.to_string();
        h_flex()
            .id(SharedString::from(format!("load-test-legend-{key:?}")))
            .items_center()
            .gap(px(6.0))
            .h(px(22.0))
            .px(px(6.0))
            .rounded(px(5.0))
            .cursor_pointer()
            .text_color(palette.fg_muted)
            .when(hidden, |item| item.opacity(0.4))
            .hover(|style| style.bg(palette.hover))
            .on_click(move |_, _, cx| {
                let _ = item_weak.update(cx, |view, cx| {
                    view.toggle_load_test_series(&tab_id, key, cx);
                });
            })
            .child(swatch)
            .child(series_label(key, load_test))
    });
    let legend: Vec<_> = legend.collect();

    let elapsed_secs = snapshot.elapsed.as_secs();
    let duration_secs = load_test.shown_duration_secs().max(elapsed_secs).max(1);
    let count = (duration_secs as usize).max(data.rps.len()).max(2);
    let latency_limits: Vec<&Threshold> = load_test
        .run_thresholds
        .iter()
        .filter(|threshold| {
            matches!(
                threshold.metric,
                ThresholdMetric::P50 | ThresholdMetric::P95 | ThresholdMetric::P99
            )
        })
        .collect();
    let max_ms = latency_limits
        .iter()
        .map(|threshold| threshold.limit)
        .fold(data.max_ms, f64::max);
    let left = rps_axis(data.max_rps);
    let right = ms_axis(max_ms);

    let mut chart = TimeSeriesChart::new("load-test-chart", count)
        .height(CHART_HEIGHT)
        .surface(palette.raised)
        .left_axis(ChartAxis::new(left.max, left.labels).label_color(palette.accent_text))
        .right_axis(ChartAxis::new(right.max, right.labels))
        .x_labels(x_labels(duration_secs))
        .time_label(|index| second_range_label(index).into())
        .selection(load_test.window);
    for key in ChartSeriesKey::ALL {
        if load_test.hidden_series.contains(&key) {
            continue;
        }
        let color = palette.series_color(key);
        let label = series_label(key, load_test);
        let order = ChartSeriesKey::LEGEND
            .iter()
            .position(|legend| *legend == key)
            .unwrap_or(0) as i32;
        let series = match key {
            ChartSeriesKey::Vus => ChartSeries::new(label, color, data.vus.clone())
                .style(SeriesStyle::area(color.opacity(0.08), color.opacity(0.35)))
                .scale_max((data.max_vus / VUS_HEIGHT_FRACTION).max(1.0))
                .dot(false)
                .format(|value| format_integer(value.round() as u64).into()),
            ChartSeriesKey::Rps => ChartSeries::new(label, color, data.rps.clone())
                .style(SeriesStyle::line(2.25))
                .format(|value| format_integer(value.round().max(0.0) as u64).into()),
            ChartSeriesKey::P50 | ChartSeriesKey::P90 | ChartSeriesKey::P99 => {
                let values = match key {
                    ChartSeriesKey::P50 => data.p50.clone(),
                    ChartSeriesKey::P90 => data.p90.clone(),
                    _ => data.p99.clone(),
                };
                let width = if key == ChartSeriesKey::P99 {
                    1.75
                } else {
                    1.5
                };
                ChartSeries::new(label, color, values)
                    .side(AxisSide::Right)
                    .style(SeriesStyle::line(width))
                    .format(|value| format_latency_ms(value).into())
            }
            ChartSeriesKey::Baseline => {
                let Some(values) = data.baseline.clone() else {
                    continue;
                };
                ChartSeries::new(label, color.opacity(0.55), values)
                    .side(AxisSide::Right)
                    .style(SeriesStyle::dashed(1.5, 4.0, 4.0))
                    .dot(false)
                    .format(|value| format_latency_ms(value).into())
            }
        };
        chart = chart.series(series.tooltip_order(order));
    }
    for threshold in latency_limits {
        chart = chart.reference_line(ReferenceLine::new(
            AxisSide::Right,
            threshold.limit,
            t!(
                "load_test.chart.goal",
                metric = threshold_metric_label(threshold.metric),
                limit = format!("{} ms", format_trimmed(threshold.limit))
            ),
            palette.danger,
        ));
    }
    if selectable {
        let tab_id = tab_id.to_string();
        chart = chart.on_select(move |selection, _, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.set_load_test_window(&tab_id, selection, cx);
            });
        });
    }

    v_flex()
        .border_1()
        .border_color(palette.border)
        .rounded(px(RADIUS_LG))
        .bg(palette.raised)
        .px(px(16.0))
        .pt(px(12.0))
        .pb(px(10.0))
        .gap(px(8.0))
        .child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(14.0))
                .gap_y(px(4.0))
                .text_size(px(12.0))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(13.0))
                        .child(t!("load_test.chart.title")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(120.0))
                        .text_color(palette.fg_subtle)
                        .when(selectable, |hint_div| hint_div.child(hint)),
                )
                .children(legend),
        )
        .child(chart)
        .into_any_element()
}

/// The lowest latency limit among the run's thresholds, in milliseconds: distribution bars from
/// there on are drawn in `warning`.
fn lowest_latency_limit(load_test: &LoadTestTab) -> Option<f64> {
    load_test
        .run_thresholds
        .iter()
        .filter(|threshold| {
            matches!(
                threshold.metric,
                ThresholdMetric::P50 | ThresholdMetric::P95 | ThresholdMetric::P99
            )
        })
        .map(|threshold| threshold.limit)
        .reduce(f64::min)
}

/// The latency distribution: one bar per bucket, the y labels, and the x labels from 0 to the
/// open ended last bucket.
fn render_distribution(
    palette: &Palette,
    mono_font: &SharedString,
    load_test: &LoadTestTab,
    summary: &SummaryData,
) -> AnyElement {
    let (bucket_us, counts) = &summary.histogram;
    let max = counts.iter().copied().max().unwrap_or(0).max(1);
    let warn_from = lowest_latency_limit(load_test);
    let buckets = counts.len().saturating_sub(1).max(1) as u32;
    let span_ms = f64::from(*bucket_us) * f64::from(buckets) / 1_000.0;
    let in_seconds = span_ms >= 1_000.0;
    let x_label = |fraction: f64, last: bool| {
        let ms = span_ms * fraction;
        let (value, unit) = if in_seconds {
            (format_trimmed(ms / 1_000.0), "s")
        } else {
            (format_trimmed(ms), "ms")
        };
        if last {
            format!("{value}+ {unit}")
        } else {
            value
        }
    };

    Card::new()
        .child(
            h_flex()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(t!("load_test.distribution.title")),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(palette.fg_subtle)
                        .child(t!(
                            "load_test.distribution.caption",
                            width = format_bucket_width(*bucket_us)
                        )),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .w(px(40.0))
                        .h(px(HISTOGRAM_HEIGHT))
                        .justify_between()
                        .items_end()
                        .font_family(mono_font.clone())
                        .text_size(px(10.5))
                        .text_color(palette.fg_subtle)
                        .child(format_integer(max))
                        .child(format_integer(max / 2))
                        .child("0"),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .h(px(HISTOGRAM_HEIGHT))
                        .items_end()
                        .gap(px(3.0))
                        .border_b_1()
                        .border_color(palette.border)
                        .children(counts.iter().enumerate().map(|(index, count)| {
                            let fraction = (*count as f32 / max as f32)
                                .clamp(if *count > 0 { 0.03 } else { 0.0 }, 1.0);
                            let start_ms = f64::from(*bucket_us) * index as f64 / 1_000.0;
                            let color = if warn_from.is_some_and(|limit| start_ms >= limit) {
                                palette.warning
                            } else {
                                palette.accent_text
                            };
                            div()
                                .flex_1()
                                .h(relative(fraction))
                                .rounded_t(px(3.0))
                                .bg(color)
                        })),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .pl(px(48.0))
                .justify_between()
                .font_family(mono_font.clone())
                .text_size(px(10.5))
                .text_color(palette.fg_subtle)
                .children((0..=4).map(|step| div().child(x_label(step as f64 / 4.0, step == 4)))),
        )
        .into_any_element()
}

/// The color of one status' bar and label: the HTTP status range colors, `danger` for a timeout
/// or a failure.
pub(crate) fn status_key_color(palette: &Palette, status: &StatusKey) -> Hsla {
    match status {
        StatusKey::Code(code) => palette.status_colors(Some(*code)).0,
        StatusKey::Timeout | StatusKey::Failed => palette.danger,
    }
}

/// The label of one status: the code, or the translated "Timeout" or "Failed".
pub(crate) fn status_key_label(status: &StatusKey) -> String {
    match status {
        StatusKey::Code(code) => code.to_string(),
        StatusKey::Timeout => t!("load_test.status_key.timeout").into_owned(),
        StatusKey::Failed => t!("load_test.status_key.failed").into_owned(),
    }
}

/// "Responses by status": one row per status with a bar and its share.
fn render_statuses(
    palette: &Palette,
    mono_font: &SharedString,
    summary: &SummaryData,
) -> AnyElement {
    let total = summary.total.max(1) as f64;

    Card::new()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(t!("load_test.status_breakdown.title")),
        )
        .children(summary.statuses.iter().map(|(status, count)| {
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
                                .w(relative(fraction.max(0.01) as f32))
                                .rounded(px(4.0))
                                .bg(color),
                        ),
                )
                .child(
                    div()
                        .w(px(64.0))
                        .text_right()
                        .text_color(palette.fg_muted)
                        .child(format!("{} %", format_decimal(fraction * 100.0, 1))),
                )
        }))
        .into_any_element()
}

/// Each target's HTTP method and display name, by target index: a generic "Target N" for a run
/// saved before the labels were kept.
pub(crate) fn target_row_label(load_test: &LoadTestTab, index: usize) -> (Option<Method>, String) {
    match load_test.target_rows.get(index) {
        Some((method, name)) => (Some(method.clone()), name.clone()),
        None => (
            None,
            t!("load_test.table.target_n", number = index + 1).into_owned(),
        ),
    }
}

/// The per request table: request, Total, p50, p95, p99 and Errors.
fn render_per_request(
    palette: &Palette,
    mono_font: &SharedString,
    load_test: &LoadTestTab,
    summary: &SummaryData,
) -> AnyElement {
    let ms = |us: u32| format_latency_ms(f64::from(us) / 1_000.0);
    let mut table = DataTable::new("load-test-per-request-table")
        .column(DataColumn::new(t!("load_test.table.per_request")))
        .column(DataColumn::numeric(t!("load_test.kpi.total")))
        .column(DataColumn::numeric("p50"))
        .column(DataColumn::numeric("p95"))
        .column(DataColumn::numeric("p99"))
        .column(DataColumn::numeric(t!("load_test.kpi.errors")));
    for (index, stats) in summary.per_target.iter().enumerate() {
        let (method, name) = target_row_label(load_test, index);
        let errors = DataCell::text(format!("{} %", format_decimal(stats.error_rate * 100.0, 1)));
        let errors = if stats.error_rate > PER_REQUEST_WARNING_ERROR_RATE {
            errors.color(palette.warning)
        } else {
            errors
        };
        table = table.row([
            DataCell::element(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.0))
                    .children(method.map(MethodBadge::label))
                    .child(div().min_w_0().truncate().child(name)),
            ),
            DataCell::text(format_integer(stats.count)),
            DataCell::text(ms(stats.p50)),
            DataCell::text(ms(stats.p95)),
            DataCell::text(ms(stats.p99)),
            errors,
        ]);
    }
    // The table's own `mono` would use telaio's monospace font; the user's choice is inherited
    // from here instead, like the rest of the dashboard.
    div()
        .font_family(mono_font.clone())
        .child(table)
        .into_any_element()
}
