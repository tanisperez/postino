//! Plain helpers behind the load test results (`views/load_test/results*.rs`): the chart's axis
//! scales and labels, time labels, and the keys of the chart series. No `gpui`, no IO.

use super::number::{format_decimal, format_integer};

/// How many intervals a chart y axis is divided into: five labels from 0 to the top.
pub const AXIS_DIVISIONS: usize = 4;

/// How many intervals the chart's x axis is divided into: seven labels from 0 to the duration.
pub const X_DIVISIONS: usize = 6;

/// One y axis of the results chart: its top value and its labels from the bottom (0) up.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisScale {
    /// The top of the axis, a "nice" number at or above every value plotted on it.
    pub max: f64,
    /// One label per grid line, bottom to top, [`AXIS_DIVISIONS`] + 1 of them.
    pub labels: Vec<String>,
}

/// The smallest "nice" step (1, 2, 2.5 or 5 times a power of ten) such that
/// [`AXIS_DIVISIONS`] steps reach `value`. `1.0` for zero, negative or non-finite values.
#[must_use]
pub fn nice_step(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return 1.0;
    }
    let raw = value / AXIS_DIVISIONS as f64;
    let magnitude = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|step| *step >= raw * (1.0 - 1e-9))
        .unwrap_or(10.0 * magnitude)
}

/// The requests per second axis: plain integers, the top one followed by `"/s"`, like the
/// design's `0, 600, 1.200, 1.800, 2.400/s`.
#[must_use]
pub fn rps_axis(max_value: f64) -> AxisScale {
    let step = nice_step(max_value);
    let labels = (0..=AXIS_DIVISIONS)
        .map(|index| {
            let value = step * index as f64;
            let text = format_axis_number(value);
            if index == AXIS_DIVISIONS {
                format!("{text}/s")
            } else {
                text
            }
        })
        .collect();
    AxisScale {
        max: step * AXIS_DIVISIONS as f64,
        labels,
    }
}

/// The milliseconds axis: the top label followed by `" ms"`, like the design's
/// `0, 25, 50, 75, 100 ms`.
#[must_use]
pub fn ms_axis(max_value: f64) -> AxisScale {
    let step = nice_step(max_value);
    let labels = (0..=AXIS_DIVISIONS)
        .map(|index| {
            let value = step * index as f64;
            let text = format_axis_number(value);
            if index == AXIS_DIVISIONS {
                format!("{text} ms")
            } else {
                text
            }
        })
        .collect();
    AxisScale {
        max: step * AXIS_DIVISIONS as f64,
        labels,
    }
}

