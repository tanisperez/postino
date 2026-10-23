//! [`to_markdown`], the Markdown rendering of a [`Report`].
//!
//! The output is GitHub flavored Markdown: a title, the verdict and thresholds, the KPIs, the
//! per request table, the status shares and the error groups. The chart and the histogram have no
//! Markdown form and are left out. The report carries no column headers for the KPIs and the
//! statuses (it only has their labels), so those two tables are laid out transposed: one column
//! per KPI or status and one row per line of information.

use super::report::{Report, ReportAlign, ReportTable};

/// The mark before a passed check.
const PASS: &str = "\u{2705}";
/// The mark before a failed check.
const FAIL: &str = "\u{274c}";

/// Renders `report` as GitHub flavored Markdown.
///
/// Only lays out the strings the report already holds, nothing is formatted or translated here.
#[must_use]
pub fn to_markdown(report: &Report) -> String {
    let mut blocks: Vec<String> = Vec::new();

    blocks.push(format!("# {}", one_line(&report.title)));
    blocks.push(one_line(&report.subtitle));
    if let Some(window) = &report.window {
        blocks.push(one_line(window));
    }
    if let Some(comparison) = &report.comparison {
        blocks.push(one_line(comparison));
    }
    if let Some(verdict) = &report.verdict {
        blocks.push(format!(
            "{} {}",
            mark(verdict.passed),
            one_line(&verdict.text)
        ));
    }

    if !report.thresholds.is_empty() {
        blocks.push(format!("## {}", one_line(&report.labels.thresholds)));
        let items: Vec<String> = report
            .thresholds
            .iter()
            .map(|t| {
                format!(
                    "- {} {}: {}",
                    mark(t.passed),
                    one_line(&t.label),
                    one_line(&t.value)
                )
            })
            .collect();
        blocks.push(items.join("\n"));
    }

    if !report.kpis.is_empty() {
        blocks.push(format!("## {}", one_line(&report.labels.summary)));
        blocks.push(kpi_table(report));
    }

    if !report.per_request.headers.is_empty() {
        blocks.push(format!("## {}", one_line(&report.per_request.title)));
        blocks.push(table(&report.per_request));
    }

    if !report.statuses.is_empty() {
        blocks.push(format!("## {}", one_line(&report.statuses_title)));
        let headers: Vec<String> = report.statuses.iter().map(|s| s.label.clone()).collect();
        let shares: Vec<String> = report
            .statuses
            .iter()
            .map(|s| s.share_text.clone())
            .collect();
        blocks.push(grid(&headers, &[ReportAlign::Right], &[shares]));
    }

    blocks.push(format!("## {}", one_line(&report.labels.errors)));
    if report.errors.is_empty() {
        blocks.push(one_line(&report.labels.no_errors));
    } else {
        for group in &report.errors {
            let heading = if group.title.contains(&group.code) {
                one_line(&group.title)
            } else {
                format!(
                    "{} \u{b7} {}",
                    one_line(&group.code),
                    one_line(&group.title)
                )
            };
            blocks.push(format!("### {heading}"));
            let mut lines = vec![
                format!(
                    "**{}** ({})",
                    one_line(&group.count),
                    one_line(&group.share)
                ),
                format!(
                    "- {}\n- {}",
                    one_line(&group.span),
                    one_line(&group.requests)
                ),
            ];
            let sample = one_line(&group.sample);
            if !sample.is_empty() {
                let fence = "`".repeat(longest_backtick_run(&sample).max(2) + 1);
                lines.push(format!("{fence}\n{sample}\n{fence}"));
            }
            blocks.push(lines.join("\n\n"));
        }
    }

    blocks.push(format!("*{}*", one_line(&report.labels.generated_by)));

    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

/// The check or cross mark for a result.
fn mark(passed: bool) -> &'static str {
    if passed { PASS } else { FAIL }
}

