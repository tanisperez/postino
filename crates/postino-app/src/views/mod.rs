//! `gpui` views: the window's visual tree.
//!
//! Application state lives in [`crate::state`], kept free of `gpui` types. These modules render
//! that state and turn user input (clicks, key bindings) into calls back into
//! [`root::AppView`], the single top-level view that owns the state for this phase.

pub mod components;
pub mod env_picker;
pub mod import_menu;
pub mod request_editor;
pub mod response_view;
pub mod root;
pub mod send;
pub mod sidebar;
pub mod status_bar;
pub mod title_bar;

pub use root::AppView;
