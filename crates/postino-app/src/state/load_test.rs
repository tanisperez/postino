//! Plain state behind the load test tab: what it targets, its editable configuration fields (with
//! plain-Rust parsing and clamping), and everything needed to render its dashboard.
//! `views/load_test/` renders this and owns the actual running [`postino_load::LoadRun`] and its
//! periodic refresh, the same split `views/send.rs` uses for sending a single request (`state/`
//! stays free of `gpui` types and of workspace file IO, so it is unit-tested directly).

use postino_core::Method;
use postino_load::history::RunRecordHeader;
use postino_load::{LoadSnapshot, Threshold, ThresholdMetric};
use postino_workspace::Node;

use rust_i18n::t;

use super::number::{format_decimal, format_signed_decimal};

/// What a load test tab targets: a single request, or every request of a folder ("collection"), in
/// tree order. Both variants carry the target's workspace id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadTestTarget {
    /// A single request, by its workspace id (for example `"auth/login.postino"`).
    Request(String),
    /// Every request under this folder, by the folder's workspace id.
    Collection(String),
}

/// Which of the two [`LoadTestTarget`] kinds the config panel's segmented control shows as
/// selected, independent of whether a target has actually been picked yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// A single request.
    Request,
    /// Every request of a folder.
    Collection,
}

/// The state of a load test run, shown as the dashboard header's badge: running
/// in accent, finished in success, stopped in warning, failed in danger.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadTestStatus {
    /// No run has started yet for this tab (or a previous run's history entry is being viewed,
    /// see [`LoadTestTab::view_history`]).
    NotStarted,
    /// A run is currently in progress.
    Running,
    /// The run completed its full configured duration.
    Finished,
    /// The run ended early: [`crate::views::load_test`]'s Stop button, or the configured error
    /// rate threshold.
    Stopped,
    /// The run could not be started at all, for example because its target resolved to no
    /// requests. `postino_load::LoadRun` itself never fails once started; this only covers setup
    /// failures before a run exists.
    Failed(LoadTestFailure),
}

/// Why a run could not be started. Kept as a value, not as text, so the message is translated
/// when it is shown and follows a language change.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadTestFailure {
    /// No workspace is open.
    NoWorkspace,
    /// The tab was closed while the run was being set up.
    TabClosed,
    /// No request or folder was picked yet.
    NoTarget,
    /// The picked folder has no requests.
    EmptyFolder,
    /// A request failed to load; carries the workspace error text, shown verbatim.
    Load(String),
}

/// The default number of virtual users.
pub const DEFAULT_VUS: u32 = 50;
/// The default run duration in seconds, matching the design.
pub const DEFAULT_DURATION_SECS: u64 = 60;
/// The default ramp-up time in seconds, matching the design.
pub const DEFAULT_RAMP_UP_SECS: u64 = 10;
/// The default think time in milliseconds, matching the design ("0" per iteration).
pub const DEFAULT_THINK_TIME_MS: u64 = 0;
/// The error rate threshold used when "Stop on errors" is on (an error rate
/// above 5%).
pub const STOP_ON_ERROR_RATE: f64 = 0.05;

/// Smallest number of virtual users accepted, per `postino_load::LoadConfig::vus`'s documented
/// range.
const VUS_MIN: u32 = 1;
/// Largest number of virtual users accepted, per `postino_load::LoadConfig::vus`'s documented
/// range.
const VUS_MAX: u32 = 500;
/// Largest run duration accepted, in seconds (24 hours). Not part of the design; a sensible
/// upper bound so a typo cannot start a run that outlives the app session.
const DURATION_SECS_MAX: u64 = 86_400;
/// Largest ramp-up time accepted, in seconds. Same rationale as [`DURATION_SECS_MAX`].
const RAMP_UP_SECS_MAX: u64 = 86_400;
/// Largest think time accepted, in milliseconds (10 minutes). Same rationale as
/// [`DURATION_SECS_MAX`].
const THINK_TIME_MS_MAX: u64 = 600_000;

/// Parses `raw` as a `u64`, clamped to `min..=max`; falls back to `default` (itself assumed to
/// already be in range) when `raw` does not parse as a plain non-negative integer.
fn parse_clamped(raw: &str, min: u64, max: u64, default: u64) -> u64 {
    raw.trim().parse::<u64>().unwrap_or(default).clamp(min, max)
}