/// The KPI table: one column per KPI, then the value with its unit, the delta when any KPI has
/// one and the goal when any has one.
fn kpi_table(report: &Report) -> String {
    let headers: Vec<String> = report.kpis.iter().map(|k| k.label.clone()).collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(
        report
            .kpis
            .iter()
            .map(|k| {
                if k.unit.is_empty() {
                    k.value.clone()
                } else {
                    format!("{} {}", k.value, k.unit)
                }
            })
            .collect(),
    );
    if report.kpis.iter().any(|k| k.delta.is_some()) {
        rows.push(
            report
                .kpis
                .iter()
                .map(|k| k.delta.clone().unwrap_or_default())
                .collect(),
        );
    }
    if report.kpis.iter().any(|k| k.goal.is_some()) {
        rows.push(
            report
                .kpis
                .iter()
                .map(|k| match &k.goal {
                    Some((text, met)) => format!("{} {}", mark(*met), text),
                    None => String::new(),
                })
                .collect(),
        );
    }
    grid(&headers, &[ReportAlign::Right], &rows)
}

/// A report table as a Markdown table.
fn table(table: &ReportTable) -> String {
    grid(&table.headers, &table.align, &table.rows)
}

/// A Markdown table. `align` is applied per column; a shorter slice repeats its last entry.
fn grid(headers: &[String], align: &[ReportAlign], rows: &[Vec<String>]) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 2);
    lines.push(row_line(headers.iter().map(String::as_str)));
    let separators: Vec<&str> = (0..headers.len())
        .map(|i| {
            let a = align
                .get(i)
                .or_else(|| align.last())
                .copied()
                .unwrap_or(ReportAlign::Left);
            match a {
                ReportAlign::Left => "---",
                ReportAlign::Right => "---:",
            }
        })
        .collect();
    lines.push(format!("| {} |", separators.join(" | ")));
    for row in rows {
        lines.push(row_line(row.iter().map(String::as_str)));
    }
    lines.join("\n")
}

/// One table row: cells escaped and joined with pipes.
fn row_line<'a>(cells: impl Iterator<Item = &'a str>) -> String {
    let cells: Vec<String> = cells.map(cell).collect();
    format!("| {} |", cells.join(" | "))
}

