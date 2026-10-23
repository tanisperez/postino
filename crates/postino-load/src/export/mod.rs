//! Exporting a load test run: [`report::Report`], the formatted content the app builds for the
//! Markdown and PDF reports, [`csv::requests_csv`], one row per request, and [`json::run_json`],
//! the whole run as JSON.
//!
//! The CSV and the JSON are data formats, not documents for a reader: keys and headers are
//! English, numbers are plain (a dot as decimal separator, no grouping) and latencies are in
//! milliseconds.

pub mod csv;
pub mod json;
pub mod report;
