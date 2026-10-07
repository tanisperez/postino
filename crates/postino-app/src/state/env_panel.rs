//! Plain data behind the sidebar's Environments panel (GitHub #64): one row per environment of
//! the workspace with its color category and variable count. Loading every environment is file
//! IO, so [`load_env_rows`] runs when the workspace or an environment changes, never in render.

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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_workspace::{EnvChange, EnvLayer};
    use pretty_assertions::assert_eq;

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
