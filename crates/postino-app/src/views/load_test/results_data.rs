//! Keeping a load test tab's derived results up to date: the statistics of the selected window
//! (`postino_load::window_stats`) and the Requests tab's filtered, sorted rows
//! (`state::load_results::filter_requests`) are computed on the background executor, never in
//! `render`, whenever their key (`state::load_test::AnalysisKey`, `RequestsKey`) no longer
//! matches the tab. Also loads a past run's saved requests.

use std::collections::HashMap;

use gpui_kit::*;

use crate::state::load_results::{ResultsTab, Shared, filter_requests};
use crate::state::load_test::{AnalysisKey, RequestsKey};
use crate::views::root::AppView;

/// The background computations in flight for one tab, with the key each was started for, so a
/// refresh never starts the same computation twice. Dropping a task cancels it.
#[derive(Default)]
pub(crate) struct LoadResultTasks {
    analysis: Option<(AnalysisKey, Task<()>)>,
    requests: Option<(RequestsKey, Task<()>)>,
    samples: Option<Task<()>>,
}

/// Every tab's [`LoadResultTasks`], by tab id.
pub(crate) type LoadResultTaskMap = HashMap<String, LoadResultTasks>;

impl AppView {
    /// Starts whatever background computation `tab_id`'s results are missing: the window
    /// statistics, and the Requests rows while the Requests tab is shown. A no-op when
    /// everything is up to date or already being computed.
    pub(crate) fn refresh_load_results(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(load_test) = self
            .state
            .tabs
            .index_of(tab_id)
            .and_then(|index| self.state.tabs.get(index))
            .and_then(|tab| tab.load_test())
        else {
            return;
        };
        let Some(log) = load_test.log.clone() else {
            return;
        };
        let tasks = self
            .load_result_tasks
            .entry(tab_id.to_string())
            .or_default();

        if let Some(key) = load_test.analysis_key()
            && load_test.analysis.as_ref().map(|(done, _)| done) != Some(&key)
            && tasks.analysis.as_ref().map(|(pending, _)| pending) != Some(&key)
        {
            let target_count = load_test.target_rows.len().max(
                log.samples()
                    .iter()
                    .map(|sample| sample.target as usize + 1)
                    .max()
                    .unwrap_or(0),
            );
            let elapsed_secs = load_test
                .snapshot
                .as_ref()
                .map_or(0.0, |snapshot| snapshot.elapsed.as_secs_f64());
            let run_secs = (load_test.shown_duration_secs() as f64)
                .max(elapsed_secs.floor())
                .max(1.0) as u32;
            let range = load_test.window_range();
            let log = log.clone();
            let tab_id = tab_id.to_string();
            let done_key = key.clone();
            let task = cx.spawn(async move |weak, cx| {
                let stats = cx
                    .background_spawn(async move {
                        postino_load::window_stats(
                            &log,
                            target_count,
                            run_secs,
                            range,
                            elapsed_secs,
                        )
                    })
                    .await;
                let _ = weak.update(cx, |view, cx| {
                    if let Some(tasks) = view.load_result_tasks.get_mut(&tab_id) {
                        tasks.analysis = None;
                    }
                    view.edit_load_test(&tab_id, cx, |load_test| {
                        load_test.analysis = Some((done_key, Shared::new(stats)));
                    });
                    view.refresh_load_results(&tab_id, cx);
                });
            });
            tasks.analysis = Some((key, task));
        }

        if load_test.results_tab == ResultsTab::Requests
            && let Some(key) = load_test.requests_key()
            && load_test.requests.as_ref().map(|(done, _)| done) != Some(&key)
            && tasks.requests.as_ref().map(|(pending, _)| pending) != Some(&key)
        {
            let names: Vec<String> = load_test
                .target_rows
                .iter()
                .map(|(_, name)| name.clone())
                .collect();
            let range = load_test.window_range();
            let filter = key.filter.clone();
            let sort = key.sort;
            let tab_id = tab_id.to_string();
            let done_key = key.clone();
            let task = cx.spawn(async move |weak, cx| {
                let rows = cx
                    .background_spawn(
                        async move { filter_requests(&log, range, &filter, sort, &names) },
                    )
                    .await;
                let _ = weak.update(cx, |view, cx| {
                    if let Some(tasks) = view.load_result_tasks.get_mut(&tab_id) {
                        tasks.requests = None;
                    }
                    view.edit_load_test(&tab_id, cx, |load_test| {
                        load_test.requests = Some((done_key, Shared::new(rows)));
                    });
                    view.refresh_load_results(&tab_id, cx);
                });
            });
            tasks.requests = Some((key, task));
        }
    }

    /// Loads the saved requests of past run `number` in the background (decompressing a large
    /// run takes a moment) and shows them on `tab_id` if it still shows that run. A run saved
    /// before requests were saved marks the tab's log as unavailable.
    pub(crate) fn load_load_test_samples(
        &mut self,
        tab_id: &str,
        number: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self
            .state
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf())
        else {
            return;
        };
        let tab_id_owned = tab_id.to_string();
        let task = cx.spawn(async move |weak, cx| {
            let loaded = cx
                .background_spawn(async move { postino_load::history::load_samples(&root, number) })
                .await;
            let _ = weak.update(cx, |view, cx| {
                if let Some(tasks) = view.load_result_tasks.get_mut(&tab_id_owned) {
                    tasks.samples = None;
                }
                let log = match loaded {
                    Ok(log) => log,
                    Err(error) => {
                        log::warn!(
                            "could not load the requests of load test run {number}: {error}"
                        );
                        None
                    }
                };
                view.edit_load_test(&tab_id_owned, cx, |load_test| {
                    if load_test.run_number != Some(number) || load_test.is_running() {
                        return;
                    }
                    load_test.log_unavailable = log.is_none();
                    load_test.replace_log(log);
                });
                view.refresh_load_results(&tab_id_owned, cx);
            });
        });
        self.load_result_tasks
            .entry(tab_id.to_string())
            .or_default()
            .samples = Some(task);
    }
}
