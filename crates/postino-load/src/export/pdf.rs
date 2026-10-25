//! [`to_pdf`], the PDF rendering of a [`Report`].
//!
//! An A4 portrait document on a white background, laid out top to bottom: title, verdict and
//! threshold chips, the KPI grid, the "Performance over time" chart, the latency histogram next to
//! the status bars, the per request table and the error groups. Everything is vector: the chart
//! and the histogram are drawn from the report's raw numbers, the check and cross marks are
//! small strokes, not font glyphs.
//!
//! The layout is done in two steps. `Layout` places every block and records plain drawing
//! operations (`Op`) page by page, breaking onto a new page when the next block does not fit
//! (tables break row by row and repeat their header). Once the page count is known, the footers
//! are added and `draw::render` replays the operations with `krilla`. As in the Markdown report,
//! nothing here formats or translates: text that does not fit its box is cut with an ellipsis.

use thiserror::Error;

mod draw;
mod text;

use draw::{Line, Op, Paint, Shape, render};
use text::{Fonts, Style, Weight, clean};

use super::report::{
    Report, ReportAlign, ReportAxis, ReportChart, ReportErrorGroup, ReportHistogram, ReportKpi,
    ReportSeriesStyle, ReportTone, Rgb,
};

/// The TrueType bytes of the four faces a report is set in. The app passes the Geist and Geist
/// Mono fonts it bundles.
#[derive(Debug, Clone, Copy)]
pub struct PdfFonts<'a> {
    /// The text face (Geist Regular).
    pub regular: &'a [u8],
    /// The slightly heavier face for labels and legends (Geist Medium).
    pub medium: &'a [u8],
    /// The heading and big number face (Geist SemiBold).
    pub semibold: &'a [u8],
    /// The monospace face for numbers (Geist Mono Regular).
    pub mono: &'a [u8],
}

/// Why a PDF could not be produced.
#[derive(Debug, Error)]
pub enum PdfError {
    /// A font could not be read. The payload names the face: `regular`, `medium`, `semibold` or
    /// `mono`.
    #[error("the {0} font could not be read")]
    Font(&'static str),
    /// The PDF writer failed.
    #[error("could not write the PDF: {0}")]
    Write(String),
}

/// Renders `report` as an A4 PDF, using `fonts` for every piece of text.
///
/// # Errors
///
/// [`PdfError::Font`] when one of the fonts is not a readable TrueType font, and
/// [`PdfError::Write`] when the PDF writer fails.
pub fn to_pdf(report: &Report, fonts: &PdfFonts<'_>) -> Result<Vec<u8>, PdfError> {
    let fonts = Fonts::new(fonts)?;
    let pages = layout(report, &fonts);
    render(&pages, &fonts, &report.title)
}

// Page geometry, in points (A4 portrait).
const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN: f32 = 40.0;
const LEFT: f32 = MARGIN;
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN;
const TOP: f32 = MARGIN;
/// Where content stops, above the footer.
const BOTTOM: f32 = PAGE_H - MARGIN - 12.0;
/// The footer text baseline.
const FOOTER_Y: f32 = PAGE_H - MARGIN + 2.0;
/// The space between two blocks.
const GAP: f32 = 14.0;

// The fixed light theme neutrals.
const TEXT: Rgb = Rgb(0x1b, 0x1c, 0x21);
const MUTED: Rgb = Rgb(0x5e, 0x62, 0x70);
const SUBTLE: Rgb = Rgb(0x8a, 0x8e, 0x9a);
const BORDER: Rgb = Rgb(0xe3, 0xe4, 0xe8);
const GRID: Rgb = Rgb(0xec, 0xee, 0xf1);
const SURFACE: Rgb = Rgb(0xf5, 0xf6, 0xf8);
const GOOD: Rgb = Rgb(0x1a, 0x8f, 0x5a);
const BAD: Rgb = Rgb(0xd2, 0x3f, 0x3f);
const WARN: Rgb = Rgb(0xb7, 0x79, 0x1f);
const ACCENT: Rgb = Rgb(0x5b, 0x63, 0xe6);

/// The color of a tone.
fn tone_color(tone: ReportTone) -> Rgb {
    match tone {
        ReportTone::Neutral => MUTED,
        ReportTone::Good => GOOD,
        ReportTone::Bad => BAD,
        ReportTone::Warn => WARN,
    }
}

/// The color of a pass or fail result.
fn pass_color(passed: bool) -> Rgb {
    if passed { GOOD } else { BAD }
}

/// Positions a row of items of the given widths, wrapping onto new rows when `max_w` is
/// exceeded. Returns the row and the x offset of each item, and the row count.
fn flow(widths: &[f32], max_w: f32, gap: f32) -> (Vec<(usize, f32)>, usize) {
    let mut places = Vec::with_capacity(widths.len());
    let (mut row, mut x) = (0, 0.0);
    for &w in widths {
        if x > 0.0 && x + w > max_w {
            row += 1;
            x = 0.0;
        }
        places.push((row, x));
        x += w + gap;
    }
    (places, if widths.is_empty() { 0 } else { row + 1 })
}

/// Lays `report` out into pages, footers included.
fn layout(report: &Report, fonts: &Fonts<'_>) -> Vec<Vec<Op>> {
    let mut l = Layout {
        fonts,
        pages: vec![Vec::new()],
        y: TOP,
    };
    l.header(report);
    l.chips(report);
    l.kpis(&report.kpis);
    if let Some(chart) = &report.chart {
        l.chart(chart);
    }
    l.distribution(report);
    l.per_request(report);
    l.errors(report);
    l.footers(report);
    l.pages
}

/// The page builder: the pages so far and where the next block starts.
struct Layout<'f, 'd> {
    fonts: &'f Fonts<'d>,
    pages: Vec<Vec<Op>>,
    y: f32,
}

impl Layout<'_, '_> {
    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = TOP;
    }

