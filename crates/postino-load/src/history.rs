//! Run history: one JSON file per load test run, saved under `<workspace>/.postino/runs/`.
//!
//! This is a documented exception to "`postino-workspace` owns the filesystem": `postino-load` does
//! its own IO here to avoid a `postino-workspace -> postino-load -> postino-runner` dependency
//! edge, which would break the acyclic dependency graph of `AGENTS.md`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::LoadConfig;
use crate::metrics::LoadSnapshot;

/// The subfolder run history is stored in, relative to a workspace root. The workspace scanner
/// already skips hidden folders, so this never shows up as a collection.
const RUNS_DIR: &str = ".postino/runs";

/// Everything that can go wrong reading or writing run history.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// A filesystem operation failed: creating `.postino/runs/`, or reading or writing a run
    /// file.
    #[error("io error at {path:?}: {source}")]
    Io {
        /// The path the operation was attempted on.
        path: PathBuf,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// A run record could not be serialized to JSON. Should not happen for the plain data this
    /// crate produces; kept as a typed error instead of a panic, matching every other fallible
    /// step in this crate.
    #[error("could not encode {path:?}: {source}")]
    Encode {
        /// The file that was being written.
        path: PathBuf,
        /// The underlying serialization error.
        #[source]
        source: serde_json::Error,
    },
    /// A run file's content is not valid JSON, or not shaped like a [`RunRecord`].
    #[error("could not parse {path:?}: {source}")]
    Decode {
        /// The file that was being read.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// A [`LoadConfig`] without the actual [`postino_core::Request`] values, for storing alongside a
/// [`RunRecord`] (a config summary). This avoids
/// needing `serde` support on `postino-core`'s domain types just to persist history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadConfigSummary {
    /// The number of virtual users the run was configured with.
    pub vus: u32,
    /// The configured run duration, in seconds.
    pub duration_secs: u64,
    /// The configured ramp-up time, in seconds.
    pub ramp_up_secs: u64,
    /// The configured think time, in milliseconds.
    pub think_time_ms: u64,
    /// The configured error rate threshold, if any (a fraction, for example `0.05` for 5%).
    pub stop_on_error_rate: Option<f64>,
    /// How many targets (requests) the run looped over.
    pub target_count: usize,
}

impl From<&LoadConfig> for LoadConfigSummary {
    /// Builds a summary from the config a run was started with.
    fn from(config: &LoadConfig) -> Self {
        Self {
            vus: config.vus,
            duration_secs: config.duration.as_secs(),
            ramp_up_secs: config.ramp_up.as_secs(),
            think_time_ms: u64::try_from(config.think_time.as_millis()).unwrap_or(u64::MAX),
            stop_on_error_rate: config.stop_on_error_rate,
            target_count: config.targets.len(),
        }
    }
}

/// A single run's saved history entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    /// The run number, matching its file name (4
    /// digits, zero padded).
    pub number: u32,
    /// When the run started, as Unix seconds.
    pub started_at_unix: u64,
    /// A human label for what the run targeted, for example a request's name, or "3 requests"
    /// for a collection.
    pub target_label: String,
    /// The configuration the run used, without the request bodies (see [`LoadConfigSummary`]).
    pub config: LoadConfigSummary,
    /// The metrics at the moment the run ended.
    pub snapshot: LoadSnapshot,
    /// Whether the run ended before its configured duration fully elapsed (stopped by hand, or
    /// by [`crate::LoadConfig::stop_on_error_rate`]), the same meaning as
    /// [`crate::LoadSummary::stopped_early`]: the app shows this run as "Stopped" when `true`,
    /// "Finished" when `false`. `#[serde(default)]` (`false`) for a run file saved before this
    /// field existed, which falls back to always reading as "Finished", the only status those
    /// older records could show anyway.
    #[serde(default)]
    pub stopped_early: bool,
    /// Each target's HTTP method (as its plain token, e.g. `"GET"`, `"POST"`, or a custom
    /// method exactly as written) and display name, in the same order as
    /// [`crate::LoadSnapshot::per_target`], for the per-request table of a run viewed from
    /// history. A plain `(String, String)` pair rather than [`postino_core::Method`] directly:
    /// `postino-core` intentionally has no `serde` dependency (see [`LoadConfigSummary`]'s own
    /// doc comment), and `Method`'s `Display`/`FromStr` round-trip a token losslessly (`FromStr`
    /// is infallible, an unrecognized token becomes `Method::Custom`). `#[serde(default)]`
    /// (empty) for a run file saved before this field existed, which falls back to the generic
    /// "Target N" label those older records already showed.
    #[serde(default)]
    pub target_labels: Vec<(String, String)>,
}

