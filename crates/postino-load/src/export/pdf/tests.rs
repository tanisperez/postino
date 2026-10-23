use std::path::PathBuf;

use super::*;
use crate::export::report::{
    ReportKpi, ReportLabels, ReportReferenceLine, ReportSeries, ReportStatus, ReportTable,
    ReportThreshold, ReportVerdict,
};

const REGULAR: &[u8] = include_bytes!("../../../tests/fixtures/fonts/Geist-Regular.ttf");
const MEDIUM: &[u8] = include_bytes!("../../../tests/fixtures/fonts/Geist-Medium.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../../tests/fixtures/fonts/Geist-SemiBold.ttf");
const MONO: &[u8] = include_bytes!("../../../tests/fixtures/fonts/GeistMono-Regular.ttf");

fn fonts() -> PdfFonts<'static> {
    PdfFonts {
        regular: REGULAR,
        medium: MEDIUM,
        semibold: SEMIBOLD,
        mono: MONO,
    }
}

/// The number of pages the layout needs for `report`.
fn page_count(report: &Report) -> usize {
    let fonts = Fonts::new(&fonts()).expect("test fonts are valid");
    layout(report, &fonts).len()
}

const GREEN: Rgb = Rgb(0x1f, 0xa8, 0x6a);
const RED: Rgb = Rgb(0xe0, 0x4f, 0x4f);
const AMBER: Rgb = Rgb(0xd9, 0x8e, 0x1f);
const ORANGE: Rgb = Rgb(0xe0, 0x6a, 0x45);
const TEAL: Rgb = Rgb(0x2b, 0xa8, 0x98);
const INDIGO: Rgb = Rgb(0x5b, 0x63, 0xe6);
const GRAY: Rgb = Rgb(0x8a, 0x8e, 0x9a);

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

fn with_delta(mut k: ReportKpi, delta: &str, tone: ReportTone) -> ReportKpi {
    k.delta = Some(delta.into());
    k.delta_tone = tone;
    k
}

fn with_goal(mut k: ReportKpi, goal: &str, met: bool) -> ReportKpi {
    k.goal = Some((goal.into(), met));
    k
}

fn series(
    label: &str,
    color: Rgb,
    axis: ReportAxis,
    style: ReportSeriesStyle,
    values: Vec<f64>,
) -> ReportSeries {
    ReportSeries {
        label: label.into(),
        color,
        axis,
        style,
        values,
    }
}

fn status(label: &str, share: f64, text: &str, color: Rgb) -> ReportStatus {
    ReportStatus {
        label: label.into(),
        share,
        share_text: text.into(),
        color,
    }
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|c| (*c).to_string()).collect()
}

fn group(code: &str, title: &str, count: &str, share: &str, sample: &str) -> ReportErrorGroup {
    ReportErrorGroup {
        code: code.into(),
        title: title.into(),
        count: count.into(),
        share: share.into(),
        span: "First \u{b7} last: 00:36 \u{b7} 00:43".into(),
        requests: "Affected requests: POST create 301, GET list 111".into(),
        sample: sample.into(),
    }
}