    /// Starts a new page unless `height` more points fit on this one.
    fn ensure(&mut self, height: f32) {
        if self.y + height > BOTTOM && self.y > TOP + 0.5 {
            self.new_page();
        }
    }

    fn push(&mut self, op: Op) {
        if let Some(page) = self.pages.last_mut() {
            page.push(op);
        }
    }

    fn width(&self, text: &str, weight: Weight, size: f32) -> f32 {
        self.fonts.face(weight).width(text, size)
    }

    /// Draws `text` with its baseline at `y`, cut to `max_w` points.
    fn text(&mut self, x: f32, y: f32, text: &str, style: Style, max_w: f32) {
        let text = self
            .fonts
            .face(style.weight)
            .fit(&clean(text), style.size, max_w);
        if !text.is_empty() {
            self.push(Op::Text { x, y, text, style });
        }
    }

    /// Like [`Self::text`], but ends at `right` instead of starting at a point.
    fn text_right(&mut self, right: f32, y: f32, text: &str, style: Style, max_w: f32) {
        let text = self
            .fonts
            .face(style.weight)
            .fit(&clean(text), style.size, max_w);
        let w = self.width(&text, style.weight, style.size);
        if !text.is_empty() {
            self.push(Op::Text {
                x: right - w,
                y,
                text,
                style,
            });
        }
    }

    /// Like [`Self::text`], centered on `center`.
    fn text_center(&mut self, center: f32, y: f32, text: &str, style: Style) {
        let w = self.width(&clean(text), style.weight, style.size);
        self.text(center - w / 2.0, y, text, style, f32::INFINITY);
    }

    fn rect(
        &mut self,
        (x, y, w, h): (f32, f32, f32, f32),
        r: f32,
        fill: Option<Paint>,
        stroke: Option<Line>,
    ) {
        self.push(Op::Shape {
            shape: Shape::Rect { x, y, w, h, r },
            fill,
            stroke,
        });
    }

    fn line(&mut self, from: (f32, f32), to: (f32, f32), stroke: Line) {
        self.push(Op::Shape {
            shape: Shape::Poly {
                pts: vec![from, to],
                closed: false,
            },
            fill: None,
            stroke: Some(stroke),
        });
    }

    fn poly(
        &mut self,
        pts: Vec<(f32, f32)>,
        closed: bool,
        fill: Option<Paint>,
        stroke: Option<Line>,
    ) {
        self.push(Op::Shape {
            shape: Shape::Poly { pts, closed },
            fill,
            stroke,
        });
    }

    /// A check mark in a square of side `s` at (`x`, `y`).
    fn check(&mut self, x: f32, y: f32, s: f32, color: Rgb) {
        let pts = vec![
            (x + s * 0.08, y + s * 0.55),
            (x + s * 0.38, y + s * 0.86),
            (x + s * 0.94, y + s * 0.14),
        ];
        self.poly(pts, false, None, Some(Line::solid(color, 1.3)));
    }

    /// A cross in a square of side `s` at (`x`, `y`).
    fn cross(&mut self, x: f32, y: f32, s: f32, color: Rgb) {
        let stroke = Line::solid(color, 1.3);
        self.line(
            (x + s * 0.15, y + s * 0.15),
            (x + s * 0.85, y + s * 0.85),
            stroke,
        );
        self.line(
            (x + s * 0.85, y + s * 0.15),
            (x + s * 0.15, y + s * 0.85),
            stroke,
        );
    }

    /// A check mark or a cross, by result.
    fn mark(&mut self, x: f32, y: f32, s: f32, passed: bool) {
        if passed {
            self.check(x, y, s, GOOD);
        } else {
            self.cross(x, y, s, BAD);
        }
    }

    /// A bordered, white rounded card.
    fn card(&mut self, bounds: (f32, f32, f32, f32)) {
        self.rect(bounds, 7.0, None, Some(Line::solid(BORDER, 0.75)));
    }

    // Title, subtitle, window and comparison lines.
    fn header(&mut self, report: &Report) {
        self.text(
            LEFT,
            self.y + 15.0,
            &report.title,
            Style::new(Weight::Semibold, 18.0, TEXT),
            CONTENT_W,
        );
        self.y += 24.0;
        self.text(
            LEFT,
            self.y + 8.0,
            &report.subtitle,
            Style::new(Weight::Regular, 9.5, MUTED),
            CONTENT_W,
        );
        self.y += 14.0;
        for line in report.window.iter().chain(report.comparison.iter()) {
            self.text(
                LEFT,
                self.y + 8.0,
                line,
                Style::new(Weight::Medium, 9.0, MUTED),
                CONTENT_W,
            );
            self.y += 13.0;
        }
        self.y += 10.0;
    }