/// Parses and clamps a virtual user count to `1..=500`
/// (`postino_load::LoadConfig::vus`'s documented range).
#[must_use]
pub fn clamp_vus(raw: &str) -> u32 {
    let value = parse_clamped(
        raw,
        u64::from(VUS_MIN),
        u64::from(VUS_MAX),
        u64::from(DEFAULT_VUS),
    );
    // `value` is already clamped into `VUS_MIN..=VUS_MAX`, both of which fit in a `u32`, so this
    // narrowing never truncates.
    u32::try_from(value).unwrap_or(DEFAULT_VUS)
}

/// Parses and clamps a run duration in seconds to `1..=86400`.
#[must_use]
pub fn clamp_duration_secs(raw: &str) -> u64 {
    parse_clamped(raw, 1, DURATION_SECS_MAX, DEFAULT_DURATION_SECS)
}

/// Parses and clamps a ramp-up time in seconds to `0..=86400`.
#[must_use]
pub fn clamp_ramp_up_secs(raw: &str) -> u64 {
    parse_clamped(raw, 0, RAMP_UP_SECS_MAX, DEFAULT_RAMP_UP_SECS)
}

/// Parses and clamps a think time in milliseconds to `0..=600000`.
#[must_use]
pub fn clamp_think_time_ms(raw: &str) -> u64 {
    parse_clamped(raw, 0, THINK_TIME_MS_MAX, DEFAULT_THINK_TIME_MS)
}

/// The raw text of the config panel's numeric inputs, kept as typed so a field never fights the
/// user mid-edit; [`Self::resolved`] parses and clamps them only when a value is actually needed
/// (starting a run, showing the ramp-up hint).
#[derive(Debug, Clone, PartialEq)]
pub struct LoadTestConfigInputs {
    /// The "Virtual users" field's current text.
    pub vus: String,
    /// The "Duration" field's current text, in seconds.
    pub duration_secs: String,
    /// The "Ramp-up" field's current text, in seconds.
    pub ramp_up_secs: String,
    /// The "Think time" field's current text, in milliseconds.
    pub think_time_ms: String,
    /// Whether "Stop on errors" is on.
    pub stop_on_error: bool,
    /// Whether "Reuse connections" is on.
    pub reuse_connections: bool,
    /// The "Thresholds" rows, in the order they were added, at most one per metric.
    pub thresholds: Vec<ThresholdInput>,
}

/// One "Thresholds" row of the config panel: its metric and its limit as typed, in the unit the
/// row shows (milliseconds, percent for the error rate, requests per second).
#[derive(Debug, Clone, PartialEq)]
pub struct ThresholdInput {
    /// The metric checked.
    pub metric: ThresholdMetric,
    /// The limit's text.
    pub limit: String,
}

impl ThresholdInput {
    /// A new row for `metric`, seeded with [`default_threshold_limit`].
    #[must_use]
    pub fn new(metric: ThresholdMetric) -> Self {
        Self {
            metric,
            limit: default_threshold_limit(metric).to_string(),
        }
    }

    /// The [`Threshold`] this row stands for, or `None` while its text is not a valid limit
    /// ([`parse_threshold_limit`]).
    #[must_use]
    pub fn resolved(&self) -> Option<Threshold> {
        parse_threshold_limit(self.metric, &self.limit).map(|limit| Threshold {
            metric: self.metric,
            limit,
        })
    }
}

/// The limit a newly added threshold row starts with, in the unit the row shows.
#[must_use]
pub fn default_threshold_limit(metric: ThresholdMetric) -> &'static str {
    match metric {
        ThresholdMetric::P50 => "100",
        ThresholdMetric::P95 => "250",
        ThresholdMetric::P99 => "500",
        ThresholdMetric::ErrorRate => "1",
        ThresholdMetric::Rps => "100",
    }
}

/// The unit a threshold row shows after its limit: `"ms"` for latencies, `"%"` for the error
/// rate, nothing for requests per second.
#[must_use]
pub fn threshold_unit(metric: ThresholdMetric) -> &'static str {
    match metric {
        ThresholdMetric::P50 | ThresholdMetric::P95 | ThresholdMetric::P99 => "ms",
        ThresholdMetric::ErrorRate => "%",
        ThresholdMetric::Rps => "",
    }
}

/// Parses a threshold row's text into a limit in the [`Threshold`] unit: milliseconds and
/// requests per second as typed, the error rate from percent to a fraction. Accepts a comma or a
/// period as the decimal separator. `None` for anything that is not a finite number of at least
/// zero, or an error rate above 100 %.
#[must_use]
pub fn parse_threshold_limit(metric: ThresholdMetric, raw: &str) -> Option<f64> {
    let value: f64 = raw.trim().replace(',', ".").parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    match metric {
        ThresholdMetric::ErrorRate if value > 100.0 => None,
        ThresholdMetric::ErrorRate => Some(value / 100.0),
        _ => Some(value),
    }
}