/// A report like the design's: 60 seconds, 50 VUs, rate limiting around seconds 36 to 43.
fn full_report() -> Report {
    // A small deterministic generator, as in the design.
    let mut seed: u32 = 11;
    let mut rand = move || {
        seed = (seed * 9301 + 49297) % 233_280;
        f64::from(seed) / 233_280.0
    };
    let (mut vus, mut rps, mut p50, mut p90, mut p99, mut base) =
        (vec![], vec![], vec![], vec![], vec![], vec![]);
    for i in 0..60_u32 {
        let v = f64::from((5 * (i + 1)).min(50));
        let ramp = v / 50.0;
        let spike = if (36..=43).contains(&i) {
            ((f64::from(i) - 35.5) / 8.0 * std::f64::consts::PI).sin()
        } else {
            0.0
        };
        let r = ramp * (2150.0 + rand() * 120.0 - 60.0) * (1.0 - 0.3 * spike);
        let a = 6.0 + 4.0 * ramp + rand() * 1.5 + 5.0 * spike;
        let b = a * 2.1 + rand() * 2.0 + 18.0 * spike;
        let c = b * 1.5 + rand() * 4.0 + 40.0 * spike;
        vus.push(v / 50.0 * 2400.0 * 0.4);
        rps.push(r);
        p50.push(a);
        p90.push(b);
        p99.push(c.min(100.0));
        base.push(a * 2.1 * 1.45 + rand() * 3.0);
    }
    let chart = ReportChart {
        title: "Performance over time".into(),
        series: vec![
            series("VUs", GRAY, ReportAxis::Left, ReportSeriesStyle::Area, vus),
            series(
                "Run #2 p99",
                ORANGE,
                ReportAxis::Right,
                ReportSeriesStyle::Dashed(1.2),
                base,
            ),
            series(
                "p99",
                ORANGE,
                ReportAxis::Right,
                ReportSeriesStyle::Line(1.4),
                p99,
            ),
            series(
                "p90",
                AMBER,
                ReportAxis::Right,
                ReportSeriesStyle::Line(1.2),
                p90,
            ),
            series(
                "p50",
                TEAL,
                ReportAxis::Right,
                ReportSeriesStyle::Line(1.2),
                p50,
            ),
            series(
                "Req/s",
                INDIGO,
                ReportAxis::Left,
                ReportSeriesStyle::Line(1.8),
                rps,
            ),
        ],
        left_max: 2400.0,
        right_max: 100.0,
        left_ticks: ["0", "600", "1,200", "1,800", "2,400"]
            .map(String::from)
            .to_vec(),
        right_ticks: ["0 ms", "25 ms", "50 ms", "75 ms", "100 ms"]
            .map(String::from)
            .to_vec(),
        x_ticks: [
            "00:00", "00:10", "00:20", "00:30", "00:40", "00:50", "01:00",
        ]
        .map(String::from)
        .to_vec(),
        reference_lines: vec![ReportReferenceLine {
            axis: ReportAxis::Right,
            value: 50.0,
            label: "p99 goal 50 ms".into(),
            color: RED,
        }],
        window: Some((34, 45)),
    };
    let counts: Vec<u64> = (0..26_u32)
        .map(|i| {
            let x = (f64::from(i) - 3.0) / 3.0;
            (4200.0 * (-x * x).exp() + 40.0 * f64::from(26 - i) / 26.0) as u64
        })
        .collect();
    let colors = (0..26)
        .map(|i| {
            if i < 13 {
                INDIGO
            } else if i < 20 {
                AMBER
            } else {
                RED
            }
        })
        .collect();
    Report {
        title: "Run #3 \u{b7} users".into(),
        subtitle: "Finished \u{b7} Keep-alive \u{b7} 23/10/2026 12:04 \u{b7} 50 VUs \u{b7} 60 s"
            .into(),
        window: Some("Window 34\u{2013}46 s".into()),
        comparison: Some("Compared with Run #2".into()),
        verdict: Some(ReportVerdict {
            passed: false,
            text: "Not passed \u{b7} 1 of 3".into(),
        }),
        thresholds: vec![
            ReportThreshold {
                label: "p99 < 50 ms".into(),
                value: "62 ms".into(),
                passed: false,
            },
            ReportThreshold {
                label: "Errors < 1 %".into(),
                value: "0.81 %".into(),
                passed: true,
            },
            ReportThreshold {
                label: "Req/s \u{2265} 1,800".into(),
                value: "2,030".into(),
                passed: true,
            },
        ],
        kpis: vec![
            with_goal(
                with_delta(kpi("Req/s", "2,030", ""), "+1.0 %", ReportTone::Good),
                "goal \u{2265} 1,800",
                true,
            ),
            with_delta(kpi("p50", "11", "ms"), "+10.0 %", ReportTone::Bad),
            with_delta(kpi("p95", "28", "ms"), "+7.7 %", ReportTone::Bad),
            with_goal(
                with_delta(kpi("p99", "62", "ms"), "+63.2 %", ReportTone::Bad),
                "goal < 50 ms",
                false,
            ),
            with_delta(kpi("Errors", "0.81", "%"), "+0.61 pt", ReportTone::Bad),
            with_delta(kpi("Total", "50,837", ""), "+5.4 %", ReportTone::Good),
        ],
        chart: Some(chart),
        histogram: Some(ReportHistogram {
            title: "Latency distribution".into(),
            caption: "requests per 4 ms interval".into(),
            counts,
            colors,
            x_labels: ["0", "25", "50", "75", "100+ ms"]
                .map(String::from)
                .to_vec(),
        }),
        statuses_title: "Responses by status".into(),
        statuses: vec![
            status("200", 0.659, "65.9 %", GREEN),
            status("201", 0.325, "32.5 %", GREEN),
            status("429", 0.0081, "0.81 %", AMBER),
            status("502", 0.0045, "0.45 %", RED),
            status("Timeout", 0.0034, "0.34 %", RED),
        ],
        per_request: ReportTable {
            title: "Per request".into(),
            headers: ["Request", "Total", "p50", "p95", "p99", "Errors"]
                .map(String::from)
                .to_vec(),
            align: vec![
                ReportAlign::Left,
                ReportAlign::Right,
                ReportAlign::Right,
                ReportAlign::Right,
                ReportAlign::Right,
                ReportAlign::Right,
            ],
            rows: vec![
                row(&["POST create", "16,945", "12 ms", "31 ms", "58 ms", "1.20 %"]),
                row(&["GET list", "17,130", "10 ms", "26 ms", "49 ms", "0.31 %"]),
                row(&[
                    "DELETE delete",
                    "16,762",
                    "11 ms",
                    "27 ms",
                    "51 ms",
                    "0.92 %",
                ]),
            ],
        },
        errors: vec![
            group(
                "429",
                "429 Too Many Requests",
                "412",
                "0.81 %",
                "{\"error\":\"rate limit exceeded\",\"retry_after\":2}",
            ),
            group(
                "Timeout",
                "Request timed out",
                "173",
                "0.34 %",
                "operation timed out after 5000 ms",
            ),
            group(
                "502",
                "502 Bad Gateway",
                "229",
                "0.45 %",
                "<html><body><h1>502 Bad Gateway</h1></body></html>",
            ),
        ],
        labels: ReportLabels {
            summary: "Summary".into(),
            thresholds: "Thresholds".into(),
            errors: "Errors".into(),
            generated_by: "Generated by Postino 0.4.0 on 2026-10-23 12:04".into(),
            no_errors: "No errors".into(),
        },
    }
}