    // The verdict chip and the threshold chips, wrapping onto more rows when needed.
    fn chips(&mut self, report: &Report) {
        const H: f32 = 20.0;
        const PAD: f32 = 9.0;
        const MARK: f32 = 8.0;
        const SIZE: f32 = 8.5;
        // Each chip: width, then what to draw. `None` for the verdict, the index otherwise.
        let mut chips: Vec<(f32, Option<usize>)> = Vec::new();
        let inner_max = CONTENT_W - 2.0 * PAD - MARK - 5.0;
        if let Some(verdict) = &report.verdict {
            let text = self
                .width(&clean(&verdict.text), Weight::Semibold, SIZE)
                .min(inner_max);
            chips.push((2.0 * PAD + MARK + 5.0 + text, None));
        }
        for (i, t) in report.thresholds.iter().enumerate() {
            let label = self.width(&clean(&t.label), Weight::Regular, SIZE);
            let value = self.width(&clean(&t.value), Weight::Mono, SIZE);
            let w = (2.0 * PAD + MARK + 5.0 + label + 6.0 + value).min(CONTENT_W);
            chips.push((w, Some(i)));
        }
        if chips.is_empty() {
            return;
        }
        let widths: Vec<f32> = chips.iter().map(|c| c.0).collect();
        let (places, rows) = flow(&widths, CONTENT_W, 6.0);
        let mut row_started = usize::MAX;
        for (&(w, which), &(row, dx)) in chips.iter().zip(&places) {
            if row != row_started {
                if row_started != usize::MAX {
                    self.y += H + 6.0;
                }
                self.ensure(H);
                row_started = row;
            }
            let (x, y) = (LEFT + dx, self.y);
            let mid = y + H / 2.0 + SIZE * 0.36;
            let inner = w - 2.0 * PAD - MARK - 5.0;
            match which {
                None => {
                    let Some(verdict) = &report.verdict else {
                        continue;
                    };
                    let color = pass_color(verdict.passed);
                    self.rect(
                        (x, y, w, H),
                        5.0,
                        Some((color, 0.1)),
                        Some(Line {
                            alpha: 0.35,
                            ..Line::solid(color, 0.75)
                        }),
                    );
                    self.mark(x + PAD, y + (H - MARK) / 2.0, MARK, verdict.passed);
                    self.text(
                        x + PAD + MARK + 5.0,
                        mid,
                        &verdict.text,
                        Style::new(Weight::Semibold, SIZE, color),
                        inner,
                    );
                }
                Some(i) => {
                    let Some(t) = report.thresholds.get(i) else {
                        continue;
                    };
                    let color = pass_color(t.passed);
                    self.rect((x, y, w, H), 5.0, None, Some(Line::solid(BORDER, 0.75)));
                    self.mark(x + PAD, y + (H - MARK) / 2.0, MARK, t.passed);
                    let value_w = self
                        .width(&clean(&t.value), Weight::Mono, SIZE)
                        .min(inner * 0.5);
                    let label_w = inner - value_w - 6.0;
                    let label_x = x + PAD + MARK + 5.0;
                    self.text(
                        label_x,
                        mid,
                        &t.label,
                        Style::new(Weight::Regular, SIZE, MUTED),
                        label_w,
                    );
                    let shown = self.width(
                        &self.fonts.regular.fit(&clean(&t.label), SIZE, label_w),
                        Weight::Regular,
                        SIZE,
                    );
                    self.text(
                        label_x + shown + 6.0,
                        mid,
                        &t.value,
                        Style::new(Weight::Mono, SIZE, color),
                        value_w,
                    );
                }
            }
        }
        self.y += H + GAP;
        let _ = rows;
    }

    // The KPI cells in one bordered row.
    fn kpis(&mut self, kpis: &[ReportKpi]) {
        if kpis.is_empty() {
            return;
        }
        const PAD: f32 = 10.0;
        let has_goal = kpis.iter().any(|k| k.goal.is_some());
        let h = if has_goal { 62.0 } else { 48.0 };
        // One row, or two like the app's grid when a goal would not fit a sixth of the width.
        let goal_fits = |columns: usize| {
            let room = CONTENT_W / columns as f32 - 2.0 * PAD - 11.0;
            kpis.iter()
                .filter_map(|k| k.goal.as_ref())
                .all(|(goal, _)| self.width(&clean(goal), Weight::Regular, 7.5) <= room)
        };
        let columns = if goal_fits(kpis.len()) {
            kpis.len()
        } else {
            kpis.len().div_ceil(2)
        };
        let rows = kpis.len().div_ceil(columns);
        let card_h = h * rows as f32;
        self.ensure(card_h);
        let top = self.y;
        let cell_w = CONTENT_W / columns as f32;
        self.card((LEFT, top, CONTENT_W, card_h));
        for row in 1..rows {
            let y = top + h * row as f32;
            self.line((LEFT, y), (LEFT + CONTENT_W, y), Line::solid(BORDER, 0.75));
        }
        for (i, k) in kpis.iter().enumerate() {
            let x = LEFT + (i % columns) as f32 * cell_w;
            let y = top + (i / columns) as f32 * h;
            if i % columns > 0 {
                self.line((x, y + 8.0), (x, y + h - 8.0), Line::solid(BORDER, 0.75));
            }
            let pad = PAD;
            let inner = cell_w - 2.0 * pad;
            let mut label_w = inner;
            if let Some(delta) = &k.delta {
                let dw = self
                    .width(&clean(delta), Weight::Medium, 7.5)
                    .min(inner * 0.6);
                self.text_right(
                    x + cell_w - pad,
                    y + 17.0,
                    delta,
                    Style::new(Weight::Medium, 7.5, tone_color(k.delta_tone)),
                    dw,
                );
                label_w = inner - dw - 4.0;
            }
            self.text(
                x + pad,
                y + 17.0,
                &k.label,
                Style::new(Weight::Regular, 7.5, MUTED),
                label_w,
            );
            let unit_w = if k.unit.is_empty() {
                0.0
            } else {
                self.width(&clean(&k.unit), Weight::Regular, 8.0) + 3.0
            };
            let value_max = (inner - unit_w).max(10.0);
            let value = self.fonts.semibold.fit(&clean(&k.value), 15.0, value_max);
            let value_w = self.width(&value, Weight::Semibold, 15.0);
            self.text(
                x + pad,
                y + 36.0,
                &value,
                Style::new(Weight::Semibold, 15.0, TEXT),
                value_max,
            );
            if unit_w > 0.0 {
                self.text(
                    x + pad + value_w + 3.0,
                    y + 36.0,
                    &k.unit,
                    Style::new(Weight::Regular, 8.0, SUBTLE),
                    inner - value_w - 3.0,
                );
            }
            if let Some((goal, met)) = &k.goal {
                self.mark(x + pad, y + 46.0, 7.0, *met);
                self.text(
                    x + pad + 11.0,
                    y + 52.5,
                    goal,
                    Style::new(Weight::Regular, 7.5, pass_color(*met)),
                    inner - 11.0,
                );
            }
        }
        self.y += card_h + GAP;
    }