/// `value` with as many decimals as it needs (none for `5`, one for `7.5`, two for `0.25`), in
/// the current language.
fn format_axis_number(value: f64) -> String {
    let decimals = (0..3)
        .find(|decimals| {
            let scaled = value * 10f64.powi(*decimals);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(3) as usize;
    format_decimal(value, decimals)
}

/// The x axis labels of a run of `duration_secs` seconds: [`X_DIVISIONS`] + 1 labels from `0s`
/// to the duration, whole seconds.
#[must_use]
pub fn x_labels(duration_secs: u64) -> Vec<String> {
    (0..=X_DIVISIONS as u64)
        .map(|index| {
            let secs = duration_secs * index / X_DIVISIONS as u64;
            format!("{}s", format_integer(secs))
        })
        .collect()
}

/// `"mm:ss"` for `secs` seconds; minutes keep counting past an hour (`"75:00"`).
#[must_use]
pub fn mmss(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// `"mm:ss.mmm"` for `millis` milliseconds, the time column of the Requests tab.
#[must_use]
pub fn mmss_millis(millis: u64) -> String {
    format!("{}.{:03}", mmss(millis / 1000), millis % 1000)
}

/// The chart tooltip's time line for the point at `second`: `"00:34 \u{2013} 00:35"`.
#[must_use]
pub fn second_range_label(second: usize) -> String {
    let second = second as u64;
    format!("{} \u{2013} {}", mmss(second), mmss(second + 1))
}

/// A selected window of seconds `first..=last` as `"34\u{2013}46 s"`: from the start of its
/// first second to the end of its last.
#[must_use]
pub fn window_label(first: usize, last: usize) -> String {
    format!(
        "{}\u{2013}{} s",
        format_integer(first as u64),
        format_integer(last as u64 + 1)
    )
}

/// A latency in milliseconds for a table cell or a tooltip: one decimal below 10 ms, whole
/// milliseconds up to a second, seconds with one decimal above (`"4.2 ms"`, `"38 ms"`,
/// `"5.0 s"`).
#[must_use]
pub fn format_latency_ms(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{} s", format_decimal(ms / 1000.0, 1))
    } else if ms < 10.0 {
        format!("{} ms", format_decimal(ms, 1))
    } else {
        format!("{} ms", format_decimal(ms, 0))
    }
}

/// A response size for the Requests tab: bytes below 1,000, kilobytes with one decimal up to a
/// megabyte, megabytes above (`"312 B"`, `"4.2 KB"`, `"1.5 MB"`).
#[must_use]
pub fn format_size(bytes: u64) -> String {
    if bytes < 1000 {
        format!("{} B", format_integer(bytes))
    } else if bytes < 1_000_000 {
        format!("{} KB", format_decimal(bytes as f64 / 1024.0, 1))
    } else {
        format!("{} MB", format_decimal(bytes as f64 / (1024.0 * 1024.0), 1))
    }
}

/// One series of the results chart, the key its legend toggle hides it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartSeriesKey {
    /// Active virtual users, an area on the left axis scaled to 90 % of its height.
    Vus,
    /// Requests per second, on the left axis.
    Rps,
    /// Median latency, on the right axis.
    P50,
    /// 90th percentile latency, on the right axis.
    P90,
    /// 99th percentile latency, on the right axis.
    P99,
    /// The compared run's 99th percentile, dashed, on the right axis.
    Baseline,
}

impl ChartSeriesKey {
    /// The series in drawing order, back to front.
    pub const ALL: [ChartSeriesKey; 6] = [
        ChartSeriesKey::Vus,
        ChartSeriesKey::Baseline,
        ChartSeriesKey::P99,
        ChartSeriesKey::P90,
        ChartSeriesKey::P50,
        ChartSeriesKey::Rps,
    ];

    /// The series in legend order.
    pub const LEGEND: [ChartSeriesKey; 6] = [
        ChartSeriesKey::Vus,
        ChartSeriesKey::Rps,
        ChartSeriesKey::P50,
        ChartSeriesKey::P90,
        ChartSeriesKey::P99,
        ChartSeriesKey::Baseline,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn nice_steps_cover_the_value_in_four_intervals() {
        assert!((nice_step(2_250.0) - 600.0).abs() > 0.0);
        assert!((nice_step(2_250.0) - 1_000.0).abs() < 1e-9);
        assert!((nice_step(2_000.0) - 500.0).abs() < 1e-9);
        assert!((nice_step(95.0) - 25.0).abs() < 1e-9);
        assert!((nice_step(100.0) - 25.0).abs() < 1e-9);
        assert!((nice_step(3.2) - 1.0).abs() < 1e-9);
        assert!((nice_step(0.0) - 1.0).abs() < 1e-9);
        assert!((nice_step(f64::NAN) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn axes_label_every_grid_line_with_the_unit_on_top() {
        let rps = rps_axis(1_900.0);
        assert!((rps.max - 2_000.0).abs() < 1e-9);
        assert_eq!(rps.labels, vec!["0", "500", "1,000", "1,500", "2,000/s"]);
        let ms = ms_axis(9.0);
        assert!((ms.max - 10.0).abs() < 1e-9);
        assert_eq!(ms.labels, vec!["0", "2.5", "5", "7.5", "10 ms"]);
    }

    #[test]
    fn x_labels_split_the_run_in_six() {
        assert_eq!(
            x_labels(60),
            vec!["0s", "10s", "20s", "30s", "40s", "50s", "60s"]
        );
    }

    #[test]
    fn time_labels() {
        assert_eq!(mmss(75), "01:15");
        assert_eq!(mmss_millis(34_512), "00:34.512");
        assert_eq!(second_range_label(34), "00:34 \u{2013} 00:35");
        assert_eq!(window_label(34, 45), "34\u{2013}46 s");
    }

    #[test]
    fn latencies_and_sizes() {
        assert_eq!(format_latency_ms(4.24), "4.2 ms");
        assert_eq!(format_latency_ms(38.4), "38 ms");
        assert_eq!(format_latency_ms(5_000.0), "5.0 s");
        assert_eq!(format_size(312), "312 B");
        assert_eq!(format_size(4_300), "4.2 KB");
    }
}