/// A lightweight preview of a [`RunRecord`], for a run list or a "Compare with" dropdown, without
/// reading the heavier fields of the full record (the histogram, the series and the per-target
/// table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecordHeader {
    /// The run number.
    pub number: u32,
    /// When the run started, as Unix seconds.
    pub started_at_unix: u64,
    /// The `target_label` of the underlying [`RunRecord`].
    pub target_label: String,
    /// The total sample count of the underlying [`RunRecord`].
    pub total: u64,
    /// The requests per second of the underlying [`RunRecord`].
    pub rps: f64,
    /// The p95 latency of the underlying [`RunRecord`], in microseconds.
    pub p95: u32,
    /// The error rate of the underlying [`RunRecord`] (`0.0` to `1.0`).
    pub error_rate: f64,
}

impl From<&RunRecord> for RunRecordHeader {
    /// Extracts the header fields from a full record.
    fn from(record: &RunRecord) -> Self {
        Self {
            number: record.number,
            started_at_unix: record.started_at_unix,
            target_label: record.target_label.clone(),
            total: record.snapshot.total,
            rps: record.snapshot.rps,
            p95: record.snapshot.p95,
            error_rate: record.snapshot.error_rate,
        }
    }
}

/// The path a run file for `number` would have under `root`.
fn run_path(root: &Path, number: u32) -> PathBuf {
    root.join(RUNS_DIR).join(format!("{number:04}.json"))
}