/// A threshold as one line of text, for example `"p99 < 50 ms"`, `"Errors < 0.5 %"` or
/// `"Requests/s >= 1,800"` (with the real greater-or-equal sign), numbers in the current
/// language.
#[must_use]
pub fn threshold_label(threshold: &Threshold) -> String {
    let (limit, unit) = match threshold.metric {
        ThresholdMetric::ErrorRate => (format_trimmed(threshold.limit * 100.0), " %"),
        ThresholdMetric::Rps => (format_trimmed(threshold.limit), ""),
        _ => (format_trimmed(threshold.limit), " ms"),
    };
    format!(
        "{} {} {limit}{unit}",
        threshold_metric_label(threshold.metric),
        threshold.metric.operator()
    )
}

/// The display name of a threshold metric: `"p50"`, `"p95"`, `"p99"`, or the translated
/// "Errors" and "Requests/s".
#[must_use]
pub fn threshold_metric_label(metric: ThresholdMetric) -> String {
    match metric {
        ThresholdMetric::P50 => "p50".to_string(),
        ThresholdMetric::P95 => "p95".to_string(),
        ThresholdMetric::P99 => "p99".to_string(),
        ThresholdMetric::ErrorRate => t!("load_test.kpi.errors").into_owned(),
        ThresholdMetric::Rps => t!("load_test.kpi.rps").into_owned(),
    }
}

/// `value` with up to two decimals and no trailing zeros, in the current language: `50`, `0.5`,
/// `1,800`.
#[must_use]
pub fn format_trimmed(value: f64) -> String {
    let decimals = if (value - value.round()).abs() < 1e-9 {
        0
    } else if (value * 10.0 - (value * 10.0).round()).abs() < 1e-9 {
        1
    } else {
        2
    };
    format_decimal(value, decimals)
}

impl Default for LoadTestConfigInputs {
    fn default() -> Self {
        Self {
            vus: DEFAULT_VUS.to_string(),
            duration_secs: DEFAULT_DURATION_SECS.to_string(),
            ramp_up_secs: DEFAULT_RAMP_UP_SECS.to_string(),
            think_time_ms: DEFAULT_THINK_TIME_MS.to_string(),
            stop_on_error: true,
            reuse_connections: true,
            thresholds: Vec::new(),
        }
    }
}

impl LoadTestConfigInputs {
    /// Parses and clamps every field, ready to build a `postino_load::LoadConfig` from (once the
    /// caller has resolved the actual `postino_load::LoadTarget`s).
    #[must_use]
    pub fn resolved(&self) -> ResolvedLoadTestConfig {
        ResolvedLoadTestConfig {
            vus: clamp_vus(&self.vus),
            duration_secs: clamp_duration_secs(&self.duration_secs),
            ramp_up_secs: clamp_ramp_up_secs(&self.ramp_up_secs),
            think_time_ms: clamp_think_time_ms(&self.think_time_ms),
            stop_on_error_rate: self.stop_on_error.then_some(STOP_ON_ERROR_RATE),
            reuse_connections: self.reuse_connections,
            thresholds: self
                .thresholds
                .iter()
                .filter_map(ThresholdInput::resolved)
                .collect(),
        }
    }
}

/// [`LoadTestConfigInputs`], parsed and clamped, in the plain numbers/durations
/// `postino_load::LoadConfig` needs (`views/load_test/run.rs` converts the durations).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLoadTestConfig {
    /// The number of virtual users, `1..=500`.
    pub vus: u32,
    /// The run duration, in seconds.
    pub duration_secs: u64,
    /// The ramp-up time, in seconds.
    pub ramp_up_secs: u64,
    /// The think time, in milliseconds.
    pub think_time_ms: u64,
    /// The error rate threshold, when "Stop on errors" is on.
    pub stop_on_error_rate: Option<f64>,
    /// Whether each virtual user reuses its connections (`postino_load::LoadConfig`).
    pub reuse_connections: bool,
    /// The thresholds whose limit parses; a row still being typed is left out.
    pub thresholds: Vec<Threshold>,
}

