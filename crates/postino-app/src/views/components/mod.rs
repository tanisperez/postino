//! Reusable, styled components. The standard ones come from telaio (re-exported here, so views
//! keep importing them from `super::components`); Postino's own domain widgets live in this
//! module: one file per component, each a `RenderOnce` struct with builder methods, reading
//! colors only from [`crate::theme::Palette`] and sizes only from [`crate::theme::metrics`].
//!
//! The `POSTINO_OPEN=components` debug hook (`gallery`) renders every one of them in every state, to
//! check against `docs/design-system.md` by eye.

pub mod env_pill;
pub mod gallery;
pub mod method_badge;
pub mod status_badge;
pub mod url_bar;
pub mod variable_chip;
pub mod variable_line;

pub use env_pill::{EnvMenuItem, EnvPill};
pub use method_badge::MethodBadge;
pub use status_badge::{StatusState, status_badge};
pub use telaio_ui::components::{
    Card, CodeEditor, ConfirmDialog, DangerButton, DocumentTab, DocumentTabs, EmptyState,
    GhostButton, HoverInput, IconButton, InlineMessage, InlineMessageKind, KeyValueRow,
    KeyValueTable, MODAL_INNER_RADIUS, ModalPlacement, NeutralButtonExt, PrimaryButton,
    ScrollbarOverlay, SecondaryButton, SectionLabel, SegmentedControl, SegmentedItem, ThemeCard,
    TreeRow, UnderlineTabItem, UnderlineTabs, edit_menu, framed_dialog, modal_body, modal_footer,
    modal_header, open_confirm_dialog, select_label_row, select_trigger, switch_halo, text_field,
};
pub use url_bar::UrlBar;
pub use variable_chip::VariableChip;
pub use variable_line::VariableField;