    // The "Performance over time" card.
    fn chart(&mut self, chart: &ReportChart) {
        const PAD: f32 = 12.0;
        const PLOT_H: f32 = 118.0;
        const TICK: f32 = 7.0;
        // The legend: a short line, then the label.
        let legend: Vec<f32> = chart
            .series
            .iter()
            .map(|s| 16.0 + 4.0 + self.width(&clean(&s.label), Weight::Regular, 8.0))
            .collect();
        let (places, rows) = flow(&legend, CONTENT_W - 2.0 * PAD, 12.0);
        let legend_h = rows as f32 * 13.0;
        let h = 11.0 + 14.0 + legend_h + 12.0 + PLOT_H + 18.0 + 4.0;
        self.ensure(h);
        let (x0, y0) = (LEFT, self.y);
        self.card((x0, y0, CONTENT_W, h));
        self.text(
            x0 + PAD,
            y0 + 11.0 + 9.0,
            &chart.title,
            Style::new(Weight::Medium, 10.0, TEXT),
            CONTENT_W - 2.0 * PAD,
        );

        // Legend row(s).
        let legend_top = y0 + 11.0 + 14.0;
        for ((s, &(row, dx)), &w) in chart.series.iter().zip(&places).zip(&legend) {
            let (x, y) = (x0 + PAD + dx, legend_top + row as f32 * 13.0 + 6.0);
            match s.style {
                ReportSeriesStyle::Area => {
                    self.rect(
                        (x, y - 3.0, 16.0, 6.0),
                        1.5,
                        Some((s.color, 0.18)),
                        Some(Line::solid(s.color, 0.9)),
                    );
                }
                ReportSeriesStyle::Line(width) => {
                    self.line(
                        (x, y),
                        (x + 16.0, y),
                        Line::solid(s.color, width.clamp(0.75, 2.25)),
                    );
                }
                ReportSeriesStyle::Dashed(width) => {
                    let stroke = Line {
                        dash: Some((3.0, 2.5)),
                        ..Line::solid(s.color, width.clamp(0.75, 2.25))
                    };
                    self.line((x, y), (x + 16.0, y), stroke);
                }
            }
            let _ = w;
            self.text(
                x + 20.0,
                y + 2.8,
                &s.label,
                Style::new(Weight::Regular, 8.0, MUTED),
                CONTENT_W - 2.0 * PAD - dx - 20.0,
            );
        }

        // Plot geometry.
        let left_w = chart
            .left_ticks
            .iter()
            .map(|t| self.width(&clean(t), Weight::Mono, TICK))
            .fold(0.0, f32::max);
        let right_w = chart
            .right_ticks
            .iter()
            .map(|t| self.width(&clean(t), Weight::Mono, TICK))
            .fold(0.0, f32::max);
        let left_gutter = if left_w > 0.0 { left_w + 6.0 } else { 0.0 };
        let right_gutter = if right_w > 0.0 { right_w + 6.0 } else { 0.0 };
        let plot_l = x0 + PAD + left_gutter;
        let plot_r = x0 + CONTENT_W - PAD - right_gutter;
        let plot_w = (plot_r - plot_l).max(10.0);
        let plot_t = legend_top + legend_h + 12.0;
        let plot_b = plot_t + PLOT_H;

        // Grid lines and the tick labels on both sides.
        let tick_y = |i: usize, n: usize| -> f32 {
            if n < 2 {
                plot_b
            } else {
                plot_b - PLOT_H * i as f32 / (n - 1) as f32
            }
        };
        let grid_n = chart.left_ticks.len().max(chart.right_ticks.len());
        for i in 0..grid_n {
            let y = tick_y(i, grid_n);
            let color = if i == 0 { BORDER } else { GRID };
            self.line((plot_l, y), (plot_r, y), Line::solid(color, 0.6));
        }
        let left_color = chart
            .series
            .iter()
            .find(|s| s.axis == ReportAxis::Left && s.style != ReportSeriesStyle::Area)
            .or_else(|| chart.series.iter().find(|s| s.axis == ReportAxis::Left))
            .map_or(MUTED, |s| s.color);
        let n_left = chart.left_ticks.len();
        for (i, label) in chart.left_ticks.iter().enumerate() {
            let y = tick_y(i, n_left) + TICK * 0.36;
            self.text_right(
                plot_l - 6.0,
                y,
                label,
                Style::new(Weight::Mono, TICK, left_color),
                f32::INFINITY,
            );
        }
        let n_right = chart.right_ticks.len();
        for (i, label) in chart.right_ticks.iter().enumerate() {
            let y = tick_y(i, n_right) + TICK * 0.36;
            self.text(
                plot_r + 6.0,
                y,
                label,
                Style::new(Weight::Mono, TICK, MUTED),
                f32::INFINITY,
            );
        }
        let n_x = chart.x_ticks.len();
        for (i, label) in chart.x_ticks.iter().enumerate() {
            let frac = if n_x < 2 {
                0.0
            } else {
                i as f32 / (n_x - 1) as f32
            };
            let w = self.width(&clean(label), Weight::Mono, TICK);
            let cx =
                (plot_l + plot_w * frac).clamp(x0 + PAD + w / 2.0, x0 + CONTENT_W - PAD - w / 2.0);
            self.text_center(
                cx,
                plot_b + 12.0,
                label,
                Style::new(Weight::Mono, TICK, SUBTLE),
            );
        }

        // The selected window, behind the data.
        let points = chart
            .series
            .iter()
            .map(|s| s.values.len())
            .max()
            .unwrap_or(0);
        let x_at = |i: usize| -> f32 {
            if points < 2 {
                plot_l + plot_w / 2.0
            } else {
                plot_l + plot_w * i as f32 / (points - 1) as f32
            }
        };
        if let Some((a, b)) = chart.window {
            let (a, b) = (a.min(b), a.max(b));
            if points > 0 {
                let (xa, xb) = (x_at(a.min(points - 1)), x_at(b.min(points - 1)));
                let edge = Line {
                    alpha: 0.6,
                    ..Line::solid(ACCENT, 0.75)
                };
                self.rect(
                    (xa, plot_t, (xb - xa).max(0.5), PLOT_H),
                    0.0,
                    Some((ACCENT, 0.12)),
                    None,
                );
                self.line((xa, plot_t), (xa, plot_b), edge);
                self.line((xb, plot_t), (xb, plot_b), edge);
            }
        }

        // The series, the first one at the back.
        for s in &chart.series {
            let max = match s.axis {
                ReportAxis::Left => chart.left_max,
                ReportAxis::Right => chart.right_max,
            };
            let max = if max.is_finite() && max > 0.0 {
                max
            } else {
                1.0
            };
            let y_at = |v: f64| -> f32 { plot_b - PLOT_H * (v / max).clamp(0.0, 1.0) as f32 };
            for run in runs(&s.values, &x_at, &y_at) {
                match s.style {
                    ReportSeriesStyle::Area => {
                        if run.len() >= 2 {
                            let mut shape = run.clone();
                            if let (Some(first), Some(last)) = (run.first(), run.last()) {
                                shape.push((last.0, plot_b));
                                shape.push((first.0, plot_b));
                            }
                            self.poly(shape, true, Some((s.color, 0.14)), None);
                            self.poly(
                                run,
                                false,
                                None,
                                Some(Line {
                                    alpha: 0.7,
                                    ..Line::solid(s.color, 0.8)
                                }),
                            );
                        }
                    }
                    ReportSeriesStyle::Line(width) => {
                        self.stroke_run(run, Line::solid(s.color, width))
                    }
                    ReportSeriesStyle::Dashed(width) => {
                        let stroke = Line {
                            dash: Some((3.5, 2.5)),
                            alpha: 0.8,
                            ..Line::solid(s.color, width)
                        };
                        self.stroke_run(run, stroke);
                    }
                }
            }
        }

        // Reference lines, in front of the data.
        for r in &chart.reference_lines {
            let max = match r.axis {
                ReportAxis::Left => chart.left_max,
                ReportAxis::Right => chart.right_max,
            };
            let max = if max.is_finite() && max > 0.0 {
                max
            } else {
                1.0
            };
            let value = if r.value.is_finite() { r.value } else { 0.0 };
            let y = plot_b - PLOT_H * (value / max).clamp(0.0, 1.0) as f32;
            let stroke = Line {
                dash: Some((4.0, 3.0)),
                alpha: 0.75,
                ..Line::solid(r.color, 0.8)
            };
            self.line((plot_l, y), (plot_r, y), stroke);
            let label_y = if y - 9.0 < plot_t - 2.0 {
                y + 9.0
            } else {
                y - 3.0
            };
            // A white backing, so the label stays readable over the data.
            let label_w = self
                .width(&clean(&r.label), Weight::Mono, TICK)
                .min(plot_w - 8.0);
            self.rect(
                (plot_l + 2.0, label_y - TICK, label_w + 4.0, TICK + 2.5),
                1.5,
                Some((Rgb(255, 255, 255), 0.85)),
                None,
            );
            self.text(
                plot_l + 4.0,
                label_y,
                &r.label,
                Style::new(Weight::Mono, TICK, r.color),
                plot_w - 8.0,
            );
        }
        self.y += h + GAP;
    }

