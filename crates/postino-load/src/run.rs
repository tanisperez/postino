//! [`LoadRun`], a load test in progress: one OS thread per virtual user, a shared metrics
//! aggregator, and the controls the UI needs (`plans/ui-redesign.md`, Phase 1d, point 2). See
//! the crate docs for the concurrency model.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use postino_core::Environment;
use postino_http::HttpError;
use postino_runner::{FailedStage, RunResult, Runner, SessionEnv};

use crate::config::LoadConfig;
use crate::metrics::{Aggregator, LoadSnapshot, Sample};
use crate::status::StatusKey;

/// How often a waiting thread (the supervisor between virtual users, or a virtual user during
/// `think_time`) checks whether it should stop. Short enough that [`LoadRun::stop`] and the
/// `stop_on_error_rate` threshold both take effect well within a second, without busy-looping.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The final result of a load test run, read once with [`LoadRun::join`].
#[derive(Debug, Clone, PartialEq)]
pub struct LoadSummary {
    /// The metrics at the moment the run ended.
    pub snapshot: LoadSnapshot,
    /// Whether the run ended before [`LoadConfig::duration`] fully elapsed: because
    /// [`LoadRun::stop`] was called, or because [`LoadConfig::stop_on_error_rate`] was exceeded.
    /// `false` means the run completed its full configured duration.
    pub stopped_early: bool,
}

/// State shared between every virtual user thread, the supervisor thread and the [`LoadRun`]
/// handle held by the caller.
struct Shared {
    /// Set to stop every virtual user thread and the supervisor as soon as possible: by
    /// [`LoadRun::stop`], by the supervisor once [`LoadConfig::duration`] elapses, or by a
    /// virtual user once [`LoadConfig::stop_on_error_rate`] is exceeded.
    stop: AtomicBool,
    /// How many virtual user threads are currently running.
    active_vus: AtomicU32,
    /// The metrics every virtual user feeds into. See the crate docs for why this is a `Mutex`
    /// rather than something lock-free.
    metrics: Mutex<Aggregator>,
    /// The moment the run started, the origin every [`Sample::elapsed`] is measured from.
    start: Instant,
}

/// A load test in progress, or one that has already finished. Created with [`LoadRun::start`].
pub struct LoadRun {
    shared: Arc<Shared>,
    /// The thread that starts every virtual user and waits for the run to end. `None` only
    /// after [`LoadRun::join`] has consumed it.
    supervisor: Option<JoinHandle<LoadSummary>>,
}

impl LoadRun {
    /// Starts a load test in the background and returns immediately.
    ///
    /// One OS thread per virtual user (`ureq`, used by [`postino_http::send`], is blocking), each
    /// started once its turn in the linear ramp-up over `config.ramp_up` comes up. Each virtual
    /// user clones `session_env` once and loops over `config.targets` in order for as long as the
    /// run lasts, so an environment change a post script makes (for example a login token) is
    /// visible to the next target of the same virtual user, sleeping `config.think_time` between
    /// iterations.
    #[must_use]
    pub fn start(
        config: LoadConfig,
        environment: Environment,
        session_env: SessionEnv,
        runner: Arc<Runner>,
    ) -> Self {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            active_vus: AtomicU32::new(0),
            metrics: Mutex::new(Aggregator::new(config.targets.len())),
            start: Instant::now(),
        });

        let supervisor_shared = Arc::clone(&shared);
        let supervisor = thread::spawn(move || {
            run_supervisor(
                &supervisor_shared,
                &config,
                &environment,
                session_env,
                &runner,
            )
        });

        Self {
            shared,
            supervisor: Some(supervisor),
        }
    }

    /// Asks the run to stop as soon as possible.
    ///
    /// Every virtual user thread checks this at least once every [`POLL_INTERVAL`] (between
    /// targets, and while sleeping through `think_time`), so a call to `stop` finishes the run
    /// promptly, well under a second, even with a long `think_time` or `duration` left to go.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }

    /// Whether the run has ended (reached `duration`, was stopped, or hit the error rate
    /// threshold) and every virtual user thread has already been joined by the supervisor.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        match &self.supervisor {
            Some(handle) => handle.is_finished(),
            None => true,
        }
    }

    /// A live read of the current metrics. Cheap enough to call every 250 ms from the UI; see
    /// the crate docs for the cost of computing percentiles.
    #[must_use]
    pub fn snapshot(&self) -> LoadSnapshot {
        let elapsed = self.shared.start.elapsed();
        let active_vus = self.shared.active_vus.load(Ordering::SeqCst);
        lock_metrics(&self.shared.metrics).snapshot(elapsed, active_vus)
    }

    /// Blocks until the run has finished and returns its final [`LoadSummary`].
    ///
    /// # Panics
    ///
    /// Panics if the supervisor thread itself panicked, which is not expected to happen: it
    /// contains no `unwrap`/`expect` of its own, and every lock it takes recovers from poisoning
    /// instead of propagating a panic (see the crate docs).
    #[allow(clippy::expect_used)]
    pub fn join(mut self) -> LoadSummary {
        let supervisor = self
            .supervisor
            .take()
            .expect("join takes self by value, so it can only ever be called once");
        supervisor
            .join()
            .expect("the supervisor thread never panics, see the crate docs")
    }
}

