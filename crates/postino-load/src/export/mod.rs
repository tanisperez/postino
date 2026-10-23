//! Exporting a load test run: [`report::Report`], the formatted content the app builds for the
//! Markdown and PDF reports, and the renderers, [`markdown::to_markdown`] and (once added)
//! [`pdf::to_pdf`].

pub mod markdown;
pub mod report;