    /// Strokes one run of points: a polyline, or a dot when the run is a single point.
    fn stroke_run(&mut self, run: Vec<(f32, f32)>, stroke: Line) {
        match run.as_slice() {
            [] => {}
            [(x, y)] => {
                let r = stroke.width / 2.0;
                self.rect(
                    (x - r, y - r, stroke.width, stroke.width),
                    r,
                    Some((stroke.color, stroke.alpha)),
                    None,
                );
            }
            _ => self.poly(run, false, None, Some(stroke)),
        }
    }

    // The latency histogram on the left and the status bars on the right.
    fn distribution(&mut self, report: &Report) {
        let hist = report.histogram.as_ref();
        if hist.is_none() && report.statuses.is_empty() {
            return;
        }
        let gap = GAP;
        let (hist_w, status_w) = if hist.is_some() && !report.statuses.is_empty() {
            let hw = (CONTENT_W - gap) * 1.3 / 2.3;
            (hw, CONTENT_W - gap - hw)
        } else if hist.is_some() {
            (CONTENT_W, 0.0)
        } else {
            (0.0, CONTENT_W)
        };
        let status_h = 11.0 + 14.0 + 8.0 + report.statuses.len() as f32 * 18.0 + 6.0;
        let h = if hist.is_some() { 150.0 } else { 0.0_f32 }.max(if report.statuses.is_empty() {
            0.0
        } else {
            status_h
        });
        self.ensure(h);
        let y = self.y;
        if let Some(hist) = hist {
            self.histogram(hist, (LEFT, y, hist_w, h));
        }
        if !report.statuses.is_empty() {
            let x = LEFT + CONTENT_W - status_w;
            self.card((x, y, status_w, h));
            self.text(
                x + 12.0,
                y + 20.0,
                &report.statuses_title,
                Style::new(Weight::Medium, 10.0, TEXT),
                status_w - 24.0,
            );
            for (i, s) in report.statuses.iter().enumerate() {
                let row = y + 11.0 + 14.0 + 8.0 + i as f32 * 18.0;
                let base = row + 9.0;
                let code_w = 44.0;
                let share_w = 38.0;
                self.text(
                    x + 12.0,
                    base + 3.0,
                    &s.label,
                    Style::new(Weight::Mono, 8.5, s.color),
                    code_w - 4.0,
                );
                let bar_x = x + 12.0 + code_w;
                let bar_w = (status_w - 24.0 - code_w - share_w - 8.0).max(10.0);
                self.rect(
                    (bar_x, base - 2.5, bar_w, 5.0),
                    2.5,
                    Some((GRID, 1.0)),
                    None,
                );
                let fill = bar_w * s.share.clamp(0.0, 1.0) as f32;
                if fill > 0.0 {
                    self.rect(
                        (bar_x, base - 2.5, fill.max(2.5), 5.0),
                        2.5,
                        Some((s.color, 1.0)),
                        None,
                    );
                }
                self.text_right(
                    x + status_w - 12.0,
                    base + 3.0,
                    &s.share_text,
                    Style::new(Weight::Regular, 8.0, MUTED),
                    share_w,
                );
            }
        }
        self.y += h + GAP;
    }

