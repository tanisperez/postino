//! The text of `postino load`: the progress lines of `--no-tui` and the summary at the end.

use std::time::Duration;

use postino_load::{Delta, DeltaUnit, LoadSnapshot, StatusKey};

use super::fail_if::{FailIf, Values, format_ms};
use crate::style::Style;

/// `01:12`, or `1:02:03` past an hour.
pub fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// `30s`, `2m`, `1m 30s`, `1h`: a configured length.
pub fn span(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis == 0 {
        return "0s".to_string();
    }
    if !millis.is_multiple_of(1000) {
        return format!("{millis}ms");
    }
    let seconds = duration.as_secs();
    [
        (seconds / 3600, "h"),
        ((seconds / 60) % 60, "m"),
        (seconds % 60, "s"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, unit)| format!("{count}{unit}"))
    .collect::<Vec<_>>()
    .join(" ")
}

/// A status as the summary names it.
pub fn status_name(status: StatusKey) -> String {
    match status {
        StatusKey::Code(code) => code.to_string(),
        StatusKey::Timeout => "timeout".to_string(),
        StatusKey::Failed => "failed".to_string(),
    }
}

/// Microseconds as `38 ms`.
pub fn micros(us: u32) -> String {
    format_ms(f64::from(us) / 1000.0)
}

/// One `--no-tui` progress line.
pub fn progress_line(snapshot: &LoadSnapshot, planned: Duration, vus: u32, paused: bool) -> String {
    let mut line = format!(
        "[{} / {}] {}/{vus} VUs · {} requests · {:.1} rps · p95 {} · errors {:.1} %",
        clock(snapshot.elapsed),
        clock(planned),
        snapshot.active_vus,
        snapshot.total,
        snapshot.rps,
        micros(snapshot.p95),
        snapshot.error_rate * 100.0
    );
    if paused {
        line.push_str(" · paused");
    }
    line
}

/// What the summary needs about the run.
pub struct Summary<'a> {
    /// The first line: what ran and how.
    pub header: String,
    /// How it ended, such as `Finished after 01:00`.
    pub ending: String,
    /// Where the run was saved, when it was.
    pub saved: Option<String>,
    /// The final snapshot.
    pub snapshot: &'a LoadSnapshot,
    /// The numbers the conditions read.
    pub values: Values,
    /// Each target's method and name.
    pub targets: &'a [(String, String)],
    /// The compared run's number and its deltas.
    pub compare: Option<(u32, Vec<Delta>)>,
    /// The `--fail-if` conditions and whether each one held.
    pub conditions: Vec<(FailIf, bool)>,
}

/// The summary printed when the run ends.
pub fn summary(summary: &Summary<'_>, style: Style) -> String {
    let snapshot = summary.snapshot;
    let values = &summary.values;
    let mut out = format!("\n {}\n {}\n", style.bold(&summary.header), summary.ending);
    if let Some(saved) = &summary.saved {
        out.push_str(&format!(" {}\n", style.dim(saved)));
    }
    let errors = format!("{:.1} %", snapshot.error_rate * 100.0);
    let errors = if snapshot.error_rate > 0.0 {
        style.red(&errors)
    } else {
        style.green(&errors)
    };
    out.push_str(&format!(
        "\n Requests  {}   RPS {:.1}   Errors {errors}\n",
        snapshot.total, values.rps
    ));
    out.push_str(&format!(
        " Latency   p50 {}   p95 {}   p99 {}   avg {}   max {}\n",
        format_ms(values.p50_ms),
        format_ms(values.p95_ms),
        format_ms(values.p99_ms),
        format_ms(values.avg_ms),
        format_ms(values.max_ms)
    ));
    if !snapshot.status_counts.is_empty() {
        let statuses: Vec<String> = snapshot
            .status_counts
            .iter()
            .map(|(status, count)| {
                let name = match status {
                    StatusKey::Code(code) => style.status(*code),
                    other => style.red(&status_name(*other)),
                };
                format!("{name} × {count}")
            })
            .collect();
        out.push_str(&format!(" Status    {}\n", statuses.join("   ")));
    }

    if !snapshot.per_target.is_empty() {
        let names: Vec<String> = (0..snapshot.per_target.len())
            .map(|index| match summary.targets.get(index) {
                Some((method, name)) => format!("{method} {name}"),
                None => format!("Request {}", index + 1),
            })
            .collect();
        let width = names
            .iter()
            .map(|name| name.chars().count())
            .max()
            .unwrap_or(0);
        out.push_str(&format!(
            "\n {:<width$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>7}\n",
            "Request", "Count", "p50", "p95", "p99", "Errors"
        ));
        for (name, stats) in names.iter().zip(&snapshot.per_target) {
            out.push_str(&format!(
                " {name:<width$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>7}\n",
                stats.count,
                micros(stats.p50),
                micros(stats.p95),
                micros(stats.p99),
                format!("{:.1} %", stats.error_rate * 100.0)
            ));
        }
    }

    if let Some((number, deltas)) = &summary.compare {
        out.push_str(&format!("\n Compared with run {number}\n"));
        for delta in deltas {
            out.push_str(&format!(" {}\n", delta_line(delta, style)));
        }
    }

    if !summary.conditions.is_empty() {
        out.push_str("\n Fail if\n");
        for (condition, hit) in &summary.conditions {
            let mark = if *hit {
                style.red("✗")
            } else {
                style.green("✓")
            };
            out.push_str(&format!(
                " {mark} {condition}   {}\n",
                style.dim(&format!("was {}", condition.format_value(values)))
            ));
        }
    }
    out
}

