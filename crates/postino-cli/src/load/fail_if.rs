//! `--fail-if` conditions of `postino load`: `p95>500ms`, `errors>1%`, `rps<100`. A condition
//! that holds at the end of the run fails it (exit code 1).

use std::fmt;

use postino_load::{Threshold, ThresholdMetric};

use crate::outcome::format_duration;

/// What a condition reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Median latency, milliseconds.
    P50,
    /// 95th percentile latency, milliseconds.
    P95,
    /// 99th percentile latency, milliseconds.
    P99,
    /// Mean latency, milliseconds.
    Avg,
    /// Slowest request, milliseconds.
    Max,
    /// Error rate, a fraction.
    Errors,
    /// Requests per second.
    Rps,
}

impl Metric {
    fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "p50" => Metric::P50,
            "p95" => Metric::P95,
            "p99" => Metric::P99,
            "avg" => Metric::Avg,
            "max" => Metric::Max,
            "errors" => Metric::Errors,
            "rps" => Metric::Rps,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Metric::P50 => "p50",
            Metric::P95 => "p95",
            Metric::P99 => "p99",
            Metric::Avg => "avg",
            Metric::Max => "max",
            Metric::Errors => "errors",
            Metric::Rps => "rps",
        }
    }

    fn is_latency(self) -> bool {
        matches!(
            self,
            Metric::P50 | Metric::P95 | Metric::P99 | Metric::Avg | Metric::Max
        )
    }
}

/// The comparison of a condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
        }
    }
}

/// The numbers of a run that conditions are checked against.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Values {
    /// Median latency, milliseconds.
    pub p50_ms: f64,
    /// 95th percentile latency, milliseconds.
    pub p95_ms: f64,
    /// 99th percentile latency, milliseconds.
    pub p99_ms: f64,
    /// Mean latency, milliseconds.
    pub avg_ms: f64,
    /// Slowest request, milliseconds.
    pub max_ms: f64,
    /// Error rate, `0.0` to `1.0`.
    pub error_rate: f64,
    /// Requests per second.
    pub rps: f64,
}

impl Values {
    fn get(&self, metric: Metric) -> f64 {
        match metric {
            Metric::P50 => self.p50_ms,
            Metric::P95 => self.p95_ms,
            Metric::P99 => self.p99_ms,
            Metric::Avg => self.avg_ms,
            Metric::Max => self.max_ms,
            Metric::Errors => self.error_rate,
            Metric::Rps => self.rps,
        }
    }
}

/// A `--fail-if` condition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FailIf {
    /// What it reads.
    pub metric: Metric,
    /// How it compares.
    pub op: Op,
    /// The limit: milliseconds, a fraction for the errors, or requests per second.
    pub limit: f64,
}

impl FailIf {
    /// Parses `p95>500ms`, `avg>=1.5s`, `errors>1%` or `rps<100`. Latencies take `ms` (the
    /// default) or `s`; the errors take a percentage, with or without `%`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let at = compact
            .find(['<', '>'])
            .ok_or_else(|| format!("{text:?}: expected a condition such as p95>500ms"))?;
        let (name, rest) = compact.split_at(at);
        let (op, value) = if let Some(value) = rest.strip_prefix(">=") {
            (Op::Ge, value)
        } else if let Some(value) = rest.strip_prefix("<=") {
            (Op::Le, value)
        } else if let Some(value) = rest.strip_prefix('>') {
            (Op::Gt, value)
        } else {
            (Op::Lt, &rest[1..])
        };
        let metric = Metric::parse(name).ok_or_else(|| {
            format!("{text:?}: unknown metric {name:?}, use p50, p95, p99, avg, max, errors or rps")
        })?;
        let lower = value.to_ascii_lowercase();
        let (number, factor) = if metric.is_latency() {
            if let Some(number) = lower.strip_suffix("ms") {
                (number, 1.0)
            } else if let Some(number) = lower.strip_suffix('s') {
                (number, 1000.0)
            } else {
                (lower.as_str(), 1.0)
            }
        } else if metric == Metric::Errors {
            (lower.trim_end_matches('%'), 0.01)
        } else {
            (lower.as_str(), 1.0)
        };
        let limit = number
            .parse::<f64>()
            .ok()
            .filter(|limit| limit.is_finite() && *limit >= 0.0)
            .ok_or_else(|| format!("{text:?}: {value:?} is not a valid limit"))?;
        Ok(FailIf {
            metric,
            op,
            limit: limit * factor,
        })
    }

    /// Whether the condition holds for `values`, failing the run.
    pub fn hit(&self, values: &Values) -> bool {
        let value = values.get(self.metric);
        match self.op {
            Op::Gt => value > self.limit,
            Op::Ge => value >= self.limit,
            Op::Lt => value < self.limit,
            Op::Le => value <= self.limit,
        }
    }

    /// The value of `values` this condition reads, formatted.
    pub fn format_value(&self, values: &Values) -> String {
        format_metric(self.metric, values.get(self.metric))
    }

    /// The same goal as the app's thresholds, saved with the run so the app shows it, when there
    /// is one: an upper bound of a percentile or the error rate, or a lower bound of the
    /// throughput.
    pub fn to_threshold(self) -> Option<Threshold> {
        let upper = matches!(self.op, Op::Gt | Op::Ge);
        let metric = match (self.metric, upper) {
            (Metric::P50, true) => ThresholdMetric::P50,
            (Metric::P95, true) => ThresholdMetric::P95,
            (Metric::P99, true) => ThresholdMetric::P99,
            (Metric::Errors, true) => ThresholdMetric::ErrorRate,
            (Metric::Rps, false) => ThresholdMetric::Rps,
            _ => return None,
        };
        Some(Threshold {
            metric,
            limit: self.limit,
        })
    }
}