    // The latency histogram card.
    fn histogram(&mut self, hist: &ReportHistogram, (x, y, w, h): (f32, f32, f32, f32)) {
        const PAD: f32 = 12.0;
        self.card((x, y, w, h));
        let caption_w = self.width(&clean(&hist.caption), Weight::Regular, 8.0);
        let title_w = self.width(&clean(&hist.title), Weight::Medium, 10.0);
        let caption_max = (w - 2.0 * PAD - title_w - 10.0).max(0.0).min(caption_w);
        self.text(
            x + PAD,
            y + 20.0,
            &hist.title,
            Style::new(Weight::Medium, 10.0, TEXT),
            w - 2.0 * PAD - caption_max - 10.0,
        );
        self.text_right(
            x + w - PAD,
            y + 20.0,
            &hist.caption,
            Style::new(Weight::Regular, 8.0, SUBTLE),
            caption_max,
        );

        let max = hist.counts.iter().copied().max().unwrap_or(0);
        let axis_w = self.width(&max.to_string(), Weight::Mono, 7.0).max(10.0) + 6.0;
        let plot_l = x + PAD + axis_w;
        let plot_r = x + w - PAD;
        let plot_w = (plot_r - plot_l).max(10.0);
        let plot_t = y + 34.0;
        let plot_h = h - 34.0 - 28.0;
        let plot_b = plot_t + plot_h;
        for (i, label) in [max.to_string(), (max / 2).to_string(), "0".to_string()]
            .iter()
            .enumerate()
        {
            let ly = plot_t + plot_h * i as f32 / 2.0;
            self.text_right(
                plot_l - 6.0,
                ly + 7.0 * 0.36,
                label,
                Style::new(Weight::Mono, 7.0, SUBTLE),
                f32::INFINITY,
            );
            if i < 2 {
                self.line((plot_l, ly), (plot_r, ly), Line::solid(GRID, 0.6));
            }
        }
        self.line(
            (plot_l, plot_b),
            (plot_r, plot_b),
            Line::solid(BORDER, 0.75),
        );
        let n = hist.counts.len();
        if n > 0 && max > 0 {
            let gap = if n > 1 {
                (plot_w * 0.012).min(2.0)
            } else {
                0.0
            };
            let bar_w = (plot_w - gap * (n - 1) as f32) / n as f32;
            for (i, &count) in hist.counts.iter().enumerate() {
                if count == 0 {
                    continue;
                }
                let bar_h = (plot_h * (count as f64 / max as f64) as f32).max(1.0);
                let color = hist.colors.get(i).copied().unwrap_or(ACCENT);
                let bx = plot_l + i as f32 * (bar_w + gap);
                self.rect(
                    (bx, plot_b - bar_h, bar_w, bar_h),
                    0.0,
                    Some((color, 1.0)),
                    None,
                );
            }
        }
        let n_labels = hist.x_labels.len();
        for (i, label) in hist.x_labels.iter().enumerate() {
            let label_w = self.width(&clean(label), Weight::Mono, 7.0);
            let frac = if n_labels < 2 {
                0.0
            } else {
                i as f32 / (n_labels - 1) as f32
            };
            let cx = (plot_l + plot_w * frac).clamp(plot_l, plot_r - label_w / 2.0);
            let cx = cx.max(plot_l + label_w / 2.0).min(plot_r - label_w / 2.0);
            self.text_center(
                cx,
                plot_b + 12.0,
                label,
                Style::new(Weight::Mono, 7.0, SUBTLE),
            );
        }
    }