/// A load test tab's state: held as `crate::state::tabs::TabKind::LoadTest` inside an open tab.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadTestTab {
    /// Which kind of target the segmented control shows as selected.
    pub target_kind: TargetKind,
    /// The actual target picked, if any. `None` until the user picks one (a tab opened from the
    /// command palette starts this way; one opened from a sidebar context menu is preselected,
    /// see [`Self::for_request`]/[`Self::for_collection`]).
    pub target: Option<LoadTestTarget>,
    /// The target's display name, empty until [`Self::target`] is set.
    pub target_label: String,
    /// How many requests the target resolves to (1 for a single request), for the target
    /// picker's "N requests" hint. `0` until [`Self::target`] is set.
    pub target_request_count: usize,
    /// The editable configuration fields.
    pub config: LoadTestConfigInputs,
    /// The current run's status.
    pub status: LoadTestStatus,
    /// The run number assigned when a run starts, or when a history entry is being viewed
    /// (1-based, matching the saved file name).
    pub run_number: Option<u32>,
    /// The metrics to show: live while running, final once finished or stopped, or a past run's
    /// when viewing history. `None` before the first run.
    pub snapshot: Option<LoadSnapshot>,
    /// The run number selected in the "Compare with" dropdown, if any.
    pub compare_with: Option<u32>,
    /// The full snapshot of [`Self::compare_with`]'s run, loaded once when it is picked (rather
    /// than from every render) so the "Compare with" panel never re-reads a run file from disk on
    /// every 250 ms live refresh (`views/load_test/mod.rs`'s
    /// `AppView::refresh_load_test_compare_snapshot`). `None` while [`Self::compare_with`] is
    /// `None`, or if that run's file could not be loaded.
    pub compare_snapshot: Option<LoadSnapshot>,
    /// Every previous run recorded for this tab's target, newest first, refreshed after a run
    /// finishes and whenever the target changes (`views/load_test/mod.rs`).
    pub history: Vec<RunRecordHeader>,
    /// Each target's HTTP method and display name, in the same order as the current (running,
    /// just-finished, or viewed-from-history) run's [`postino_load::LoadSnapshot::per_target`].
    /// Set when a run starts (`views/load_test/run.rs`) and restored from
    /// [`postino_load::history::RunRecord::target_labels`] when a past run is viewed
    /// (`views/load_test/mod.rs`'s `AppView::view_load_test_history`). Used to label the
    /// per-request table's rows; empty (falling back to a generic "Target N" label, see
    /// [`crate::views::load_test::dashboard::target_row_label`]) only for a run saved before that
    /// `RunRecord` field existed.
    pub target_rows: Vec<(Method, String)>,
    /// Whether the run shown reused its connections: set when a run starts and when a past run
    /// is viewed, since the config panel may have changed since. `None` before the first run.
    pub run_reuse_connections: Option<bool>,
    /// The duration the run shown was configured with, in seconds: set when a run starts and
    /// when a past run is viewed, since the config panel may have changed since (#135). `None`
    /// before the first run.
    pub run_duration_secs: Option<u64>,
    /// The thresholds the run shown is checked against: set when a run starts and when a past
    /// run is viewed, since the config panel may have changed since.
    pub run_thresholds: Vec<Threshold>,
}

impl LoadTestTab {
    /// A tab opened with no preselected target (the command palette's "New load test" action).
    #[must_use]
    pub fn unset() -> Self {
        Self {
            target_kind: TargetKind::Request,
            target: None,
            target_label: String::new(),
            target_request_count: 0,
            config: LoadTestConfigInputs::default(),
            status: LoadTestStatus::NotStarted,
            run_number: None,
            snapshot: None,
            compare_with: None,
            compare_snapshot: None,
            history: Vec::new(),
            target_rows: Vec::new(),
            run_reuse_connections: None,
            run_duration_secs: None,
            run_thresholds: Vec::new(),
        }
    }

    /// The total duration the dashboard header measures the run shown against: the run's own
    /// duration once a run has started or a past run is viewed, the config panel's before that.
    #[must_use]
    pub fn shown_duration_secs(&self) -> u64 {
        self.run_duration_secs
            .unwrap_or_else(|| self.config.resolved().duration_secs)
    }

    /// A tab preselecting the single request `id` (a sidebar request row's "Load test..." menu
    /// item). `label` is the request's display name (its file stem).
    #[must_use]
    pub fn for_request(id: String, label: String) -> Self {
        Self {
            target_kind: TargetKind::Request,
            target: Some(LoadTestTarget::Request(id)),
            target_label: label,
            target_request_count: 1,
            ..Self::unset()
        }
    }

