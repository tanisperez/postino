//! What the dashboard shows beyond a [`LoadSnapshot`]: the mean and the slowest latency, and the
//! errors of every second. Read from the run's sample log, only the part that is new since the
//! last tick, so a tick costs as much as the requests it brings.

use postino_load::{LoadRun, LoadSnapshot, SampleLog};

use super::fail_if::Values;

/// The running numbers of a load test.
#[derive(Debug, Default)]
pub struct Live {
    /// The copy of the run's log, filled by [`LoadRun::read_samples`].
    log: SampleLog,
    /// How many samples of `log` are already counted.
    counted: usize,
    /// Requests and errors of each second, by second.
    seconds: Vec<(u64, u64)>,
    sum_us: u128,
    max_us: u32,
}

impl Live {
    /// Reads what `run` recorded since the last call.
    pub fn update(&mut self, run: &LoadRun) {
        run.read_samples(&mut self.log);
        self.count_new();
    }

    /// Takes the complete log of a finished run, counting what the ticks had not seen.
    pub fn finish(&mut self, log: SampleLog) {
        self.log = log;
        self.count_new();
    }

    fn count_new(&mut self) {
        for sample in &self.log.samples()[self.counted.min(self.log.len())..] {
            let second = (sample.elapsed_ms / 1000) as usize;
            if self.seconds.len() <= second {
                self.seconds.resize(second + 1, (0, 0));
            }
            self.seconds[second].0 += 1;
            self.seconds[second].1 += u64::from(sample.is_error);
            self.sum_us += u128::from(sample.latency_us);
            self.max_us = self.max_us.max(sample.latency_us);
        }
        self.counted = self.log.len();
    }

    /// The samples read so far.
    pub fn log(&self) -> &SampleLog {
        &self.log
    }

    /// The mean latency, in milliseconds.
    pub fn avg_ms(&self) -> f64 {
        if self.counted == 0 {
            return 0.0;
        }
        // Display precision only.
        #[allow(clippy::cast_precision_loss)]
        let avg = self.sum_us as f64 / self.counted as f64;
        avg / 1000.0
    }

    /// The slowest request, in milliseconds.
    pub fn max_ms(&self) -> f64 {
        f64::from(self.max_us) / 1000.0
    }

    /// The error rate of each second, in percent, for a sparkline.
    pub fn error_percent_series(&self) -> Vec<u64> {
        self.seconds
            .iter()
            .map(|&(count, errors)| (errors * 100).checked_div(count).unwrap_or(0))
            .collect()
    }

    /// The numbers `--fail-if` reads, from `snapshot` and the log.
    pub fn values(&self, snapshot: &LoadSnapshot) -> Values {
        Values {
            p50_ms: f64::from(snapshot.p50) / 1000.0,
            p95_ms: f64::from(snapshot.p95) / 1000.0,
            p99_ms: f64::from(snapshot.p99) / 1000.0,
            avg_ms: self.avg_ms(),
            max_ms: self.max_ms(),
            error_rate: snapshot.error_rate,
            rps: snapshot.rps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postino_load::{SampleRecord, StatusKey};
    use pretty_assertions::assert_eq;

    fn sample(elapsed_ms: u32, latency_us: u32, is_error: bool) -> SampleRecord {
        SampleRecord {
            elapsed_ms,
            vu: 0,
            target: 0,
            url: 0,
            status: StatusKey::Code(if is_error { 500 } else { 200 }),
            latency_us,
            size: 0,
            is_error,
        }
    }

    #[test]
    fn new_samples_are_counted_once() {
        let mut log = SampleLog::new();
        log.push(sample(100, 2_000, false));
        log.push(sample(900, 4_000, true));
        let mut live = Live::default();
        live.finish(log.clone());
        log.push(sample(1_500, 6_000, false));
        live.finish(log);
        assert_eq!(live.avg_ms(), 4.0);
        assert_eq!(live.max_ms(), 6.0);
        assert_eq!(live.error_percent_series(), [50, 0]);
    }
}
