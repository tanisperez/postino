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
        /// Sends the active tab's request (`Ctrl+Enter` / `Cmd+Enter`).
        SendActiveTab,
        /// Selects the workspace's 1st environment (`Ctrl 1` / `Cmd 1`).
        SelectEnvironment1,
        /// Selects the workspace's 2nd environment (`Ctrl 2` / `Cmd 2`).
        SelectEnvironment2,
        /// Selects the workspace's 3rd environment (`Ctrl 3` / `Cmd 3`).
        SelectEnvironment3,
        /// Selects the workspace's 4th environment (`Ctrl 4` / `Cmd 4`).
        SelectEnvironment4,
        /// Selects the workspace's 5th environment (`Ctrl 5` / `Cmd 5`).
        SelectEnvironment5,
        /// Selects the workspace's 6th environment (`Ctrl 6` / `Cmd 6`).
        SelectEnvironment6,
        /// Selects the workspace's 7th environment (`Ctrl 7` / `Cmd 7`).
        SelectEnvironment7,
        /// Selects the workspace's 8th environment (`Ctrl 8` / `Cmd 8`).
        SelectEnvironment8,
        /// Selects the workspace's 9th environment (`Ctrl 9` / `Cmd 9`).
        SelectEnvironment9,
        /// Selects "No environment" (`Ctrl 0` / `Cmd 0`).
        SelectNoEnvironment,
        /// Opens the Settings modal (`Ctrl ,` / `Cmd ,`).
        OpenSettings,
        /// Opens the command palette (`Ctrl K` / `Cmd K`).
        OpenCommandPalette,
        /// Opens the keyboard shortcuts cheat sheet (`F1`, or `Cmd+Shift+/` on macOS).
        OpenShortcuts,
        /// Closes the active tab (`Ctrl+W` / `Cmd+W`), asking first when it has unsaved
        /// environment edits like the tab's close button.
        CloseActiveTab,
        /// Activates the next open tab, wrapping around (`Ctrl+Tab`).
        NextTab,
        /// Activates the previous open tab, wrapping around (`Ctrl+Shift+Tab`).
        PreviousTab,
    ]
);
