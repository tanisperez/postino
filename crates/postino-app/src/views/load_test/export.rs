//! The load test results' Export menu actions: saving the run as Markdown, PDF, CSV or JSON, and
//! copying the summary as Markdown (`Ctrl+Shift+C`).
//!
//! Everything that costs time runs on the background executor: the statistics of the covered
//! range, rendering the PDF, building the CSV and the JSON and writing the file. The UI thread
//! only gathers what the tab holds (cheap clones and `Arc`s), formats the report's texts and
//! shows the result.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::notification::Notification;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_load::export::csv::requests_csv;
use postino_load::export::json::run_json;
use postino_load::export::markdown::to_markdown;
use postino_load::export::pdf::{PdfFonts, to_pdf};
use postino_load::export::report::Report;
use postino_load::history::{self, LoadConfigSummary, RunRecord};
use postino_load::{LoadSnapshot, SampleLog, Threshold, WindowStats, window_stats};
use rust_i18n::t;
use telaio_ui::fonts::{GEIST_MEDIUM, GEIST_MONO_REGULAR, GEIST_REGULAR, GEIST_SEMIBOLD};

use crate::actions::CopyLoadTestSummary;
use crate::state::format::shorten_path;
use crate::state::load_report::{
    ReportColors, ReportInput, build_report, connection_label, stats_from_snapshot, status_label,
};
use crate::state::load_results::{ChartData, Shared, SummaryData};
use crate::state::load_test::{LoadTestStatus, LoadTestTab};
use crate::views::root::AppView;

/// A file format of the Export menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    /// The Markdown report.
    Markdown,
    /// The PDF report.
    Pdf,
    /// One CSV row per request.
    Csv,
    /// The whole run as JSON.
    Json,
}

impl ExportFormat {
    /// The file extension, without the dot.
    fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
        }
    }

    /// The name used in the log.
    fn name(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "markdown",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
        }
    }

    /// Whether the format is a document built from a [`Report`].
    fn is_report(self) -> bool {
        matches!(self, ExportFormat::Markdown | ExportFormat::Pdf)
    }
}

/// What an export needs from a load test tab, copied out of it so the work can move to the
/// background: the log and the chart are shared `Arc`s, the rest is small.
struct ExportSource {
    number: u32,
    target_label: String,
    status_label: String,
    connection_label: Option<String>,
    started_at: Option<u64>,
    vus: Option<u32>,
    duration_secs: u64,
    snapshot: LoadSnapshot,
    compare: Option<(u32, LoadSnapshot)>,
    thresholds: Vec<Threshold>,
    chart: Option<Shared<ChartData>>,
    /// The selected window, as inclusive second indices.
    window: Option<(usize, usize)>,
    /// Whether only the window is exported. `false` when there is no window or no log to cut.
    window_only: bool,
    /// Each target's method and name.
    targets: Vec<(String, String)>,
    timeout_secs: u64,
    log: Option<Shared<SampleLog>>,
    /// The tab's statistics when they already cover what is exported.
    analysis: Option<Shared<WindowStats>>,
    target_count: usize,
    run_secs: u32,
    elapsed_secs: f64,
    /// The workspace folder, where the run's record is saved.
    root: Option<PathBuf>,
    /// The record to export as JSON when the saved one cannot be read.
    record: RunRecord,
}

