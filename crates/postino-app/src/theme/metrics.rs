//! Named layout constants for `plans/ui-redesign.md` section 2.2 (heights, widths, radii). Views
//! read sizes only from here, never as a literal, so every size in the app traces back to one
//! place, the same rule the palette (`theme::palette`) follows for color.
//!
//! Nothing in this crate calls these yet: the shell and components that lay themselves out with
//! them are built in phases 3 and 4. Until then they are unused, so this file allows dead code
//! for itself rather than leaving the constants half-wired or deleting work phase 3 needs.
#![allow(dead_code)]

/// Height of the title bar.
pub const TITLE_BAR_HEIGHT: f32 = 40.0;
/// Height of the open-tabs bar.
pub const OPEN_TABS_BAR_HEIGHT: f32 = 36.0;
/// Height of the response viewer's tab bar.
pub const RESPONSE_TAB_BAR_HEIGHT: f32 = 36.0;
/// Height of the request editor's inner tabs (Params, Headers, Body, ...).
pub const REQUEST_INNER_TABS_HEIGHT: f32 = 32.0;
/// Height of the URL bar and the primary Send button.
pub const URL_BAR_HEIGHT: f32 = 32.0;
/// Height of the primary Send button. Equal to [`URL_BAR_HEIGHT`], named separately because the
/// two are independent design decisions that happen to share a value.
pub const SEND_BUTTON_HEIGHT: f32 = 32.0;
/// Minimum width of the primary Send button (label plus key hint), a few px wider than its
/// natural width so it does not look cramped.
pub const SEND_BUTTON_MIN_WIDTH: f32 = 100.0;
/// Height of ordinary controls (inputs, buttons other than Send).
pub const CONTROL_HEIGHT: f32 = 30.0;
/// Height of a sidebar tree row.
pub const TREE_ROW_HEIGHT: f32 = 26.0;
/// Height of a menu row (environment menu, workspace switcher, ...).
pub const MENU_ROW_HEIGHT: f32 = 28.0;
/// Height of the status bar.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Side length of the status bar's keyboard shortcuts button, a little under its height.
pub const STATUS_BAR_BUTTON: f32 = 20.0;
/// Height of a segmented control's inner items.
pub const SEGMENTED_CONTROL_INNER_HEIGHT: f32 = 22.0;

/// Default sidebar width.
pub const SIDEBAR_WIDTH: f32 = 248.0;
/// Minimum sidebar width when resizing.
pub const SIDEBAR_WIDTH_MIN: f32 = 180.0;
/// Maximum sidebar width when resizing.
pub const SIDEBAR_WIDTH_MAX: f32 = 480.0;
/// Width of the activity rail left of the sidebar.
pub const RAIL_WIDTH: f32 = 48.0;
/// Side length of an activity rail button.
pub const RAIL_BUTTON_SIZE: f32 = 34.0;
/// Height of a row of the Environments panel.
pub const ENV_ROW_HEIGHT: f32 = 30.0;
/// Width of the command palette's search trigger in the title bar.
pub const SEARCH_TRIGGER_WIDTH: f32 = 360.0;
/// Width (and height) of each window control button (minimize, maximize, close).
pub const WINDOW_CONTROL_WIDTH: f32 = 40.0;

/// Radius for chips and checkboxes, the smallest step of the design's radius scale.
pub const RADIUS_XS: f32 = 4.0;
/// Radius for segmented control items, tree rows and badges.
pub const RADIUS_SM: f32 = 6.0;
/// Radius for inputs, buttons and cards in editors.
pub const RADIUS_MD: f32 = 8.0;
/// Radius for menus, popovers and dashboard cards.
pub const RADIUS_LG: f32 = 10.0;
/// Radius for modals (for example the Settings dialog).
pub const RADIUS_MODAL: f32 = 14.0;

/// Height of a `StatusBadge`.
pub const STATUS_BADGE_HEIGHT: f32 = 22.0;
/// Fixed width of a `MethodBadge`'s `label` variant, and of the method label in a sidebar tree
/// row or open tab.
pub const METHOD_LABEL_WIDTH: f32 = 34.0;
/// Side length of an `IconButton`'s small size.
pub const ICON_BUTTON_SM: f32 = 24.0;
/// Side length of an `IconButton`'s large size.
pub const ICON_BUTTON_LG: f32 = 28.0;
/// Side length of the icon buttons in the Collections, Environments and Load tests headers: the
/// small icon in a slightly larger box, so the hover background has more room.
pub const ICON_BUTTON_HEADER: f32 = 22.0;
/// Width of the checkbox and delete columns of a `KeyValueTable`.
pub const KEY_VALUE_SIDE_COL_WIDTH: f32 = 30.0;
/// Width of the column that centers a colored dot in an `EnvMenu` row.
pub const MENU_DOT_COLUMN_WIDTH: f32 = 13.0;
/// Height of the title bar's `EnvPill` trigger (see `docs/design-system.md`).
pub const ENV_PILL_HEIGHT: f32 = 26.0;
/// Height of the sidebar's filter input (see `docs/design-system.md`).
pub const SIDEBAR_FILTER_HEIGHT: f32 = 28.0;