    // The per request table, row by row with a repeated header.
    fn per_request(&mut self, report: &Report) {
        const ROW: f32 = 18.0;
        const HEAD: f32 = 20.0;
        const SIZE: f32 = 8.5;
        const COL_GAP: f32 = 12.0;
        let table = &report.per_request;
        let n = table.headers.len();
        if n == 0 {
            return;
        }
        let right = |i: usize| {
            table.align.get(i).copied().unwrap_or(ReportAlign::Left) == ReportAlign::Right
        };

        // Column widths.
        let natural: Vec<f32> = (0..n)
            .map(|i| {
                let (weight, head) = if right(i) {
                    (Weight::Mono, Weight::Medium)
                } else {
                    (Weight::Regular, Weight::Medium)
                };
                let cells = table
                    .rows
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| self.width(&clean(c), weight, SIZE))
                    .fold(0.0, f32::max);
                cells.max(self.width(&clean(&table.headers[i]), head, 7.5))
            })
            .collect();
        let n_right = (0..n).filter(|&i| right(i)).count();
        let n_left = n - n_right;
        let gaps = COL_GAP * (n - 1) as f32;
        let mut widths = natural.clone();
        if n_left == 0 {
            widths = vec![(CONTENT_W - gaps) / n as f32; n];
        } else {
            let mut right_w = (0..n)
                .filter(|&i| right(i))
                .map(|i| natural[i])
                .fold(0.0, f32::max);
            let min_left = 90.0 * n_left as f32;
            if n_right as f32 * right_w + gaps + min_left > CONTENT_W {
                right_w = ((CONTENT_W - gaps - min_left) / n_right.max(1) as f32).max(30.0);
            }
            let left_total = (CONTENT_W - gaps - n_right as f32 * right_w).max(0.0);
            let left_natural: f32 = (0..n).filter(|&i| !right(i)).map(|i| natural[i]).sum();
            for i in 0..n {
                widths[i] = if right(i) {
                    right_w
                } else if left_natural > 0.0 {
                    left_total * natural[i] / left_natural
                } else {
                    left_total / n_left as f32
                };
            }
        }