impl ExportSource {
    /// Gathers what `load_test` shows, `None` while a run is in progress or before there is
    /// anything to export.
    fn from_tab(load_test: &LoadTestTab, timeout_secs: u64, root: Option<&Path>) -> Option<Self> {
        if load_test.is_running() {
            return None;
        }
        let snapshot = load_test.snapshot.clone()?;
        let number = load_test.run_number?;
        let log = load_test.log.clone();
        // Without the requests there is nothing to cut a window from.
        let window_only =
            load_test.export_window_only && load_test.window.is_some() && log.is_some();
        let targets: Vec<(String, String)> = load_test
            .target_rows
            .iter()
            .map(|(method, name)| (method.to_string(), name.clone()))
            .collect();
        let compare = load_test
            .compare_with
            .zip(load_test.compare_snapshot.clone());
        let chart = load_test.chart.clone().or_else(|| {
            Some(Shared::new(ChartData::new(
                &snapshot,
                compare.as_ref().map(|(_, snapshot)| snapshot),
            )))
        });
        let target_count = log.as_ref().map_or(targets.len(), |log| {
            targets.len().max(
                log.samples()
                    .iter()
                    .map(|sample| sample.target as usize + 1)
                    .max()
                    .unwrap_or(0),
            )
        });
        let elapsed_secs = snapshot.elapsed.as_secs_f64();
        let run_secs = (load_test.shown_duration_secs() as f64)
            .max(elapsed_secs.ceil())
            .max(1.0) as u32;
        // The tab's statistics are of its selected window: they are what is exported when that
        // window is, or when there is none.
        let analysis = load_test
            .analysis
            .as_ref()
            .filter(|(key, _)| {
                key.generation == load_test.log_generation
                    && key.window == load_test.window
                    && (window_only || load_test.window.is_none())
            })
            .map(|(_, stats)| stats.clone());
        let config = load_test.config.resolved();
        let started_at = load_test.run_started_at;
        let record = RunRecord {
            number,
            started_at_unix: started_at.unwrap_or(0),
            target_label: load_test.target_label.clone(),
            config: LoadConfigSummary {
                vus: load_test.run_vus.unwrap_or(config.vus),
                duration_secs: load_test.shown_duration_secs(),
                ramp_up_secs: config.ramp_up_secs,
                think_time_ms: config.think_time_ms,
                stop_on_error_rate: config.stop_on_error_rate,
                target_count: target_count.max(snapshot.per_target.len()),
                reuse_connections: load_test.run_reuse_connections.unwrap_or(false),
            },
            snapshot: snapshot.clone(),
            stopped_early: load_test.status == LoadTestStatus::Stopped,
            target_labels: targets.clone(),
            thresholds: load_test.run_thresholds.clone(),
        };
        Some(Self {
            number,
            target_label: load_test.target_label.clone(),
            status_label: status_label(&load_test.status),
            connection_label: load_test.run_reuse_connections.map(connection_label),
            started_at,
            vus: load_test.run_vus,
            duration_secs: load_test.shown_duration_secs(),
            snapshot,
            compare,
            thresholds: load_test.run_thresholds.clone(),
            chart,
            window: load_test.window,
            window_only,
            targets,
            timeout_secs,
            log,
            analysis,
            target_count,
            run_secs,
            elapsed_secs,
            root: root.map(Path::to_path_buf),
            record,
        })
    }

    /// The covered range in whole seconds `[start, end)`: the window when only it is exported,
    /// the whole run (`None`) otherwise.
    fn range(&self) -> Option<Range<u32>> {
        self.window
            .filter(|_| self.window_only)
            .map(|(first, last)| first as u32..last as u32 + 1)
    }

    /// The statistics of the covered range. Reads the whole log: run it in the background.
    fn covered_stats(&self) -> Arc<WindowStats> {
        if let Some(stats) = &self.analysis {
            return stats.0.clone();
        }
        Arc::new(match &self.log {
            Some(log) => window_stats(
                log,
                self.target_count,
                self.run_secs,
                self.range(),
                self.elapsed_secs,
            ),
            None => stats_from_snapshot(&self.snapshot),
        })
    }

    /// The report of the covered range, with its texts in the interface language.
    fn report(&self, stats: &WindowStats) -> Report {
        let summary = SummaryData::from_stats(stats);
        let colors = ReportColors::light();
        let input = ReportInput {
            run_number: self.number,
            target_label: &self.target_label,
            status_label: &self.status_label,
            connection_label: self.connection_label.as_deref(),
            started_at: self.started_at,
            vus: self.vus,
            duration_secs: self.duration_secs,
            summary: &summary,
            compare: self
                .compare
                .as_ref()
                .map(|(number, snapshot)| (*number, snapshot)),
            thresholds: &self.thresholds,
            run_inputs: SummaryData::from_snapshot(&self.snapshot).threshold_inputs(),
            chart: self.chart.as_deref(),
            window: self.window,
            window_only: self.window_only,
            targets: &self.targets,
            error_groups: &stats.error_groups,
            error_examples: &self.snapshot.error_examples,
            timeout_secs: self.timeout_secs,
            generated_at: unix_now(),
            colors: &colors,
        };
        build_report(&input)
    }

    /// The suggested file name: `run-0003.md`, or `run-0003-34-46s.md` for a window.
    fn file_name(&self, format: ExportFormat) -> String {
        let window = self
            .window
            .filter(|_| self.window_only)
            .map(|(first, last)| format!("-{first}-{}s", last + 1))
            .unwrap_or_default();
        format!("run-{:04}{window}.{}", self.number, format.extension())
    }

