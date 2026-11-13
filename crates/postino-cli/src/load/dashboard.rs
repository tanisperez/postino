//! The live dashboard of `postino load` on a terminal, with `ratatui`: progress, requests,
//! throughput and errors with sparklines, the percentiles, a latency chart, the statuses and a
//! table per request. Redrawn four times a second; `q` stops, `p` pauses, `s` saves a report.
//!
//! The terminal is restored when the dashboard ends, and by the panic hook `ratatui::init`
//! installs if anything panics, so it is never left in raw mode.

use std::io;
use std::time::{Duration, Instant};

use postino_load::{LoadRun, LoadSnapshot, SampleLog, StatusKey};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Sparkline, Table,
};

use super::live::Live;
use super::summary::{clock, micros, status_name};

/// The time between two redraws.
const TICK: Duration = Duration::from_millis(250);
/// How long a message stays in the footer.
const MESSAGE_TIME: Duration = Duration::from_secs(5);
/// The most request rows the table shows.
const MAX_ROWS: usize = 8;

/// What the dashboard says about the run, besides its numbers.
#[derive(Debug, Clone)]
pub struct Header {
    /// What runs.
    pub label: String,
    /// The virtual users at full ramp-up.
    pub vus: u32,
    /// The configured length.
    pub duration: Duration,
    /// The configured ramp-up.
    pub ramp_up: Duration,
    /// Each target's method and name.
    pub targets: Vec<(String, String)>,
    /// Whether to use colors.
    pub color: bool,
}

/// Saves a report of the run so far, returning what to show in the footer.
pub type SaveReport<'a> = dyn FnMut(&LoadSnapshot, &SampleLog) -> Result<String, String> + 'a;

/// Everything one frame draws.
struct View<'a> {
    header: &'a Header,
    snapshot: &'a LoadSnapshot,
    live: &'a Live,
    /// The elapsed time without the pauses.
    elapsed: Duration,
    paused: bool,
    stopping: bool,
    message: Option<&'a str>,
}

/// Shows the dashboard until the run ends. Returns whether the user stopped it.
pub fn show(
    run: &LoadRun,
    live: &mut Live,
    header: &Header,
    save: &mut SaveReport<'_>,
) -> io::Result<bool> {
    let mut terminal = ratatui::try_init()?;
    let mut stopping = false;
    let mut message: Option<(String, Instant)> = None;
    let result = (|| -> io::Result<bool> {
        loop {
            live.update(run);
            let snapshot = run.snapshot();
            if message
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() > MESSAGE_TIME)
            {
                message = None;
            }
            let view = View {
                header,
                snapshot: &snapshot,
                live,
                elapsed: snapshot.elapsed.saturating_sub(run.paused_for()),
                paused: run.is_paused(),
                stopping,
                message: message.as_ref().map(|(text, _)| text.as_str()),
            };
            terminal.draw(|frame| draw(frame, &view))?;
            if run.is_finished() {
                return Ok(stopping);
            }
            let deadline = Instant::now() + TICK;
            loop {
                let now = Instant::now();
                if now >= deadline || !event::poll(deadline - now)? {
                    break;
                }
                let Event::Key(key) = event::read()? else {
                    // A resize redraws at once.
                    break;
                };
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let ctrl_c =
                    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
                if ctrl_c && stopping {
                    // A second Ctrl+C does not wait for the requests in flight.
                    ratatui::restore();
                    std::process::exit(130);
                }
                match key.code {
                    _ if ctrl_c => {
                        run.resume();
                        run.stop();
                        stopping = true;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => {
                        run.resume();
                        run.stop();
                        stopping = true;
                    }
                    KeyCode::Char('p') if !stopping => {
                        if run.is_paused() {
                            run.resume();
                        } else {
                            run.pause();
                        }
                    }
                    KeyCode::Char('s') => {
                        live.update(run);
                        let text = match save(&run.snapshot(), live.log()) {
                            Ok(text) => text,
                            Err(error) => format!("Could not save the report: {error}"),
                        };
                        message = Some((text, Instant::now()));
                    }
                    _ => continue,
                }
                break;
            }
        }
    })();
    ratatui::restore();
    result
}

/// A foreground color, or none without colors.
fn fg(view: &View<'_>, color: Color) -> Style {
    if view.header.color {
        Style::new().fg(color)
    } else {
        Style::new()
    }
}

fn status_color(status: StatusKey) -> Color {
    match status {
        StatusKey::Code(200..=299) => Color::Green,
        StatusKey::Code(300..=399) => Color::Cyan,
        StatusKey::Code(400..=499) => Color::Yellow,
        StatusKey::Code(_) => Color::Red,
        StatusKey::Timeout | StatusKey::Failed => Color::LightRed,
    }
}

