//! Action definitions used as `gpui` key bindings and menu handlers.
//!
//! `gpui`'s `actions!` macro (re-exported by `gpui-kit`) turns each name below into a small unit
//! struct that implements `gpui::Action`, so it can be bound to a key combination with
//! `KeyBinding::new` and matched with `on_action`. The first argument is just a namespace label
//! used in debug output, it does not need to refer to a real module.

use gpui_kit::actions;

actions!(
    postino,
    [
        /// Saves the active tab's request to disk (`Ctrl+S` / `Cmd+S`).
        SaveActiveTab,
        /// Sends the active tab's request (`Ctrl+Enter` / `Cmd+Enter`), `plans/mvp.md` Phase 9.
        SendActiveTab,
    ]
);
