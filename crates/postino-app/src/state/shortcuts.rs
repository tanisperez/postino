//! The data behind the keyboard shortcuts cheat sheet (`F1`, or `Cmd+Shift+/` on macOS): the
//! shortcuts grouped by area, with their translated labels and the keys to show as chips. It
//! mirrors `main.rs`'s `bind_keys`, so a new key binding gets a row here. `views/shortcuts.rs`
//! renders it; this module is plain data, unit tested without a window.

use rust_i18n::t;

/// The modifier key label, matching `main.rs`'s key bindings: `Cmd` on macOS, `Ctrl` elsewhere.
#[cfg(target_os = "macos")]
pub const MODIFIER_KEY: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
pub const MODIFIER_KEY: &str = "Ctrl";

/// One shortcut: what it does and the keys to press, one chip per key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// The translated action label.
    pub label: String,
    /// The keys, in the order they are pressed together (`["Ctrl", "K"]`).
    pub keys: Vec<String>,
}

/// A titled list of shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutGroup {
    /// The translated group title.
    pub title: String,
    /// The group's shortcuts.
    pub shortcuts: Vec<Shortcut>,
}

/// The keys that open the cheat sheet itself: `F1`, or `Cmd+Shift+/` on macOS, where `F1` is
/// the brightness key unless `Fn` is held.
pub fn open_keys() -> Vec<String> {
    if cfg!(target_os = "macos") {
        keys(&["Cmd", "Shift", "/"])
    } else {
        keys(&["F1"])
    }
}

/// [`open_keys`] as one string for a hint (`"F1"`, `"Cmd+Shift+/"`).
pub fn open_hint() -> String {
    open_keys().join("+")
}

/// Every shortcut, grouped. Built once when the cheat sheet opens.
pub fn groups() -> Vec<ShortcutGroup> {
    let modifier = MODIFIER_KEY;
    vec![
        ShortcutGroup {
            title: t!("shortcuts.group.general").to_string(),
            shortcuts: vec![
                shortcut(t!("shortcuts.action.palette"), &[modifier, "K"]),
                shortcut(t!("shortcuts.action.open_folder"), &[modifier, "O"]),
                shortcut(t!("shortcuts.action.settings"), &[modifier, ","]),
                Shortcut {
                    label: t!("shortcuts.action.help").to_string(),
                    keys: open_keys(),
                },
                shortcut(t!("shortcuts.action.close_tab"), &[modifier, "W"]),
                shortcut(t!("shortcuts.action.next_tab"), &["Ctrl", "Tab"]),
                shortcut(
                    t!("shortcuts.action.previous_tab"),
                    &["Ctrl", "Shift", "Tab"],
                ),
            ],
        },
        ShortcutGroup {
            title: t!("shortcuts.group.requests").to_string(),
            shortcuts: vec![
                shortcut(t!("shortcuts.action.send"), &[modifier, "\u{21b5}"]),
                shortcut(t!("shortcuts.action.save"), &[modifier, "S"]),
            ],
        },
        ShortcutGroup {
            title: t!("shortcuts.group.environments").to_string(),
            shortcuts: vec![
                shortcut(
                    t!("shortcuts.action.select_environment"),
                    &[modifier, "1-9"],
                ),
                shortcut(t!("shortcuts.action.no_environment"), &[modifier, "0"]),
            ],
        },
    ]
}

fn shortcut(label: impl ToString, keys_pressed: &[&str]) -> Shortcut {
    Shortcut {
        label: label.to_string(),
        keys: keys(keys_pressed),
    }
}

fn keys(keys_pressed: &[&str]) -> Vec<String> {
    keys_pressed.iter().map(|key| key.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_and_shortcut_has_text_and_keys() {
        let groups = groups();
        assert_eq!(groups.len(), 3);
        for group in &groups {
            assert!(!group.title.is_empty());
            assert!(!group.shortcuts.is_empty());
            for shortcut in &group.shortcuts {
                assert!(!shortcut.label.is_empty());
                assert!(!shortcut.keys.is_empty());
            }
        }
    }

    #[test]
    fn no_two_shortcuts_share_the_same_keys() {
        let mut seen = std::collections::HashSet::new();
        for group in groups() {
            for shortcut in group.shortcuts {
                assert!(
                    seen.insert(shortcut.keys.join("+")),
                    "duplicate keys for {:?}",
                    shortcut.label
                );
            }
        }
    }

    #[test]
    fn the_cheat_sheet_lists_its_own_keys() {
        let general = &groups()[0];
        assert!(
            general
                .shortcuts
                .iter()
                .any(|shortcut| shortcut.keys == open_keys())
        );
    }

    #[test]
    fn the_open_hint_joins_the_keys() {
        assert_eq!(open_hint(), open_keys().join("+"));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(open_hint(), "F1");
    }
}
