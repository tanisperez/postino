//! [`StatusBadge`]: the response status pill shown in the response pane's tab bar.
//!
//! Custom element for the dot-plus-text pill (no gpui-kit widget matches its shape); the
//! `Sending` state wraps gpui-kit's own [`Spinner`] in place of the dot.

use gpui_kit::component::spinner::Spinner;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::theme::PaletteExt;
use crate::theme::metrics::{RADIUS_SM, STATUS_BADGE_HEIGHT};

/// What a [`StatusBadge`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusState {
    /// A response was received with this HTTP status code.
    Code(u16),
    /// No request has been sent yet, or the last send failed before a status was known.
    NotSent,
    /// A request is in flight.
    Sending,
}

/// The response status badge: a colored dot (or spinner, while [`StatusState::Sending`]) plus a
/// text label, on a `*-subtle` background, matching the status
/// color mapping in `docs/design-system.md` ([`crate::theme::Palette::status_colors`]).
#[derive(IntoElement)]
pub struct StatusBadge {
    state: StatusState,
    label: Option<SharedString>,
}

impl StatusBadge {
    /// A badge for `state`, with the default label (the code itself, `"Not sent"` or
    /// `"Sending..."`). Override with [`Self::label`] to show a status's reason phrase.
    pub fn new(state: StatusState) -> Self {
        Self { state, label: None }
    }

    /// Overrides the default label, for example `"200 OK"` instead of a bare `"200"`.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn default_label(&self) -> SharedString {
        match &self.state {
            StatusState::Code(code) => code.to_string().into(),
            StatusState::NotSent => t!("response.status.not_sent").into_owned().into(),
            StatusState::Sending => t!("common.sending").into_owned().into(),
        }
    }
}

impl RenderOnce for StatusBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.palette();
        let status = match &self.state {
            StatusState::Code(code) => Some(*code),
            StatusState::NotSent | StatusState::Sending => None,
        };
        let (fg, bg) = palette.status_colors(status);
        let sending = matches!(self.state, StatusState::Sending);
        let label = self.label.clone().unwrap_or_else(|| self.default_label());

        let marker: AnyElement = if sending {
            Spinner::new().color(fg).into_any_element()
        } else {
            div()
                .flex_none()
                .size(px(6.0))
                .rounded(px(3.0))
                .bg(fg)
                .into_any_element()
        };

        div()
            .flex()
            .items_center()
            .gap(px(5.0))
            .h(px(STATUS_BADGE_HEIGHT))
            .px_2()
            .rounded(px(RADIUS_SM))
            .bg(bg)
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(fg)
            .child(marker)
            .child(label)
    }
}
