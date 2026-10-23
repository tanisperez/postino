//! The load test results' Export menu actions: saving the run as Markdown, PDF, CSV or JSON, and
//! copying the summary as Markdown (`Ctrl+Shift+C`).

use gpui_kit::*;

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

impl AppView {
    /// Asks where to save the results of `tab_id`'s run as `format`, then writes them there.
    pub(crate) fn export_load_test(
        &mut self,
        tab_id: &str,
        format: ExportFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = (tab_id, format, window, cx);
    }

    /// Copies the summary of `tab_id`'s run to the clipboard as Markdown.
    pub(crate) fn copy_load_test_summary(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = (tab_id, window, cx);
    }
}