    /// The bytes of `format`, from the report or the data. Slow: run it in the background.
    fn render(
        &self,
        format: ExportFormat,
        report: Option<&Report>,
        stats: &WindowStats,
    ) -> Result<Vec<u8>, String> {
        match (format, report) {
            (ExportFormat::Markdown, Some(report)) => Ok(to_markdown(report).into_bytes()),
            (ExportFormat::Pdf, Some(report)) => to_pdf(
                report,
                &PdfFonts {
                    regular: GEIST_REGULAR,
                    medium: GEIST_MEDIUM,
                    semibold: GEIST_SEMIBOLD,
                    mono: GEIST_MONO_REGULAR,
                },
            )
            .map_err(|error| error.to_string()),
            (ExportFormat::Csv, _) => {
                let empty = SampleLog::new();
                let log = self.log.as_deref().unwrap_or(&empty);
                Ok(requests_csv(log, &self.targets, self.range()).into_bytes())
            }
            (ExportFormat::Json, _) => {
                // The record saved when the run ended is the reference; the tab's own copy
                // stands in when it cannot be read.
                let saved = self
                    .root
                    .as_deref()
                    .and_then(|root| history::load(root, self.number).ok());
                let record = saved.as_ref().unwrap_or(&self.record);
                Ok(run_json(record, self.log.as_deref(), stats, self.range()).into_bytes())
            }
            _ => Err("no report to render".to_string()),
        }
    }
}

/// The current time as Unix seconds.
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

