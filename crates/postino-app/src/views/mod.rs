//! `gpui` views: the window's visual tree.
//!
//! Application state lives in [`crate::state`], kept free of `gpui` types. These modules render
//! that state and turn user input (clicks, key bindings) into calls back into
//! [`root::AppView`], the single top-level view that owns the state for this phase.

pub mod command_palette;
pub mod components;
pub mod define_variable;
pub mod env_editor;
pub mod env_panel;
pub mod env_picker;
pub mod import_menu;
pub mod load_panel;
pub mod load_test;
pub mod navigation;
pub mod request_editor;
pub mod response_view;
pub mod root;
pub mod send;
pub mod settings;
pub mod shortcuts;
pub mod sidebar;
pub mod snippet_dialog;
pub mod status_bar;
pub mod title_bar;
pub mod update;

pub use root::AppView;