    /// A tab preselecting every request under the folder `id` (a sidebar folder row's "Load
    /// test..." menu item). `label` is the folder's name, `request_count` how many requests it
    /// contains ([`count_requests`]).
    #[must_use]
    pub fn for_collection(id: String, label: String, request_count: usize) -> Self {
        Self {
            target_kind: TargetKind::Collection,
            target: Some(LoadTestTarget::Collection(id)),
            target_label: label,
            target_request_count: request_count,
            ..Self::unset()
        }
    }

    /// Whether a run is currently in progress: the config panel disables its fields and shows
    /// "Stop run" while this is true.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.status == LoadTestStatus::Running
    }

    /// The metrics that have no threshold row yet, for the "Add threshold" menu.
    #[must_use]
    pub fn missing_threshold_metrics(&self) -> Vec<ThresholdMetric> {
        ThresholdMetric::ALL
            .into_iter()
            .filter(|metric| {
                !self
                    .config
                    .thresholds
                    .iter()
                    .any(|input| input.metric == *metric)
            })
            .collect()
    }
}

/// Recursively counts every request under `nodes` (a folder's children, or a whole tree).
#[must_use]
pub fn count_requests(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Node::Folder(folder) => count_requests(&folder.children),
            Node::Request(_) => 1,
        })
        .sum()
}