/// Saves `record` to `<root>/.postino/runs/<NNNN>.json`, creating the folder if it does not
/// exist yet. Overwrites any existing file for the same run number.
pub fn save(root: &Path, record: &RunRecord) -> Result<(), HistoryError> {
    let dir = root.join(RUNS_DIR);
    fs::create_dir_all(&dir).map_err(|source| HistoryError::Io { path: dir, source })?;

    let path = run_path(root, record.number);
    let json = serde_json::to_string_pretty(record).map_err(|source| HistoryError::Encode {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, json).map_err(|source| HistoryError::Io { path, source })
}

/// Loads the run numbered `number` from `<root>/.postino/runs/`.
pub fn load(root: &Path, number: u32) -> Result<RunRecord, HistoryError> {
    let path = run_path(root, number);
    let text = fs::read_to_string(&path).map_err(|source| HistoryError::Io {
        path: path.clone(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| HistoryError::Decode { path, source })
}

/// Lists every run saved under `<root>/.postino/runs/`, newest first (highest run number
/// first). A file that fails to read or parse is skipped rather than failing the whole listing:
/// one corrupt run should not hide every other one. Returns an empty list if the folder does not
/// exist yet.
#[must_use]
pub fn list(root: &Path) -> Vec<RunRecordHeader> {
    let dir = root.join(RUNS_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut headers: Vec<RunRecordHeader> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let text = fs::read_to_string(entry.path()).ok()?;
            let record: RunRecord = serde_json::from_str(&text).ok()?;
            Some(RunRecordHeader::from(&record))
        })
        .collect();

    headers.sort_by_key(|header| std::cmp::Reverse(header.number));
    headers
}

/// The next run number to use under `<root>/.postino/runs/`: one more than the highest existing
/// run file's number, or `1` if the folder does not exist or has none.
///
/// Based only on file names, not their content, so a corrupt file still reserves its number and
/// is never silently overwritten by [`save`].
#[must_use]
pub fn next_number(root: &Path) -> u32 {
    let dir = root.join(RUNS_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return 1;
    };

    let highest = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            path.file_stem()?.to_str()?.parse::<u32>().ok()
        })
        .max();

    highest.map_or(1, |number| number + 1)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;
    use crate::config::LoadTarget;
    use crate::metrics::{LoadSnapshot, SecondPoint, TargetStats};
    use crate::status::StatusKey;

    /// A minimal but structurally complete snapshot, for tests that only care about history
    /// plumbing, not the metrics themselves.
    fn sample_snapshot() -> LoadSnapshot {
        LoadSnapshot {
            elapsed: Duration::from_secs(1),
            active_vus: 4,
            total: 10,
            rps: 10.0,
            p50: 1_000,
            p95: 2_000,
            p99: 3_000,
            error_rate: 0.1,
            series: vec![SecondPoint {
                rps: 10.0,
                p95: 2_000,
            }],
            histogram: [0; 26],
            status_counts: vec![(StatusKey::Code(200), 9), (StatusKey::Code(500), 1)],
            per_target: vec![TargetStats {
                count: 10,
                p50: 1_000,
                p95: 2_000,
                p99: 3_000,
                error_rate: 0.1,
            }],
        }
    }

    fn sample_record(number: u32) -> RunRecord {
        let config = LoadConfig {
            targets: vec![LoadTarget::new("login", postino_core::Request::default())],
            vus: 4,
            duration: Duration::from_secs(60),
            ramp_up: Duration::from_secs(10),
            think_time: Duration::from_millis(500),
            stop_on_error_rate: Some(0.05),
        };
        RunRecord {
            number,
            started_at_unix: 1_700_000_000,
            target_label: "login".to_string(),
            config: LoadConfigSummary::from(&config),
            snapshot: sample_snapshot(),
            stopped_early: false,
            target_labels: vec![("POST".to_string(), "login".to_string())],
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().expect("temp dir");
        let record = sample_record(1);

        save(dir.path(), &record).expect("save should succeed");
        let loaded = load(dir.path(), 1).expect("load should succeed");
        assert_eq!(loaded, record);
    }

    #[test]
    fn save_writes_a_four_digit_zero_padded_file_name() {
        let dir = tempdir().expect("temp dir");
        save(dir.path(), &sample_record(7)).expect("save should succeed");
        assert!(dir.path().join(".postino/runs/0007.json").is_file());
    }

    #[test]
    fn next_number_is_one_for_an_empty_or_missing_history() {
        let dir = tempdir().expect("temp dir");
        assert_eq!(next_number(dir.path()), 1);
    }

    #[test]
    fn next_number_is_one_more_than_the_highest_existing_run() {
        let dir = tempdir().expect("temp dir");
        save(dir.path(), &sample_record(1)).expect("save should succeed");
        save(dir.path(), &sample_record(3)).expect("save should succeed");
        assert_eq!(next_number(dir.path()), 4);
    }

    #[test]
    fn list_is_newest_first() {
        let dir = tempdir().expect("temp dir");
        save(dir.path(), &sample_record(1)).expect("save should succeed");
        save(dir.path(), &sample_record(2)).expect("save should succeed");
        save(dir.path(), &sample_record(3)).expect("save should succeed");

        let headers = list(dir.path());
        let numbers: Vec<u32> = headers.iter().map(|header| header.number).collect();
        assert_eq!(numbers, vec![3, 2, 1]);
    }

    #[test]
    fn list_skips_a_corrupt_file_but_keeps_the_others() {
        let dir = tempdir().expect("temp dir");
        save(dir.path(), &sample_record(1)).expect("save should succeed");
        save(dir.path(), &sample_record(2)).expect("save should succeed");
        fs::write(dir.path().join(".postino/runs/0003.json"), "not json").expect("write junk");

        let headers = list(dir.path());
        let numbers: Vec<u32> = headers.iter().map(|header| header.number).collect();
        assert_eq!(numbers, vec![2, 1]);
    }

    #[test]
    fn next_number_still_counts_a_corrupt_file_by_its_name() {
        let dir = tempdir().expect("temp dir");
        save(dir.path(), &sample_record(1)).expect("save should succeed");
        fs::create_dir_all(dir.path().join(".postino/runs")).expect("create runs dir");
        fs::write(dir.path().join(".postino/runs/0002.json"), "not json").expect("write junk");

        assert_eq!(next_number(dir.path()), 3);
    }

    #[test]
    fn load_of_a_missing_run_is_an_error() {
        let dir = tempdir().expect("temp dir");
        assert!(matches!(load(dir.path(), 99), Err(HistoryError::Io { .. })));
    }

    #[test]
    fn list_of_a_workspace_with_no_runs_folder_is_empty() {
        let dir = tempdir().expect("temp dir");
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn round_trip_preserves_stopped_early_and_target_labels() {
        let dir = tempdir().expect("temp dir");
        let mut record = sample_record(1);
        record.stopped_early = true;
        record.target_labels = vec![
            ("POST".to_string(), "create".to_string()),
            ("GET".to_string(), "list".to_string()),
        ];

        save(dir.path(), &record).expect("save should succeed");
        let loaded = load(dir.path(), 1).expect("load should succeed");

        assert!(loaded.stopped_early);
        assert_eq!(
            loaded.target_labels,
            vec![
                ("POST".to_string(), "create".to_string()),
                ("GET".to_string(), "list".to_string()),
            ]
        );
    }

    #[test]
    fn a_run_file_saved_before_stopped_early_and_target_labels_existed_still_loads() {
        let dir = tempdir().expect("temp dir");
        // The exact shape `save` wrote before its newer fields existed: everything
        // `RunRecord` still has, nothing it gained since.
        let mut value = serde_json::to_value(sample_record(1)).expect("serialize");
        let object = value
            .as_object_mut()
            .expect("record serializes as an object");
        object.remove("stopped_early");
        object.remove("target_labels");
        let old_json = serde_json::to_string_pretty(&value).expect("serialize back to text");

        fs::create_dir_all(dir.path().join(".postino/runs")).expect("create runs dir");
        fs::write(dir.path().join(".postino/runs/0001.json"), old_json).expect("write old record");

        let loaded = load(dir.path(), 1).expect("an old record without these fields still loads");
        assert!(
            !loaded.stopped_early,
            "should default to false (\"Finished\")"
        );
        assert!(
            loaded.target_labels.is_empty(),
            "should default to empty (falls back to the generic \"Target N\" label)"
        );
    }
}