/// A metric's value as the summary shows it: `112 ms`, `0.4 %`, `572.3`.
pub fn format_metric(metric: Metric, value: f64) -> String {
    if metric.is_latency() {
        format_ms(value)
    } else if metric == Metric::Errors {
        format!("{:.1} %", value * 100.0)
    } else {
        format!("{value:.1}")
    }
}

/// Milliseconds as `38 ms`, `1.2 s` or `<1 ms`.
pub fn format_ms(ms: f64) -> String {
    if ms <= 0.0 {
        return "0 ms".to_string();
    }
    format_duration(std::time::Duration::from_secs_f64(ms / 1000.0))
}

impl fmt::Display for FailIf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.metric.name(),
            self.op.symbol(),
            format_metric(self.metric, self.limit)
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn conditions_parse_with_their_units() {
        let p95 = FailIf::parse("p95>500ms").expect("p95");
        assert_eq!(
            (p95.metric, p95.op, p95.limit),
            (Metric::P95, Op::Gt, 500.0)
        );
        assert_eq!(FailIf::parse("avg >= 1.5s").expect("avg").limit, 1500.0);
        assert_eq!(FailIf::parse("max>200").expect("max").limit, 200.0);
        let errors = FailIf::parse("errors>1%").expect("errors");
        assert!((errors.limit - 0.01).abs() < 1e-12);
        let rps = FailIf::parse("rps<100").expect("rps");
        assert_eq!((rps.op, rps.limit), (Op::Lt, 100.0));
    }

    #[test]
    fn bad_conditions_say_why() {
        assert!(FailIf::parse("p95").is_err());
        assert!(
            FailIf::parse("p90>5ms")
                .expect_err("metric")
                .contains("unknown metric")
        );
        assert!(FailIf::parse("p95>fast").is_err());
        assert!(FailIf::parse("p95>-1").is_err());
    }

    #[test]
    fn a_condition_that_holds_fails_the_run() {
        let values = Values {
            p95_ms: 600.0,
            error_rate: 0.004,
            rps: 120.0,
            ..Values::default()
        };
        assert!(FailIf::parse("p95>500ms").expect("p95").hit(&values));
        assert!(!FailIf::parse("errors>1%").expect("errors").hit(&values));
        assert!(!FailIf::parse("rps<100").expect("rps").hit(&values));
        assert!(FailIf::parse("rps<=120").expect("rps").hit(&values));
    }

    #[test]
    fn app_goals_exist_for_the_usual_directions() {
        let goal = FailIf::parse("p95>500ms").expect("p95").to_threshold();
        assert_eq!(
            goal,
            Some(Threshold {
                metric: ThresholdMetric::P95,
                limit: 500.0
            })
        );
        assert!(
            FailIf::parse("rps<100")
                .expect("rps")
                .to_threshold()
                .is_some()
        );
        assert_eq!(FailIf::parse("avg>5ms").expect("avg").to_threshold(), None);
        assert_eq!(FailIf::parse("p95<5ms").expect("p95").to_threshold(), None);
    }

    #[test]
    fn conditions_print_back_readably() {
        assert_eq!(
            FailIf::parse("p95>500ms").expect("p95").to_string(),
            "p95 > 500 ms"
        );
        assert_eq!(
            FailIf::parse("errors>1%").expect("errors").to_string(),
            "errors > 1.0 %"
        );
    }
}