        let first_rows = table.rows.len().min(2);
        self.ensure(16.0 + 6.0 + HEAD + first_rows as f32 * ROW);
        self.text(
            LEFT,
            self.y + 11.0,
            &table.title,
            Style::new(Weight::Semibold, 11.0, TEXT),
            CONTENT_W,
        );
        self.y += 16.0 + 6.0;
        self.table_header(
            &table.headers,
            &widths,
            &right_flags(table.align.as_slice(), n),
            COL_GAP,
            HEAD,
        );
        for row in &table.rows {
            if self.y + ROW > BOTTOM {
                self.new_page();
                self.table_header(
                    &table.headers,
                    &widths,
                    &right_flags(table.align.as_slice(), n),
                    COL_GAP,
                    HEAD,
                );
            }
            let base = self.y + ROW / 2.0 + SIZE * 0.36;
            let mut x = LEFT;
            for (i, &width) in widths.iter().enumerate() {
                let cell = row.get(i).map_or("", String::as_str);
                if right(i) {
                    self.text_right(
                        x + width,
                        base,
                        cell,
                        Style::new(Weight::Mono, SIZE, TEXT),
                        width,
                    );
                } else {
                    self.text(
                        x,
                        base,
                        cell,
                        Style::new(Weight::Regular, SIZE, TEXT),
                        width,
                    );
                }
                x += width + COL_GAP;
            }
            self.y += ROW;
            self.line(
                (LEFT, self.y),
                (LEFT + CONTENT_W, self.y),
                Line::solid(GRID, 0.6),
            );
        }
        self.y += GAP;
    }

    // A table header row: muted labels over a line.
    fn table_header(
        &mut self,
        headers: &[String],
        widths: &[f32],
        right: &[bool],
        gap: f32,
        height: f32,
    ) {
        let base = self.y + height / 2.0 + 7.5 * 0.36;
        let mut x = LEFT;
        for (i, header) in headers.iter().enumerate() {
            if right.get(i).copied().unwrap_or(false) {
                self.text_right(
                    x + widths[i],
                    base,
                    header,
                    Style::new(Weight::Medium, 7.5, SUBTLE),
                    widths[i],
                );
            } else {
                self.text(
                    x,
                    base,
                    header,
                    Style::new(Weight::Medium, 7.5, SUBTLE),
                    widths[i],
                );
            }
            x += widths[i] + gap;
        }
        self.y += height;
        self.line(
            (LEFT, self.y),
            (LEFT + CONTENT_W, self.y),
            Line::solid(BORDER, 0.75),
        );
    }

    // The error groups, or the "no errors" note.
    fn errors(&mut self, report: &Report) {
        const GROUP_H: f32 = 84.0;
        let heading_h = 16.0 + 6.0;
        let first_h = if report.errors.is_empty() {
            40.0
        } else {
            GROUP_H
        };
        self.ensure(heading_h + first_h);
        self.text(
            LEFT,
            self.y + 11.0,
            &report.labels.errors,
            Style::new(Weight::Semibold, 11.0, TEXT),
            CONTENT_W,
        );
        self.y += heading_h;
        if report.errors.is_empty() {
            let y = self.y;
            self.card((LEFT, y, CONTENT_W, 40.0));
            self.text_center(
                LEFT + CONTENT_W / 2.0,
                y + 24.0,
                &report.labels.no_errors,
                Style::new(Weight::Regular, 9.0, SUBTLE),
            );
            self.y += 40.0 + GAP;
            return;
        }
        for group in &report.errors {
            self.ensure(GROUP_H);
            self.error_group(group, GROUP_H);
            self.y += GROUP_H + 8.0;
        }
    }

    // One error group card.
    fn error_group(&mut self, g: &ReportErrorGroup, h: f32) {
        const PAD: f32 = 12.0;
        let (x, y) = (LEFT, self.y);
        self.card((x, y, CONTENT_W, h));
        let color = if g.code == "429" { WARN } else { BAD };
        let base = y + 20.0;
        let code_w = (self.width(&clean(&g.code), Weight::Mono, 9.5) + 12.0).clamp(44.0, 100.0);
        self.text(
            x + PAD,
            base,
            &g.code,
            Style::new(Weight::Mono, 9.5, color),
            code_w - 4.0,
        );
        // Count and share on the right, the title takes what is left.
        let share_w = self.width(&clean(&g.share), Weight::Regular, 8.5).max(30.0);
        let count_w = self.width(&clean(&g.count), Weight::Semibold, 9.5);
        let share_right = x + CONTENT_W - PAD;
        self.text_right(
            share_right,
            base,
            &g.share,
            Style::new(Weight::Regular, 8.5, MUTED),
            share_w,
        );
        let count_right = share_right - share_w - 10.0;
        self.text_right(
            count_right,
            base,
            &g.count,
            Style::new(Weight::Semibold, 9.5, TEXT),
            count_w,
        );
        let title_max = count_right - count_w - 12.0 - (x + PAD + code_w);
        self.text(
            x + PAD + code_w,
            base,
            &g.title,
            Style::new(Weight::Medium, 9.5, TEXT),
            title_max,
        );
        let inner = CONTENT_W - 2.0 * PAD;
        self.text(
            x + PAD,
            y + 35.0,
            &g.span,
            Style::new(Weight::Regular, 8.0, MUTED),
            inner,
        );
        self.text(
            x + PAD,
            y + 47.0,
            &g.requests,
            Style::new(Weight::Regular, 8.0, MUTED),
            inner,
        );
        // The sample in a mono box.
        let box_y = y + 54.0;
        self.rect(
            (x + PAD, box_y, inner, 20.0),
            4.0,
            Some((SURFACE, 1.0)),
            Some(Line::solid(BORDER, 0.6)),
        );
        self.text(
            x + PAD + 7.0,
            box_y + 10.0 + 7.5 * 0.36,
            &g.sample,
            Style::new(Weight::Mono, 7.5, MUTED),
            inner - 14.0,
        );
    }

    // The footer of every page: the generated by line and the page number.
    fn footers(&mut self, report: &Report) {
        let total = self.pages.len();
        for page in 0..total {
            let number = format!("{} / {}", page + 1, total);
            let number_w = self.width(&number, Weight::Mono, 8.0);
            let left = self.fonts.regular.fit(
                &clean(&report.labels.generated_by),
                8.0,
                CONTENT_W - number_w - 16.0,
            );
            let ops = [
                Op::Text {
                    x: LEFT,
                    y: FOOTER_Y,
                    text: left,
                    style: Style::new(Weight::Regular, 8.0, SUBTLE),
                },
                Op::Text {
                    x: LEFT + CONTENT_W - number_w,
                    y: FOOTER_Y,
                    text: number,
                    style: Style::new(Weight::Mono, 8.0, SUBTLE),
                },
            ];
            if let Some(page) = self.pages.get_mut(page) {
                page.extend(ops);
            }
        }
    }
}

/// Which columns are right aligned, for `n` columns.
fn right_flags(align: &[ReportAlign], n: usize) -> Vec<bool> {
    (0..n)
        .map(|i| align.get(i).copied() == Some(ReportAlign::Right))
        .collect()
}

/// The values of a series as runs of consecutive points; a missing or non finite value ends a run.
fn runs(
    values: &[f64],
    x_at: &dyn Fn(usize) -> f32,
    y_at: &dyn Fn(f64) -> f32,
) -> Vec<Vec<(f32, f32)>> {
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    for (i, &v) in values.iter().enumerate() {
        if v.is_finite() {
            current.push((x_at(i), y_at(v)));
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
