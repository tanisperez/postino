//! The inner tabs of the request editor and response viewer panels (`plans/mvp.md`, Phase 9).
//!
//! These are plain, `Copy` enums so the currently selected tab can be stored directly on
//! [`crate::views::AppView`] without any `gpui` type.

/// The request editor's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestTab {
    /// The `::: query` section, as an editable key-value table.
    #[default]
    Params,
    /// The header list, as an editable key-value table.
    Headers,
    /// The body: type selector plus a code editor (or a key-value table for a Form body).
    Body,
    /// The `::: pre` section, a JavaScript code editor.
    Pre,
    /// The `::: post` section, a JavaScript code editor.
    Post,
    /// The `::: docs` section, free text notes.
    Docs,
}

impl RequestTab {
    /// Every tab, in the order the tab bar lists them.
    pub const ALL: [RequestTab; 6] = [
        RequestTab::Params,
        RequestTab::Headers,
        RequestTab::Body,
        RequestTab::Pre,
        RequestTab::Post,
        RequestTab::Docs,
    ];

    /// The label shown on the tab.
    pub fn label(self) -> &'static str {
        match self {
            RequestTab::Params => "Params",
            RequestTab::Headers => "Headers",
            RequestTab::Body => "Body",
            RequestTab::Pre => "Pre-request",
            RequestTab::Post => "Post-response",
            RequestTab::Docs => "Docs",
        }
    }
}

/// The response viewer's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponseTab {
    /// The response body, pretty-printed when it is JSON, with a raw toggle.
    #[default]
    Body,
    /// The response headers.
    Headers,
    /// The post-response script's `test()` results.
    Tests,
    /// The pre/post scripts' `console.*` output.
    Console,
}

impl ResponseTab {
    /// Every tab, in the order the tab bar lists them.
    pub const ALL: [ResponseTab; 4] = [
        ResponseTab::Body,
        ResponseTab::Headers,
        ResponseTab::Tests,
        ResponseTab::Console,
    ];

    /// The label shown on the tab.
    pub fn label(self) -> &'static str {
        match self {
            ResponseTab::Body => "Body",
            ResponseTab::Headers => "Headers",
            ResponseTab::Tests => "Tests",
            ResponseTab::Console => "Console",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_tab_is_params() {
        assert_eq!(RequestTab::default(), RequestTab::Params);
    }

    #[test]
    fn default_response_tab_is_body() {
        assert_eq!(ResponseTab::default(), ResponseTab::Body);
    }

    #[test]
    fn every_request_tab_has_a_label() {
        for tab in RequestTab::ALL {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn every_response_tab_has_a_label() {
        for tab in ResponseTab::ALL {
            assert!(!tab.label().is_empty());
        }
    }
}