/// Finds the folder `id` anywhere in `nodes` and returns its display name and request count
/// ([`count_requests`] of its children).
#[must_use]
pub fn find_folder(nodes: &[Node], id: &str) -> Option<(String, usize)> {
    for node in nodes {
        if let Node::Folder(folder) = node {
            if folder.id == id {
                return Some((folder.name.clone(), count_requests(&folder.children)));
            }
            if let Some(found) = find_folder(&folder.children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Finds the request `id` anywhere in `nodes` and returns its display name (its file stem, not
/// the full path, matching the sidebar's own row label).
#[must_use]
pub fn find_request_name(nodes: &[Node], id: &str) -> Option<String> {
    for node in nodes {
        match node {
            Node::Folder(folder) => {
                if let Some(found) = find_request_name(&folder.children, id) {
                    return Some(found);
                }
            }
            Node::Request(request) if request.id == id => return Some(request.name.clone()),
            Node::Request(_) => {}
        }
    }
    None
}

/// Every non-broken request in `nodes`, in tree order: its workspace id and a display label (its
/// id with the `.postino` extension stripped, for example `"auth/login"`), for the target
/// picker's "Request" list.
#[must_use]
pub fn requests_flat(nodes: &[Node]) -> Vec<(String, String)> {
    let mut items = Vec::new();
    collect_requests_flat(nodes, &mut items);
    items
}

fn collect_requests_flat(nodes: &[Node], items: &mut Vec<(String, String)>) {
    for node in nodes {
        match node {
            Node::Folder(folder) => collect_requests_flat(&folder.children, items),
            Node::Request(request) => {
                if request.broken.is_some() {
                    continue;
                }
                let label = request
                    .id
                    .strip_suffix(".postino")
                    .unwrap_or(&request.id)
                    .to_string();
                items.push((request.id.clone(), label));
            }
        }
    }
}

/// Every folder in `nodes`, in tree order: its workspace id, name and request count
/// ([`count_requests`]), for the target picker's "Collection" list. A folder with no requests at
/// all (only empty subfolders) is still listed; [`crate::views::load_test`] refuses to start a
/// run against it.
#[must_use]
pub fn folders_flat(nodes: &[Node]) -> Vec<(String, String, usize)> {
    let mut items = Vec::new();
    collect_folders_flat(nodes, &mut items);
    items
}

fn collect_folders_flat(nodes: &[Node], items: &mut Vec<(String, String, usize)>) {
    for node in nodes {
        if let Node::Folder(folder) = node {
            items.push((
                folder.id.clone(),
                folder.name.clone(),
                count_requests(&folder.children),
            ));
            collect_folders_flat(&folder.children, items);
        }
    }
}

/// The ids of every non-broken request under the folder `id` (or every request in `nodes` when
/// `id` is not found, which should not happen: [`views::load_test`] only ever passes an id from
/// [`folders_flat`]), in tree order.
#[must_use]
pub fn request_ids_under(nodes: &[Node], id: &str) -> Vec<String> {
    fn find_folder_children<'a>(nodes: &'a [Node], id: &str) -> Option<&'a [Node]> {
        for node in nodes {
            if let Node::Folder(folder) = node {
                if folder.id == id {
                    return Some(&folder.children);
                }
                if let Some(found) = find_folder_children(&folder.children, id) {
                    return Some(found);
                }
            }
        }
        None
    }

    let children = find_folder_children(nodes, id).unwrap_or(&[]);
    requests_flat(children)
        .into_iter()
        .map(|(id, _label)| id)
        .collect()
}

/// How a "Compare with" delta reads: better, worse, or no visible change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaTone {
    /// The metric moved in the good direction (success color).
    Better,
    /// The metric moved in the bad direction (danger color).
    Worse,
    /// The change rounds to zero at one decimal (muted color, no sign).
    Neutral,
}

/// Classifies `change` (displayed with one decimal, see [`format_delta`]) for a metric where
/// `higher_is_better` says which direction is the good one. A change that rounds to `0.0` is
/// [`DeltaTone::Neutral`] whichever direction the metric prefers.
pub fn delta_tone(change: f64, higher_is_better: bool) -> DeltaTone {
    if (change * 10.0).round() == 0.0 {
        DeltaTone::Neutral
    } else if (change > 0.0) == higher_is_better {
        DeltaTone::Better
    } else {
        DeltaTone::Worse
    }
}

/// Formats `change` with one decimal and `unit` appended (for example `"%"` or `" pt"`): signed
/// for any non-zero change, and plain `0.0` (never `+0.0` or `-0.0`) when it rounds to zero.
pub fn format_delta(change: f64, unit: &str) -> String {
    if (change * 10.0).round() == 0.0 {
        format!("{}{unit}", format_decimal(0.0, 1))
    } else {
        format!("{}{unit}", format_signed_decimal(change, 1))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_core::Method;
    use postino_workspace::{Folder, RequestEntry};
    use pretty_assertions::assert_eq;

    fn request(id: &str, name: &str, broken: bool) -> Node {
        Node::Request(RequestEntry {
            id: id.to_string(),
            name: name.to_string(),
            broken: broken.then(|| "broken".to_string()),
            method: (!broken).then_some(Method::Get),
        })
    }

    fn folder(id: &str, name: &str, children: Vec<Node>) -> Node {
        Node::Folder(Folder {
            id: id.to_string(),
            name: name.to_string(),
            children,
        })
    }

    fn sample_tree() -> Vec<Node> {
        vec![
            folder(
                "users",
                "users",
                vec![
                    request("users/create.postino", "create", false),
                    request("users/list.postino", "list", false),
                    request("users/broken.postino", "broken", true),
                ],
            ),
            folder("empty", "empty", vec![]),
            request("health.postino", "health", false),
        ]
    }

    #[test]
    fn clamp_vus_parses_and_clamps_to_the_engines_range() {
        assert_eq!(clamp_vus("50"), 50);
        assert_eq!(clamp_vus("0"), 1);
        assert_eq!(clamp_vus("5000"), 500);
    }

    #[test]
    fn clamp_vus_falls_back_to_the_default_on_garbage_input() {
        assert_eq!(clamp_vus(""), DEFAULT_VUS);
        assert_eq!(clamp_vus("not a number"), DEFAULT_VUS);
        assert_eq!(clamp_vus("-3"), DEFAULT_VUS);
        assert_eq!(clamp_vus("12.5"), DEFAULT_VUS);
    }

    #[test]
    fn clamp_duration_secs_clamps_to_at_least_one_second() {
        assert_eq!(clamp_duration_secs("0"), 1);
        assert_eq!(clamp_duration_secs("60"), 60);
        assert_eq!(clamp_duration_secs("999999999"), DURATION_SECS_MAX);
    }

    #[test]
    fn clamp_ramp_up_secs_allows_zero() {
        assert_eq!(clamp_ramp_up_secs("0"), 0);
        assert_eq!(clamp_ramp_up_secs("10"), 10);
    }

    #[test]
    fn clamp_think_time_ms_allows_zero_and_clamps_the_top() {
        assert_eq!(clamp_think_time_ms("0"), 0);
        assert_eq!(clamp_think_time_ms("500"), 500);
        assert_eq!(clamp_think_time_ms("999999999"), THINK_TIME_MS_MAX);
    }

    #[test]
    fn shown_duration_is_the_panel_s_until_a_run_is_shown() {
        let mut tab = LoadTestTab::unset();
        assert_eq!(tab.shown_duration_secs(), DEFAULT_DURATION_SECS);
        tab.config.duration_secs = "30".to_string();
        assert_eq!(tab.shown_duration_secs(), 30);
    }

    /// A past run viewed from history keeps its own duration whatever the panel says (#135).
    #[test]
    fn shown_duration_is_the_run_s_once_a_run_is_shown() {
        let mut tab = LoadTestTab::unset();
        tab.run_duration_secs = Some(10);
        assert_eq!(tab.config.resolved().duration_secs, DEFAULT_DURATION_SECS);
        assert_eq!(tab.shown_duration_secs(), 10);
    }

    #[test]
    fn resolved_config_uses_the_stop_on_error_rate_only_when_the_switch_is_on() {
        let mut inputs = LoadTestConfigInputs::default();
        assert_eq!(
            inputs.resolved().stop_on_error_rate,
            Some(STOP_ON_ERROR_RATE)
        );
        inputs.stop_on_error = false;
        assert_eq!(inputs.resolved().stop_on_error_rate, None);
    }

    #[test]
    fn connections_are_reused_by_default() {
        let mut inputs = LoadTestConfigInputs::default();
        assert!(inputs.resolved().reuse_connections);
        inputs.reuse_connections = false;
        assert!(!inputs.resolved().reuse_connections);
    }

    #[test]
    fn default_config_inputs_round_trip_through_the_clamp_functions() {
        let inputs = LoadTestConfigInputs::default();
        let resolved = inputs.resolved();
        assert_eq!(resolved.vus, DEFAULT_VUS);
        assert_eq!(resolved.duration_secs, DEFAULT_DURATION_SECS);
        assert_eq!(resolved.ramp_up_secs, DEFAULT_RAMP_UP_SECS);
        assert_eq!(resolved.think_time_ms, DEFAULT_THINK_TIME_MS);
    }

    #[test]
    fn for_request_preselects_a_single_request_target() {
        let tab = LoadTestTab::for_request("auth/login.postino".to_string(), "login".to_string());
        assert_eq!(
            tab.target,
            Some(LoadTestTarget::Request("auth/login.postino".to_string()))
        );
        assert_eq!(tab.target_kind, TargetKind::Request);
        assert_eq!(tab.target_request_count, 1);
        assert_eq!(tab.status, LoadTestStatus::NotStarted);
    }

    #[test]
    fn for_collection_preselects_a_folder_target_with_its_request_count() {
        let tab = LoadTestTab::for_collection("users".to_string(), "users".to_string(), 3);
        assert_eq!(
            tab.target,
            Some(LoadTestTarget::Collection("users".to_string()))
        );
        assert_eq!(tab.target_kind, TargetKind::Collection);
        assert_eq!(tab.target_request_count, 3);
    }

    #[test]
    fn unset_tab_has_no_target() {
        let tab = LoadTestTab::unset();
        assert!(tab.target.is_none());
        assert_eq!(tab.target_request_count, 0);
    }

    #[test]
    fn is_running_only_while_the_status_is_running() {
        let mut tab = LoadTestTab::unset();
        assert!(!tab.is_running());
        tab.status = LoadTestStatus::Running;
        assert!(tab.is_running());
        tab.status = LoadTestStatus::Finished;
        assert!(!tab.is_running());
    }

    #[test]
    fn count_requests_counts_recursively_and_skips_nothing() {
        assert_eq!(count_requests(&sample_tree()), 4);
    }

    #[test]
    fn find_folder_returns_name_and_request_count() {
        let tree = sample_tree();
        assert_eq!(find_folder(&tree, "users"), Some(("users".to_string(), 3)));
        assert_eq!(find_folder(&tree, "empty"), Some(("empty".to_string(), 0)));
        assert_eq!(find_folder(&tree, "missing"), None);
    }

    #[test]
    fn find_request_name_finds_a_nested_request_by_id() {
        let tree = sample_tree();
        assert_eq!(
            find_request_name(&tree, "users/create.postino"),
            Some("create".to_string())
        );
        assert_eq!(find_request_name(&tree, "missing.postino"), None);
    }

    #[test]
    fn requests_flat_skips_broken_files_and_strips_the_extension() {
        let tree = sample_tree();
        let items = requests_flat(&tree);
        let labels: Vec<&str> = items.iter().map(|(_, label)| label.as_str()).collect();
        assert_eq!(labels, vec!["users/create", "users/list", "health"]);
    }

    #[test]
    fn folders_flat_lists_every_folder_with_its_request_count() {
        let tree = sample_tree();
        let items = folders_flat(&tree);
        assert_eq!(
            items,
            vec![
                ("users".to_string(), "users".to_string(), 3),
                ("empty".to_string(), "empty".to_string(), 0),
            ]
        );
    }

    #[test]
    fn request_ids_under_lists_only_the_folders_own_requests() {
        let tree = sample_tree();
        assert_eq!(
            request_ids_under(&tree, "users"),
            vec![
                "users/create.postino".to_string(),
                "users/list.postino".to_string()
            ]
        );
        assert!(request_ids_under(&tree, "empty").is_empty());
    }

    #[test]
    fn threshold_limits_accept_a_comma_or_a_period_and_errors_are_percent() {
        assert_eq!(
            parse_threshold_limit(ThresholdMetric::P99, " 50 "),
            Some(50.0)
        );
        assert_eq!(
            parse_threshold_limit(ThresholdMetric::P99, "12,5"),
            Some(12.5)
        );
        assert_eq!(
            parse_threshold_limit(ThresholdMetric::ErrorRate, "0.5"),
            Some(0.005)
        );
        assert_eq!(
            parse_threshold_limit(ThresholdMetric::ErrorRate, "101"),
            None
        );
        assert_eq!(parse_threshold_limit(ThresholdMetric::Rps, "-1"), None);
        assert_eq!(parse_threshold_limit(ThresholdMetric::Rps, ""), None);
        assert_eq!(parse_threshold_limit(ThresholdMetric::Rps, "inf"), None);
    }

    #[test]
    fn resolved_config_skips_threshold_rows_that_do_not_parse() {
        let inputs = LoadTestConfigInputs {
            thresholds: vec![
                ThresholdInput::new(ThresholdMetric::P99),
                ThresholdInput {
                    metric: ThresholdMetric::Rps,
                    limit: "abc".to_string(),
                },
            ],
            ..LoadTestConfigInputs::default()
        };
        assert_eq!(
            inputs.resolved().thresholds,
            vec![Threshold {
                metric: ThresholdMetric::P99,
                limit: 500.0
            }]
        );
    }

    #[test]
    fn missing_threshold_metrics_leaves_out_the_ones_already_added() {
        let mut tab = LoadTestTab::unset();
        tab.config.thresholds = vec![ThresholdInput::new(ThresholdMetric::ErrorRate)];
        assert_eq!(
            tab.missing_threshold_metrics(),
            vec![
                ThresholdMetric::P50,
                ThresholdMetric::P95,
                ThresholdMetric::P99,
                ThresholdMetric::Rps
            ]
        );
    }

    #[test]
    fn threshold_label_reads_like_the_design() {
        let p99 = Threshold {
            metric: ThresholdMetric::P99,
            limit: 50.0,
        };
        assert_eq!(threshold_label(&p99), "p99 < 50 ms");
        let errors = Threshold {
            metric: ThresholdMetric::ErrorRate,
            limit: 0.005,
        };
        assert_eq!(threshold_label(&errors), "Errors < 0.5 %");
        let rps = Threshold {
            metric: ThresholdMetric::Rps,
            limit: 1_800.0,
        };
        assert_eq!(threshold_label(&rps), "Requests/s \u{2265} 1,800");
    }

    #[test]
    fn delta_tone_higher_is_better() {
        assert_eq!(delta_tone(5.0, true), DeltaTone::Better);
        assert_eq!(delta_tone(-5.0, true), DeltaTone::Worse);
        assert_eq!(delta_tone(0.0, true), DeltaTone::Neutral);
        assert_eq!(delta_tone(0.04, true), DeltaTone::Neutral);
        assert_eq!(delta_tone(-0.04, true), DeltaTone::Neutral);
    }

    #[test]
    fn delta_tone_lower_is_better() {
        assert_eq!(delta_tone(-5.0, false), DeltaTone::Better);
        assert_eq!(delta_tone(5.0, false), DeltaTone::Worse);
        assert_eq!(delta_tone(0.0, false), DeltaTone::Neutral);
        assert_eq!(delta_tone(0.04, false), DeltaTone::Neutral);
        assert_eq!(delta_tone(-0.04, false), DeltaTone::Neutral);
    }

    #[test]
    fn format_delta_signs_only_non_zero_changes() {
        assert_eq!(format_delta(2.34, "%"), "+2.3%");
        assert_eq!(format_delta(-2.34, " pt"), "-2.3 pt");
        assert_eq!(format_delta(0.0, " pt"), "0.0 pt");
        assert_eq!(format_delta(0.04, "%"), "0.0%");
        assert_eq!(format_delta(-0.04, "%"), "0.0%");
    }
}