fn long_report() -> Report {
    let mut report = full_report();
    report.per_request.rows = (0..200)
        .map(|i| {
            row(&[
                &format!("GET /users/{i}/orders/with/a/rather/long/path/segment/{i}"),
                "1,234",
                "12 ms",
                "31 ms",
                "58 ms",
                "0.20 %",
            ])
        })
        .collect();
    report.errors = (0..10)
        .map(|i| {
            group(
                &format!("5{i:02}"),
                &format!("5{i:02} Some Server Error With A Fairly Long Descriptive Title"),
                "12",
                "0.02 %",
                "{\"error\":\"internal\"}",
            )
        })
        .collect();
    report
}

fn assert_pdf(bytes: &[u8]) {
    assert!(bytes.starts_with(b"%PDF"), "not a PDF");
    assert!(bytes.len() > 1000);
}

#[test]
fn full_report_is_a_pdf_on_few_pages() {
    let report = full_report();
    assert_pdf(&to_pdf(&report, &fonts()).expect("renders"));
    assert!(page_count(&report) <= 2);
}

#[test]
fn long_report_breaks_onto_more_pages() {
    let report = long_report();
    let bytes = to_pdf(&report, &fonts()).expect("renders");
    assert_pdf(&bytes);
    assert!(page_count(&report) > 1);
}

#[test]
fn gaps_and_empty_series_do_not_fail() {
    let mut report = full_report();
    if let Some(chart) = &mut report.chart {
        chart.series[0].values = vec![f64::NAN; 60];
        chart.series[2].values[10] = f64::NAN;
        chart.series[2].values[11] = f64::INFINITY;
        chart.series[3].values.clear();
        chart.series[4].values = vec![5.0];
        chart.left_max = 0.0;
        chart.window = Some((50, 100));
    }
    assert_pdf(&to_pdf(&report, &fonts()).expect("renders"));
}

#[test]
fn empty_report_does_not_fail() {
    let mut report = full_report();
    report.verdict = None;
    report.thresholds.clear();
    report.kpis.clear();
    report.chart = None;
    report.histogram = None;
    report.statuses.clear();
    report.per_request.headers.clear();
    report.per_request.rows.clear();
    report.errors.clear();
    assert_pdf(&to_pdf(&report, &fonts()).expect("renders"));
    assert_eq!(page_count(&report), 1);
}

#[test]
fn degenerate_inputs_do_not_fail() {
    let mut report = full_report();
    if let Some(chart) = &mut report.chart {
        chart.left_ticks.clear();
        chart.right_ticks.clear();
        chart.x_ticks.clear();
        chart.series.clear();
    }
    if let Some(hist) = &mut report.histogram {
        hist.counts = vec![0; 5];
        hist.colors.clear();
        hist.x_labels = vec!["only".into()];
    }
    report.per_request.rows.push(vec!["short row".into()]);
    report.title = "x".repeat(400);
    assert_pdf(&to_pdf(&report, &fonts()).expect("renders"));
}

#[test]
fn bad_font_is_an_error() {
    let report = full_report();
    let bad = PdfFonts {
        regular: b"not a font",
        ..fonts()
    };
    assert!(matches!(
        to_pdf(&report, &bad),
        Err(PdfError::Font("regular"))
    ));
}

#[test]
fn text_is_cut_with_an_ellipsis() {
    let fonts = Fonts::new(&fonts()).expect("test fonts are valid");
    let face = &fonts.regular;
    let cut = face.fit("A fairly long sentence that does not fit", 9.0, 60.0);
    assert!(cut.ends_with('\u{2026}'));
    assert!(face.width(&cut, 9.0) <= 60.0);
    assert_eq!(face.fit("short", 9.0, 60.0), "short");
    assert_eq!(face.fit("anything", 9.0, 1.0), "");
}

/// Writes the full report to `$CARGO_TARGET_DIR/report-sample.pdf` (and the long one next to it)
/// to look at the layout: `cargo test -p postino-load report_sample -- --ignored`.
#[test]
#[ignore = "writes a file to look at the layout"]
fn report_sample() {
    let dir =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from);
    for (name, report) in [
        ("report-sample.pdf", full_report()),
        ("report-sample-long.pdf", long_report()),
    ] {
        let bytes = to_pdf(&report, &fonts()).expect("renders");
        std::fs::write(dir.join(name), bytes).expect("writes the sample");
    }
}
