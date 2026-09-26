//! Plain data behind the command palette (`Ctrl K`/`Cmd K`, `plans/ui-redesign.md` section 2.3
//! point 1 and phase 7): the items it can show, and the fuzzy matching used to filter and rank
//! them as the user types. The palette view itself is built in phase 7; this module only holds
//! the data and the matching logic, so both are unit tested without a window.

/// A single command palette entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // wired by the command palette view of phase 7
pub struct PaletteItem {
    /// What this entry is and does when chosen.
    pub kind: PaletteItemKind,
    /// The main text shown for this entry, matched against the user's query.
    pub label: String,
    /// Secondary text shown next to the label (a path, an environment's variable count, ...).
    pub detail: Option<String>,
    /// A keyboard shortcut hint shown at the right of the row, if this entry has one.
    pub shortcut: Option<String>,
}

/// What a [`PaletteItem`] represents, and what choosing it does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // wired by the command palette view of phase 7
pub enum PaletteItemKind {
    /// Opens the request with this workspace id.
    Request(String),
    /// Runs a built-in action.
    Action(ActionId),
    /// Switches to this environment, or to "No environment" for `None`.
    Environment(Option<String>),
}

/// A built-in action reachable from the command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // wired by the command palette view of phase 7
pub enum ActionId {
    /// Sends the active tab's request.
    Send,
    /// Saves the active tab.
    Save,
    /// Starts creating a new request.
    NewRequest,
    /// Starts creating a new folder.
    NewFolder,
    /// Opens the Postman collection import dialog.
    ImportCollection,
    /// Opens the Postman environment import dialog.
    ImportEnvironment,
    /// Opens the Settings modal.
    OpenSettings,
    /// Opens the "Open folder" picker.
    OpenWorkspace,
    /// Starts creating a new load test tab.
    NewLoadTest,
    /// Toggles between the light and dark theme.
    ToggleTheme,
}

/// Bonus added once per matched character that starts a word: the first character of a label, or
/// one right after a non-alphanumeric character.
const WORD_START_BONUS: i32 = 10;

/// Penalty subtracted, per skipped character, between two consecutive matched characters.
const GAP_PENALTY: i32 = 8;

/// Filters and ranks `items` by fuzzy-matching `query` against each label, case-insensitive.
///
/// A query matches an item when every one of its characters appears in the label, in order, not
/// necessarily contiguous (a subsequence match), like most command palettes. An empty query
/// matches every item with the same score, keeping their original order.
///
/// Returns one entry per matching item: its index into `items`, a score used to sort matches
/// (highest first: matches at the start of a word and with fewer gaps between matched characters
/// score higher), and the char indices of the matched characters in the label, for the view to
/// highlight them.
#[allow(dead_code)] // wired by the command palette view of phase 7
pub fn fuzzy_filter(query: &str, items: &[PaletteItem]) -> Vec<(usize, i32, Vec<usize>)> {
    let mut matches: Vec<(usize, i32, Vec<usize>)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            fuzzy_match(query, &item.label).map(|(score, indices)| (index, score, indices))
        })
        .collect();
    matches.sort_by_key(|(_, score, _)| std::cmp::Reverse(*score));
    matches
}

/// Tries to match `query` as a case-insensitive subsequence of `text`, greedily picking the
/// earliest occurrence of each character. Returns the score and the matched char indices, or
/// `None` if some character of `query` does not appear, in order, in `text`.
fn fuzzy_match(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let lower_text: Vec<char> = text
        .chars()
        .map(|character| character.to_ascii_lowercase())
        .collect();
    let lower_query: Vec<char> = query
        .chars()
        .map(|character| character.to_ascii_lowercase())
        .collect();

    let mut matched_indices = Vec::with_capacity(lower_query.len());
    let mut score = 0i32;
    let mut search_from = 0usize;
    let mut previous_index: Option<usize> = None;

    for query_char in lower_query {
        let found = lower_text[search_from..]
            .iter()
            .position(|&character| character == query_char)
            .map(|offset| offset + search_from)?;

        let is_word_start = found == 0 || !lower_text[found - 1].is_alphanumeric();
        score += 1;
        if is_word_start {
            score += WORD_START_BONUS;
        }
        if let Some(previous) = previous_index {
            let gap = (found - previous - 1) as i32;
            score -= GAP_PENALTY * gap;
        }

        matched_indices.push(found);
        previous_index = Some(found);
        search_from = found + 1;
    }

    Some((score, matched_indices))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn item(label: &str) -> PaletteItem {
        PaletteItem {
            kind: PaletteItemKind::Request(label.to_string()),
            label: label.to_string(),
            detail: None,
            shortcut: None,
        }
    }

    #[test]
    fn empty_query_matches_every_item_in_order() {
        let items = vec![item("first"), item("second"), item("third")];
        let matches = fuzzy_filter("", &items);
        let indices: Vec<usize> = matches.iter().map(|(index, _, _)| *index).collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn excludes_items_with_no_subsequence_match() {
        let items = vec![item("auth/login"), item("users/list")];
        let matches = fuzzy_filter("zzz", &items);
        assert!(matches.is_empty());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let items = vec![item("Auth/Login")];
        let matches = fuzzy_filter("LOGIN", &items);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn ranks_a_tight_word_start_match_above_a_looser_one() {
        // "lgn" should rank "auth/login" (a tight match right after a word boundary) above
        // "legal notice" (a wider match spanning a word gap).
        let items = vec![item("legal notice"), item("auth/login")];
        let matches = fuzzy_filter("lgn", &items);
        let ranked_labels: Vec<&str> = matches
            .iter()
            .map(|(index, _, _)| items[*index].label.as_str())
            .collect();
        assert_eq!(ranked_labels, vec!["auth/login", "legal notice"]);
    }

    #[test]
    fn reports_the_matched_char_indices() {
        let (_, indices) = fuzzy_match("gt", "get").expect("subsequence match");
        assert_eq!(indices, vec![0, 2]);
    }
}
