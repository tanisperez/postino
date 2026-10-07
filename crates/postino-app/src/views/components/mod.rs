//! Reusable, styled components: one file per component, each a `RenderOnce` struct with builder
//! methods, reading colors only from [`crate::theme::Palette`] and sizes only from
//! [`crate::theme::metrics`].
//!
//! The `POSTINO_OPEN=components` debug hook (`gallery`) renders every one of them in every state, to
//! check against `docs/design-system.md` by eye.

pub mod buttons;
pub mod card;
pub mod document_tabs;
pub mod edit_menu;
pub mod env_pill;
pub mod gallery;
pub mod icon_button;
pub mod inline_message;
pub mod key_value_table;
pub mod method_badge;
pub mod section_label;
pub mod segmented_control;
pub mod select_trigger;
pub mod status_badge;
pub mod text_field;
pub mod underline_tabs;
pub mod url_bar;
pub mod variable_chip;
pub mod variable_line;

pub use buttons::{DangerButton, GhostButton, PrimaryButton, SecondaryButton};
pub use card::Card;
pub use document_tabs::{DocumentTab, DocumentTabs};
pub use edit_menu::edit_menu;
pub use env_pill::{EnvMenuItem, EnvPill};
pub use icon_button::IconButton;
pub use inline_message::{InlineMessage, InlineMessageKind};
pub use key_value_table::{KeyValueRow, KeyValueTable};
pub use method_badge::MethodBadge;
pub use section_label::SectionLabel;
pub use segmented_control::{SegmentedControl, SegmentedItem};
pub use select_trigger::{select_label_row, select_trigger};
pub use status_badge::{StatusBadge, StatusState};
pub use text_field::text_field;
pub use underline_tabs::{UnderlineTabItem, UnderlineTabs};
pub use url_bar::UrlBar;
pub use variable_chip::VariableChip;
pub use variable_line::VariableField;