impl AppView {
    /// The export source of `tab_id`, or `None` for a tab that is not a finished load test.
    fn load_test_export_source(&self, tab_id: &str) -> Option<ExportSource> {
        let load_test = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get(index))
            .and_then(|tab| tab.load_test())?;
        ExportSource::from_tab(
            load_test,
            self.send_options.timeout.as_secs(),
            self.state
                .workspace
                .as_ref()
                .map(|workspace| workspace.root()),
        )
    }

    /// Asks where to save the results of `tab_id`'s run as `format`, then writes them there.
    pub(crate) fn export_load_test(
        &mut self,
        tab_id: &str,
        format: ExportFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.load_test_export_source(tab_id) else {
            return;
        };
        let directory = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&directory, Some(&source.file_name(format)));
        let source = Arc::new(source);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            let number = source.number;
            let covered = source.clone();
            let stats = cx
                .background_spawn(async move { covered.covered_stats() })
                .await;
            let report = if format.is_report() {
                let (source, stats) = (source.clone(), stats.clone());
                this.update_in(cx, |_, _, _| source.report(&stats)).ok()
            } else {
                None
            };
            let target = path.clone();
            let written = cx
                .background_spawn(async move {
                    let bytes = source.render(format, report.as_ref(), &stats)?;
                    std::fs::write(&target, bytes).map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update_in(cx, |_, window, cx| {
                let notification = match written {
                    Ok(()) => {
                        log::info!("load test run {number} exported as {}", format.name());
                        let shown = shorten_path(&path, dirs::home_dir().as_deref());
                        Notification::success(
                            t!("load_test.export.saved", path = shown).into_owned(),
                        )
                    }
                    Err(error) => {
                        log::warn!(
                            "load test run {number} could not be exported as {}: {error}",
                            format.name()
                        );
                        Notification::error(
                            t!("load_test.export.failed", error = error).into_owned(),
                        )
                    }
                };
                window.push_notification(notification, cx);
            });
        })
        .detach();
    }

    /// Handles the `Ctrl+Shift+C` / `Cmd+Shift+C` key binding (see `main.rs`'s `bind_keys`):
    /// copies the summary of the active tab when it is a load test, otherwise does nothing.
    pub(crate) fn on_copy_load_test_summary_action(
        &mut self,
        _: &CopyLoadTestSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .state
            .tabs
            .active()
            .filter(|tab| tab.load_test().is_some())
        else {
            return;
        };
        let tab_id = tab.id.clone();
        self.copy_load_test_summary(&tab_id, window, cx);
    }

    /// Copies the summary of `tab_id`'s run to the clipboard as Markdown.
    pub(crate) fn copy_load_test_summary(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.load_test_export_source(tab_id) else {
            return;
        };
        let source = Arc::new(source);
        cx.spawn_in(window, async move |this, cx| {
            let covered = source.clone();
            let stats = cx
                .background_spawn(async move { covered.covered_stats() })
                .await;
            let _ = this.update_in(cx, |_, window, cx| {
                let markdown = to_markdown(&source.report(&stats));
                cx.write_to_clipboard(ClipboardItem::new_string(markdown));
                log::info!("load test run {} summary copied as markdown", source.number);
                window.push_notification(
                    Notification::success(t!("load_test.export.copied").into_owned()),
                    cx,
                );
            });
        })
        .detach();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_core::Method;
    use postino_load::{SampleRecord, SecondPoint, StatusKey};
    // `gpui_kit::*` brings gpui's own `#[test]`, which needs a gpui test context.
    use std::prelude::v1::test;

    /// A finished run of 6 seconds with two requests per second, the last second all errors.
    fn finished_tab() -> LoadTestTab {
        let mut log = SampleLog::new();
        let url = log.intern("http://localhost/users");
        for second in 0..6u32 {
            for half in 0..2 {
                let failed = second == 5;
                log.push(SampleRecord {
                    elapsed_ms: second * 1_000 + half * 500,
                    vu: half,
                    target: 0,
                    url,
                    status: if failed {
                        StatusKey::Code(503)
                    } else {
                        StatusKey::Code(200)
                    },
                    latency_us: 20_000,
                    size: 100,
                    is_error: failed,
                });
            }
        }
        let stats = window_stats(&log, 1, 6, None, 6.0);
        let mut snapshot = LoadSnapshot {
            elapsed: std::time::Duration::from_secs(6),
            active_vus: 0,
            total: stats.total,
            rps: stats.rps,
            p50: stats.p50,
            p95: stats.p95,
            p99: stats.p99,
            error_rate: stats.error_rate,
            series: (0..6)
                .map(|_| SecondPoint {
                    rps: 2.0,
                    vus: 2,
                    p50: 20_000,
                    p90: 20_000,
                    p95: 20_000,
                    p99: 20_000,
                })
                .collect(),
            histogram: [0; 26],
            status_counts: stats.statuses.clone(),
            per_target: stats.per_target.clone(),
            error_examples: Vec::new(),
        };
        snapshot.histogram[2] = stats.total;
        let mut tab = LoadTestTab::for_request("users.postino".to_string(), "users".to_string());
        tab.status = LoadTestStatus::Finished;
        tab.run_number = Some(3);
        tab.run_duration_secs = Some(6);
        tab.target_rows = vec![(Method::Get, "users".to_string())];
        tab.snapshot = Some(snapshot);
        tab.replace_log(Some(log));
        tab
    }

    #[test]
    fn nothing_is_exported_while_running_or_without_results() {
        let mut tab = finished_tab();
        assert!(ExportSource::from_tab(&tab, 30, None).is_some());
        tab.status = LoadTestStatus::Running;
        assert!(ExportSource::from_tab(&tab, 30, None).is_none());
        assert!(ExportSource::from_tab(&LoadTestTab::unset(), 30, None).is_none());
    }

    #[test]
    fn the_file_name_and_range_follow_the_window() {
        let mut tab = finished_tab();
        tab.window = Some((2, 4));
        tab.export_window_only = false;
        let whole = ExportSource::from_tab(&tab, 30, None).expect("a finished run");
        assert_eq!(whole.file_name(ExportFormat::Markdown), "run-0003.md");
        assert_eq!(whole.range(), None);

        tab.export_window_only = true;
        let part = ExportSource::from_tab(&tab, 30, None).expect("a finished run");
        assert_eq!(part.file_name(ExportFormat::Pdf), "run-0003-2-5s.pdf");
        assert_eq!(part.range(), Some(2..5));
        assert_eq!(part.covered_stats().total, 6);

        // Without the requests there is no window to cut.
        tab.replace_log(None);
        tab.window = Some((2, 4));
        let no_log = ExportSource::from_tab(&tab, 30, None).expect("a finished run");
        assert_eq!(no_log.range(), None);
        assert_eq!(no_log.file_name(ExportFormat::Csv), "run-0003.csv");
        assert_eq!(no_log.covered_stats().total, 12);
    }

    #[test]
    fn every_format_renders() {
        let tab = finished_tab();
        let source = ExportSource::from_tab(&tab, 30, None).expect("a finished run");
        let stats = source.covered_stats();
        assert_eq!(stats.total, 12);
        let report = source.report(&stats);
        assert_eq!(report.title, "Run #3 \u{b7} users");
        assert_eq!(report.errors.len(), 1);

        let markdown = source
            .render(ExportFormat::Markdown, Some(&report), &stats)
            .expect("markdown");
        assert!(String::from_utf8_lossy(&markdown).contains("Run #3"));

        let pdf = source
            .render(ExportFormat::Pdf, Some(&report), &stats)
            .expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));

        let csv = source.render(ExportFormat::Csv, None, &stats).expect("csv");
        assert_eq!(String::from_utf8_lossy(&csv).lines().count(), 13);

        let json = source
            .render(ExportFormat::Json, None, &stats)
            .expect("json");
        let json: serde_json::Value = serde_json::from_slice(&json).expect("valid json");
        assert_eq!(json["run"]["number"], 3);
        assert_eq!(json["requests"].as_array().map(Vec::len), Some(12));
    }
}