/// The last `width` values of `values`, for a sparkline that fills its area.
fn tail(values: &[u64], width: u16) -> &[u64] {
    &values[values.len().saturating_sub(usize::from(width))..]
}

fn draw(frame: &mut Frame, view: &View<'_>) {
    let header = view.header;
    let title = format!(
        " postino load · {} · {} VUs · {} / {} ",
        header.label,
        header.vus,
        clock(view.elapsed),
        clock(header.duration)
    );
    let keys = match view.message {
        Some(message) => format!(" {message} "),
        None if view.stopping => {
            " stopping, waiting for the requests in flight · Ctrl+C quits now ".to_string()
        }
        None => " q stop · p pause · s save report ".to_string(),
    };
    let outer = Block::bordered()
        .title(Span::styled(
            title,
            Style::new().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(keys);
    let area = outer.inner(frame.area());
    frame.render_widget(outer, frame.area());

    let rows = view.snapshot.per_target.len().min(MAX_ROWS) as u16;
    let [gauge, _, kpis, latency, _, chart, status, table] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(rows + 2),
    ])
    .areas(area);

    draw_gauge(frame, view, gauge);
    draw_kpis(frame, view, kpis);
    let snapshot = view.snapshot;
    frame.render_widget(
        Paragraph::new(format!(
            " Latency   p50 {}   p95 {}   p99 {}   avg {}   max {}",
            micros(snapshot.p50),
            micros(snapshot.p95),
            micros(snapshot.p99),
            super::fail_if::format_ms(view.live.avg_ms()),
            super::fail_if::format_ms(view.live.max_ms()),
        )),
        latency,
    );
    draw_chart(frame, view, chart);
    draw_statuses(frame, view, status);
    draw_table(frame, view, table);
}

fn draw_gauge(frame: &mut Frame, view: &View<'_>, area: Rect) {
    let planned = view.header.duration.as_secs_f64().max(0.001);
    let ratio = (view.elapsed.as_secs_f64() / planned).clamp(0.0, 1.0);
    let phase = if view.stopping {
        "stopping".to_string()
    } else if view.paused {
        "paused".to_string()
    } else if view.elapsed < view.header.ramp_up && view.snapshot.active_vus < view.header.vus {
        format!(
            "ramping up, {}/{} VUs",
            view.snapshot.active_vus, view.header.vus
        )
    } else {
        format!("{} VUs", view.snapshot.active_vus)
    };
    let color = if view.paused {
        Color::Yellow
    } else {
        Color::Cyan
    };
    frame.render_widget(
        Gauge::default()
            .ratio(ratio)
            .label(format!("{:.0} %  {phase}", ratio * 100.0))
            .gauge_style(fg(view, color))
            .use_unicode(true),
        area,
    );
}

fn draw_kpis(frame: &mut Frame, view: &View<'_>, area: Rect) {
    let snapshot = view.snapshot;
    let columns: [Rect; 3] = Layout::horizontal([Constraint::Ratio(1, 3); 3]).areas(area);
    let vus: Vec<u64> = snapshot
        .series
        .iter()
        .map(|point| u64::from(point.vus))
        .collect();
    let rps: Vec<u64> = snapshot
        .series
        .iter()
        .map(|point| point.rps.round() as u64)
        .collect();
    let errors = view.live.error_percent_series();
    let error_color = if snapshot.error_rate > 0.0 {
        Color::Red
    } else {
        Color::Green
    };
    let kpis = [
        (format!(" Requests {}", snapshot.total), vus, Color::Blue),
        (format!(" RPS {:.1}", snapshot.rps), rps, Color::Cyan),
        (
            format!(" Errors {:.1} %", snapshot.error_rate * 100.0),
            errors,
            error_color,
        ),
    ];
    for ((text, series, color), column) in kpis.into_iter().zip(columns) {
        let [label, spark] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).areas(column);
        frame.render_widget(
            Paragraph::new(text).style(Style::new().add_modifier(Modifier::BOLD)),
            label,
        );
        let width = spark.width.saturating_sub(2);
        let spark = Rect {
            x: spark.x + 1,
            width,
            ..spark
        };
        frame.render_widget(
            Sparkline::default()
                .data(tail(&series, width).iter().copied())
                .style(fg(view, color)),
            spark,
        );
    }
}