/// `p95          120 ms → 112 ms   -6.7 %`.
fn delta_line(delta: &Delta, style: Style) -> String {
    let value = |value: f64| match delta.label {
        "Errors" => format!("{:.1} %", value * 100.0),
        "Requests/s" => format!("{value:.1}"),
        // Latencies come in microseconds.
        _ => format_ms(value / 1000.0),
    };
    let change = match delta.unit {
        DeltaUnit::Percent => format!("{:+.1} %", delta.change),
        DeltaUnit::PercentagePoints => format!("{:+.1} pp", delta.change),
    };
    let change = if delta.change == 0.0 {
        style.dim(&change)
    } else if delta.is_improvement {
        style.green(&change)
    } else {
        style.red(&change)
    };
    format!(
        "{:<11} {} → {}   {change}",
        delta.label,
        value(delta.previous),
        value(delta.current)
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn clocks_and_spans_read_well() {
        assert_eq!(clock(Duration::from_secs(72)), "01:12");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(span(Duration::from_secs(90)), "1m 30s");
        assert_eq!(span(Duration::from_secs(120)), "2m");
        assert_eq!(span(Duration::from_secs(7200)), "2h");
        assert_eq!(span(Duration::from_millis(200)), "200ms");
        assert_eq!(span(Duration::ZERO), "0s");
    }

    #[test]
    fn the_summary_lists_the_numbers_targets_and_conditions() {
        let snapshot = LoadSnapshot {
            elapsed: Duration::from_secs(60),
            active_vus: 0,
            total: 120,
            rps: 2.0,
            p50: 38_000,
            p95: 112_000,
            p99: 240_000,
            error_rate: 0.025,
            series: Vec::new(),
            histogram: [0; 26],
            error_examples: Vec::new(),
            status_counts: vec![(StatusKey::Code(200), 117), (StatusKey::Timeout, 3)],
            per_target: vec![postino_load::TargetStats {
                count: 120,
                p50: 38_000,
                p95: 112_000,
                p99: 240_000,
                error_rate: 0.025,
            }],
        };
        let values = Values {
            p95_ms: 112.0,
            error_rate: 0.025,
            rps: 2.0,
            ..Values::default()
        };
        let condition = FailIf::parse("errors>1%").expect("condition");
        let text = summary(
            &Summary {
                header: "postino load · list-users".into(),
                ending: "Finished after 01:00".into(),
                saved: Some("Saved as run 12".into()),
                snapshot: &snapshot,
                values,
                targets: &[("GET".into(), "list-users".into())],
                compare: None,
                conditions: vec![(condition, condition.hit(&values))],
            },
            Style::PLAIN,
        );
        assert!(
            text.contains(" Requests  120   RPS 2.0   Errors 2.5 %"),
            "{text}"
        );
        assert!(
            text.contains(" Status    200 × 117   timeout × 3"),
            "{text}"
        );
        assert!(text.contains(" GET list-users       120"), "{text}");
        assert!(text.contains(" ✗ errors > 1.0 %   was 2.5 %"), "{text}");
    }
}
