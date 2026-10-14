//! Plain data behind the sidebar's Environments panel (GitHub #64): one row per environment of
//! the workspace with its color category and variable count. Loading every environment is file
//! IO, so [`load_env_rows`] runs when the workspace or an environment changes, never in render.
//! Dragging a row reorders the environments ([`moved_order`]).

use postino_workspace::Workspace;

use super::env_color::{EnvColor, env_color};

/// One environment row of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRow {
    /// The environment name (the file name without `.env`).
    pub name: String,
    /// The dot color category, derived from the name.
    pub color: EnvColor,
    /// How many variables the environment has once `.local.env` is merged in. `0` when the
    /// files could not be read.
    pub var_count: usize,
}

/// Loads the rows of every environment of `workspace`, in the picker's order.
pub fn load_env_rows(workspace: &Workspace) -> Vec<EnvRow> {
    let names = workspace.list_environments().unwrap_or_default();
    names
        .into_iter()
        .map(|name| {
            let var_count = workspace
                .load_environment(&name)
                .map_or(0, |environment| environment.variables.len());
            EnvRow {
                color: env_color(&name),
                name,
                var_count,
            }
        })
        .collect()
}

/// The environment order after dragging the one at `from` onto the row at `to` (past the end
/// for the end of the list), which it replaces: the rows in between shift by one, like tabs.
/// `None` when nothing moves.
pub fn moved_order(names: &[String], from: usize, to: usize) -> Option<Vec<String>> {
    if from >= names.len() {
        return None;
    }
    let to = to.min(names.len() - 1);
    if from == to {
        return None;
    }
    let mut order = names.to_vec();
    let name = order.remove(from);
    order.insert(to, name);
    Some(order)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_workspace::{EnvChange, EnvLayer};
    use pretty_assertions::assert_eq;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn moved_order_moves_up_and_down() {
        let list = names(&["dev", "local", "prod"]);
        assert_eq!(
            moved_order(&list, 1, 0),
            Some(names(&["local", "dev", "prod"]))
        );
        assert_eq!(
            moved_order(&list, 0, 2),
            Some(names(&["local", "prod", "dev"]))
        );
        assert_eq!(
            moved_order(&list, 2, 0),
            Some(names(&["prod", "dev", "local"]))
        );
    }

    #[test]
    fn moved_order_past_the_end_moves_to_the_last_place() {
        let list = names(&["dev", "local", "prod"]);
        assert_eq!(
            moved_order(&list, 0, usize::MAX),
            Some(names(&["local", "prod", "dev"]))
        );
    }

    #[test]
    fn moved_order_is_none_when_nothing_moves() {
        let list = names(&["dev", "local"]);
        assert_eq!(moved_order(&list, 1, 1), None);
        assert_eq!(moved_order(&list, 1, usize::MAX), None);
        assert_eq!(moved_order(&list, 5, 0), None);
        assert_eq!(moved_order(&[], 0, 0), None);
    }

    #[test]
    fn rows_carry_color_and_merged_variable_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::open(dir.path()).expect("open");
        workspace
            .save_environment(
                "staging",
                &[EnvChange::Set {
                    layer: EnvLayer::Base,
                    key: "a".to_string(),
                    value: "1".to_string(),
                }],
            )
            .expect("save");
        workspace
            .save_environment(
                "staging",
                &[EnvChange::Set {
                    layer: EnvLayer::Base,
                    key: "b".to_string(),
                    value: "2".to_string(),
                }],
            )
            .expect("save");
        workspace
            .save_environment(
                "staging",
                &[EnvChange::Set {
                    layer: EnvLayer::Local,
                    key: "b".to_string(),
                    value: "3".to_string(),
                }],
            )
            .expect("save");
        workspace
            .save_environment(
                "staging",
                &[EnvChange::Set {
                    layer: EnvLayer::Local,
                    key: "c".to_string(),
                    value: "4".to_string(),
                }],
            )
            .expect("save");
        workspace.create_environment("local").expect("create");

        let rows = load_env_rows(&workspace);

        assert_eq!(
            rows,
            vec![
                EnvRow {
                    name: "local".to_string(),
                    color: EnvColor::Success,
                    var_count: 0
                },
                EnvRow {
                    name: "staging".to_string(),
                    color: EnvColor::Warning,
                    var_count: 3
                },
            ]
        );
    }

    #[test]
    fn a_workspace_without_environments_has_no_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::open(dir.path()).expect("open");
        assert!(load_env_rows(&workspace).is_empty());
    }
}