fn draw_chart(frame: &mut Frame, view: &View<'_>, area: Rect) {
    let series = &view.snapshot.series;
    let p95: Vec<(f64, f64)> = series
        .iter()
        .enumerate()
        .map(|(second, point)| (second as f64, f64::from(point.p95) / 1000.0))
        .collect();
    let p50: Vec<(f64, f64)> = series
        .iter()
        .enumerate()
        .map(|(second, point)| (second as f64, f64::from(point.p50) / 1000.0))
        .collect();
    let top = p95.iter().map(|(_, ms)| *ms).fold(1.0_f64, f64::max) * 1.1;
    let right = view
        .header
        .duration
        .as_secs_f64()
        .max(series.len() as f64)
        .max(1.0);
    let datasets = vec![
        Dataset::default()
            .name("p50")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(fg(view, Color::Green))
            .data(&p50),
        Dataset::default()
            .name("p95")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(fg(view, Color::Yellow))
            .data(&p95),
    ];
    let label = |ms: f64| super::fail_if::format_ms(ms);
    frame.render_widget(
        Chart::new(datasets)
            .x_axis(
                Axis::default()
                    .bounds([0.0, right])
                    .labels(["00:00".to_string(), clock(Duration::from_secs_f64(right))]),
            )
            .y_axis(Axis::default().bounds([0.0, top]).labels([
                "0".to_string(),
                label(top / 2.0),
                label(top),
            ]))
            .legend_position(Some(ratatui::widgets::LegendPosition::TopRight)),
        area,
    );
}

fn draw_statuses(frame: &mut Frame, view: &View<'_>, area: Rect) {
    let counts = &view.snapshot.status_counts;
    let total: u64 = counts.iter().map(|(_, count)| count).sum::<u64>().max(1);
    let mut spans = vec![Span::raw(" Status  ")];
    for (status, count) in counts {
        let cells = ((count * 16).div_ceil(total)).max(1) as usize;
        spans.push(Span::styled(
            status_name(*status),
            fg(view, status_color(*status)),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "█".repeat(cells),
            fg(view, status_color(*status)),
        ));
        spans.push(Span::raw(format!(" {count}   ")));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_table(frame: &mut Frame, view: &View<'_>, area: Rect) {
    let rows: Vec<Row> = view
        .snapshot
        .per_target
        .iter()
        .take(MAX_ROWS)
        .enumerate()
        .map(|(index, stats)| {
            let name = match view.header.targets.get(index) {
                Some((method, name)) => format!("{method} {name}"),
                None => format!("Request {}", index + 1),
            };
            Row::new([
                name,
                stats.count.to_string(),
                micros(stats.p50),
                micros(stats.p95),
                micros(stats.p99),
                format!("{:.1} %", stats.error_rate * 100.0),
            ])
        })
        .collect();
    let header = Row::new(["Request", "Count", "p50", "p95", "p99", "Errors"])
        .style(Style::new().add_modifier(Modifier::BOLD));
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(8),
            ],
        )
        .header(header)
        .block(Block::new().borders(ratatui::widgets::Borders::TOP)),
        area,
    );
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_load::{SecondPoint, TargetStats};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn snapshot() -> LoadSnapshot {
        LoadSnapshot {
            elapsed: Duration::from_secs(12),
            active_vus: 5,
            total: 1234,
            rps: 98.5,
            p50: 38_000,
            p95: 112_000,
            p99: 240_000,
            error_rate: 0.004,
            series: (0..12)
                .map(|second| SecondPoint {
                    rps: 100.0,
                    p95: 100_000 + second * 1000,
                    p50: 40_000,
                    p90: 90_000,
                    p99: 200_000,
                    vus: 5,
                })
                .collect(),
            histogram: [0; 26],
            status_counts: vec![(StatusKey::Code(200), 1229), (StatusKey::Code(500), 5)],
            per_target: vec![TargetStats {
                count: 1234,
                p50: 38_000,
                p95: 112_000,
                p99: 240_000,
                error_rate: 0.004,
            }],
            error_examples: Vec::new(),
        }
    }

    #[test]
    fn a_frame_shows_the_numbers() {
        let header = Header {
            label: "list-users".into(),
            vus: 5,
            duration: Duration::from_secs(60),
            ramp_up: Duration::ZERO,
            targets: vec![("GET".into(), "list-users".into())],
            color: false,
        };
        let snapshot = snapshot();
        let live = Live::default();
        let view = View {
            header: &header,
            snapshot: &snapshot,
            live: &live,
            elapsed: Duration::from_secs(12),
            paused: false,
            stopping: false,
            message: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(90, 30)).expect("terminal");
        terminal.draw(|frame| draw(frame, &view)).expect("draw");
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for needle in [
            "postino load · list-users · 5 VUs · 00:12 / 01:00",
            "20 %",
            "Requests 1234",
            "RPS 98.5",
            "Errors 0.4 %",
            "p95 112 ms",
            "GET list-users",
            "q stop · p pause · s save report",
        ] {
            assert!(text.contains(needle), "missing {needle:?}");
        }
    }
}