/// A table cell: `|` escaped and line breaks turned into spaces.
fn cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// `text` with every line break replaced by a space, so it stays on one Markdown line.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// The length of the longest run of backticks in `text`.
fn longest_backtick_run(text: &str) -> usize {
    let (mut longest, mut current) = (0, 0);
    for c in text.chars() {
        if c == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::export::report::{
        ReportErrorGroup, ReportKpi, ReportLabels, ReportStatus, ReportThreshold, ReportTone,
        ReportVerdict, Rgb,
    };

    fn labels() -> ReportLabels {
        ReportLabels {
            summary: "Summary".into(),
            thresholds: "Thresholds".into(),
            errors: "Errors".into(),
            generated_by: "Generated by Postino 0.4.0 on 2026-10-23 12:04".into(),
            no_errors: "No errors".into(),
        }
    }

    fn kpi(label: &str, value: &str, unit: &str) -> ReportKpi {
        ReportKpi {
            label: label.into(),
            value: value.into(),
            unit: unit.into(),
            delta: None,
            delta_tone: ReportTone::Neutral,
            goal: None,
        }
    }

    fn minimal() -> Report {
        Report {
            title: "Run #1 \u{b7} users".into(),
            subtitle: "Finished \u{b7} 10 VUs \u{b7} 5 s".into(),
            window: None,
            comparison: None,
            verdict: None,
            thresholds: vec![],
            kpis: vec![kpi("Req/s", "100", ""), kpi("p50", "12", "ms")],
            chart: None,
            histogram: None,
            statuses_title: "Responses by status".into(),
            statuses: vec![ReportStatus {
                label: "200".into(),
                share: 1.0,
                share_text: "100 %".into(),
                color: Rgb(0, 128, 0),
            }],
            per_request: ReportTable {
                title: "Requests".into(),
                headers: vec!["Request".into(), "Count".into()],
                align: vec![ReportAlign::Left, ReportAlign::Right],
                rows: vec![vec!["GET list".into(), "500".into()]],
            },
            errors: vec![],
            labels: labels(),
        }
    }

    fn full() -> Report {
        let mut report = minimal();
        report.window = Some("Window 34\u{2013}46 s".into());
        report.comparison = Some("Compared with Run #0".into());
        report.verdict = Some(ReportVerdict {
            passed: false,
            text: "Not passed \u{b7} 1 of 2".into(),
        });
        report.thresholds = vec![
            ReportThreshold {
                label: "p99 < 50 ms".into(),
                value: "42 ms".into(),
                passed: true,
            },
            ReportThreshold {
                label: "p50 < 10 ms".into(),
                value: "12 ms".into(),
                passed: false,
            },
        ];
        report.kpis[0].delta = Some("+4,2 %".into());
        report.kpis[0].goal = Some(("goal \u{2265} 90".into(), true));
        report.kpis[1].goal = Some(("goal < 10 ms".into(), false));
        report
            .per_request
            .rows
            .push(vec!["POST a|b".into(), "7".into()]);
        report.errors = vec![
            ReportErrorGroup {
                code: "429".into(),
                title: "429 Too Many Requests".into(),
                count: "412".into(),
                share: "0,8 %".into(),
                span: "First \u{b7} last: 00:34 \u{b7} 00:45".into(),
                requests: "Affected: POST create 412".into(),
                sample: "{\"error\":\"rate limit\"}".into(),
            },
            ReportErrorGroup {
                code: "Timeout".into(),
                title: "Request timed out".into(),
                count: "3".into(),
                share: "0,01 %".into(),
                span: "First \u{b7} last: 00:10 \u{b7} 00:11".into(),
                requests: "Affected: GET list 3".into(),
                sample: "uses `ticks` and ```triple```".into(),
            },
        ];
        report
    }

    #[test]
    fn minimal_report_exact_output() {
        let expected = "\
# Run #1 \u{b7} users

Finished \u{b7} 10 VUs \u{b7} 5 s

## Summary

| Req/s | p50 |
| ---: | ---: |
| 100 | 12 ms |

## Requests

| Request | Count |
| --- | ---: |
| GET list | 500 |

## Responses by status

| 200 |
| ---: |
| 100 % |

## Errors

No errors

*Generated by Postino 0.4.0 on 2026-10-23 12:04*
";
        assert_eq!(to_markdown(&minimal()), expected);
    }

    #[test]
    fn full_report_has_every_section() {
        let md = to_markdown(&full());
        assert!(md.contains("Window 34\u{2013}46 s\n\nCompared with Run #0\n\n"));
        assert!(md.contains("\u{274c} Not passed \u{b7} 1 of 2"));
        assert!(md.contains("- \u{2705} p99 < 50 ms: 42 ms\n- \u{274c} p50 < 10 ms: 12 ms"));
        assert!(md.contains("| Req/s | p50 |\n| ---: | ---: |"));
        assert!(md.contains("| 100 | 12 ms |\n| +4,2 % |  |"));
        assert!(md.contains("| \u{2705} goal \u{2265} 90 | \u{274c} goal < 10 ms |"));
        assert!(md.contains("| POST a\\|b | 7 |"));
        // The first title already holds its code, the second one does not.
        assert!(md.contains("### 429 Too Many Requests\n\n**412** (0,8 %)"));
        assert!(md.contains("### Timeout \u{b7} Request timed out"));
        assert!(
            md.contains("- First \u{b7} last: 00:34 \u{b7} 00:45\n- Affected: POST create 412")
        );
        assert!(md.contains("```\n{\"error\":\"rate limit\"}\n```"));
        assert!(md.contains("````\nuses `ticks` and ```triple```\n````"));
        assert!(!md.contains("No errors"));
        assert!(md.ends_with("*Generated by Postino 0.4.0 on 2026-10-23 12:04*\n"));
        assert!(!md.contains('\u{2014}'));
    }
}
