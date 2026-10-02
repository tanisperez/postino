//! Reusable, styled components (`plans/ui-redesign.md` phase 3): one file per component, each a
//! `RenderOnce` struct with builder methods, reading colors only from
//! [`crate::theme::Palette`] and sizes only from [`crate::theme::metrics`].
//!
//! None of these are wired into the real views yet (phases 4 and 5 do that); the
//! `POSTINO_OPEN=components` debug hook (`gallery`) renders every one of them in every state
//! shown in `postino_design_system/Components.dc.html`, for visual comparison.

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
pub mod status_badge;
pub mod underline_tabs;
pub mod url_bar;
pub mod variable_chip;

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
pub use status_badge::{StatusBadge, StatusState};
pub use underline_tabs::{UnderlineTabItem, UnderlineTabs};
pub use url_bar::UrlBar;
pub use variable_chip::VariableChip;