/// Runs on its own thread for the lifetime of a [`LoadRun`]: starts every virtual user thread
/// over the ramp-up window, waits for the run to end, joins every virtual user, and returns the
/// final [`LoadSummary`].
fn run_supervisor(
    shared: &Arc<Shared>,
    config: &LoadConfig,
    environment: &Environment,
    session_env: SessionEnv,
    runner: &Arc<Runner>,
) -> LoadSummary {
    let mut handles = Vec::with_capacity(config.vus as usize);

    for vu_index in 0..config.vus {
        // Linear ramp-up: the Nth virtual user (0-based) starts this far into `ramp_up`.
        let start_offset = config
            .ramp_up
            .mul_f64(f64::from(vu_index) / f64::from(config.vus));
        wait_or_stop(shared, shared.start + start_offset);
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }

        shared.active_vus.fetch_add(1, Ordering::SeqCst);
        let vu_shared = Arc::clone(shared);
        let vu_config = config.clone();
        let vu_environment = environment.clone();
        let vu_runner = Arc::clone(runner);
        let mut vu_session_env = session_env.clone();

        handles.push(thread::spawn(move || {
            run_virtual_user(
                &vu_shared,
                &vu_config,
                &vu_environment,
                &mut vu_session_env,
                &vu_runner,
            );
            vu_shared.active_vus.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    // Let the run continue until `duration` has elapsed since the whole run started (not since
    // the last virtual user started), or until something else asked it to stop first.
    wait_or_stop(shared, shared.start + config.duration);
    let stopped_early = shared.start.elapsed() < config.duration;
    shared.stop.store(true, Ordering::SeqCst);

    for handle in handles {
        // `run_virtual_user` never panics (see its own docs), so there is nothing actionable to
        // do with an `Err` here beyond letting the thread finish.
        let _ = handle.join();
    }

    let snapshot = lock_metrics(&shared.metrics).snapshot(shared.start.elapsed(), 0);
    LoadSummary {
        snapshot,
        stopped_early,
    }
}

/// Runs on its own thread for the lifetime of one virtual user: loops over `config.targets` in
/// order, sleeping `config.think_time` between iterations, until `shared.stop` is set.
///
/// Never panics: the only fallible step is locking `shared.metrics`, and that recovers from a
/// poisoned lock instead of unwrapping (see [`lock_metrics`]), so a virtual user thread can
/// always be joined by the supervisor.
fn run_virtual_user(
    shared: &Shared,
    config: &LoadConfig,
    environment: &Environment,
    session_env: &mut SessionEnv,
    runner: &Runner,
) {
    'iterations: while !shared.stop.load(Ordering::SeqCst) {
        for (target_index, target) in config.targets.iter().enumerate() {
            if shared.stop.load(Ordering::SeqCst) {
                break 'iterations;
            }

            let started = Instant::now();
            let result = runner.run(&target.request, environment, session_env);
            let latency = response_latency(&result, started.elapsed());
            let elapsed = shared.start.elapsed();

            let sample = sample_from(target_index, elapsed, latency, &result);
            let (errors, total) = lock_metrics(&shared.metrics).record(sample);

            let exceeded_threshold = config
                .stop_on_error_rate
                .is_some_and(|threshold| total >= 50 && ratio(errors, total) > threshold);
            if exceeded_threshold {
                shared.stop.store(true, Ordering::SeqCst);
                break 'iterations;
            }
        }

        if !shared.stop.load(Ordering::SeqCst) && !config.think_time.is_zero() {
            wait_or_stop(shared, Instant::now() + config.think_time);
        }
    }
}

/// `numerator / denominator` as an `f64`, used here for a live error rate check against
/// [`LoadConfig::stop_on_error_rate`]. `denominator` is always positive when this is called (it
/// is only reached once `total >= 50`).
fn ratio(numerator: u64, denominator: u64) -> f64 {
    numerator as f64 / denominator as f64
}

/// The latency to record for one [`Runner::run`] call: the response's own measured time
/// ([`postino_core::Response::time`]) when a response was received, since that excludes the time
/// spent in pre/post scripts; `wall_time` (measured around the whole call) otherwise, since there
/// is no response to measure from.
fn response_latency(result: &RunResult, wall_time: Duration) -> Duration {
    result
        .response
        .as_ref()
        .map_or(wall_time, |response| response.time)
}

/// Turns the outcome of one [`Runner::run`] call into a [`Sample`], applying the error rule of
/// `plans/ui-redesign.md`, Phase 1d, point 3: no response at all (a send failure, a timeout, or a
/// pre-request script failure) or a status of 400 or above.
///
/// A post-response script failure is deliberately not treated as an error here: the response was
/// already received, so it still speaks for itself. This matches
/// [`postino_runner::RunResult::failed_stage`]'s own contract: `Post` failures still carry a
/// `response`.
fn sample_from(
    target_index: usize,
    elapsed: Duration,
    latency: Duration,
    result: &RunResult,
) -> Sample {
    let status = match (&result.failed_stage, &result.response) {
        (Some(FailedStage::Send(HttpError::Timeout)), _) => StatusKey::Timeout,
        (Some(FailedStage::Pre(_) | FailedStage::Send(_)), _) | (_, None) => StatusKey::Failed,
        (_, Some(response)) => StatusKey::Code(response.status),
    };

    Sample {
        target_index,
        elapsed,
        latency_us: micros_clamped(latency),
        status,
        is_error: status.is_error(),
    }
}

/// `duration` as whole microseconds, saturating at `u32::MAX` (about 71.5 minutes) instead of
/// overflowing. A single load test request is never expected to take anywhere near that long;
/// this is just a safe fallback instead of a panic.
fn micros_clamped(duration: Duration) -> u32 {
    u32::try_from(duration.as_micros()).unwrap_or(u32::MAX)
}

/// Sleeps in short slices until `deadline` or until `shared.stop` becomes `true`, whichever comes
/// first. Sleeping in slices of at most [`POLL_INTERVAL`], instead of one long `thread::sleep`,
/// is what makes [`LoadRun::stop`] take effect promptly instead of only once the full remaining
/// wait is over.
fn wait_or_stop(shared: &Shared, deadline: Instant) {
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

/// Locks `metrics`, recovering from a poisoned lock instead of panicking.
///
/// A `Mutex` is poisoned when a thread panics while holding it. Nothing in this crate panics
/// while holding `metrics` (every step between locking and unlocking is plain arithmetic and
/// vector operations, see [`Aggregator::record`] and [`Aggregator::snapshot`]), so poisoning here
/// would mean a bug elsewhere already corrupted the process. Recovering the guard is still the
/// right choice for a metrics aggregator read live by a UI: showing slightly stale numbers is far
/// better than making every virtual user thread panic too.
fn lock_metrics(metrics: &Mutex<Aggregator>) -> MutexGuard<'_, Aggregator> {
    metrics.lock().unwrap_or_else(PoisonError::into_inner)
}
