//! The custom order of environments: `environments/.order`, one environment name per line.
//! [`Workspace::list_environments`] puts the listed environments first, in file order, and the
//! others after them in natural order. A missing file keeps the natural order. The file is
//! personal, like `<name>.local.env`: a new workspace ignores it in its `.gitignore`.

use std::fs;
use std::io;

use crate::ENVIRONMENTS_FOLDER;
use crate::error::WorkspaceError;
use crate::workspace::{Workspace, atomic_write};

/// The name of the order file, inside `environments/`.
pub(crate) const ENV_ORDER_FILE: &str = ".order";

impl Workspace {
    /// Saves `names` as the order of the environments, replacing `environments/.order`.
    /// [`Workspace::list_environments`] returns them in this order from now on, with any
    /// environment not in `names` after them.
    pub fn set_environment_order(&self, names: &[String]) -> Result<(), WorkspaceError> {
        let dir = self.root().join(ENVIRONMENTS_FOLDER);
        fs::create_dir_all(&dir).map_err(|source| WorkspaceError::Io {
            path: dir.clone(),
            source,
        })?;
        atomic_write(&dir.join(ENV_ORDER_FILE), &serialize(names))
    }

    /// The names saved in `environments/.order`, empty when the file does not exist. A file
    /// that cannot be read is logged and treated as missing, so it never hides the environments.
    pub(crate) fn read_environment_order(&self) -> Vec<String> {
        let path = self.root().join(ENVIRONMENTS_FOLDER).join(ENV_ORDER_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                log::warn!("cannot read {}: {error}", path.display());
                Vec::new()
            }
        }
    }

    /// Rewrites `environments/.order` with `edit` applied to its names, only when the file
    /// exists and `edit` changed something. Used by rename and delete so the file follows them.
    pub(crate) fn update_environment_order(
        &self,
        edit: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), WorkspaceError> {
        let mut names = self.read_environment_order();
        if names.is_empty() {
            return Ok(());
        }
        let before = names.clone();
        edit(&mut names);
        if names == before {
            return Ok(());
        }
        self.set_environment_order(&names)
    }
}

/// Parses the order file: one name per line, surrounding whitespace trimmed, blank lines and
/// repeated names skipped.
fn parse(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        let name = line.trim();
        if !name.is_empty() && !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// One name per line, each ended by `\n`.
fn serialize(names: &[String]) -> String {
    names.iter().map(|name| format!("{name}\n")).collect()
}

/// Reorders `names` (already in natural order): the ones in `order` first, in its order, then
/// the rest in their current order. Names in `order` that are not in `names` are ignored.
pub(crate) fn apply_order(names: Vec<String>, order: &[String]) -> Vec<String> {
    let mut sorted: Vec<String> = order
        .iter()
        .filter(|name| names.contains(name))
        .cloned()
        .collect();
    sorted.extend(names.into_iter().filter(|name| !order.contains(name)));
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn parse_trims_and_skips_blank_and_repeated_lines() {
        assert_eq!(
            parse("local\r\n\n  staging \nlocal\nprod"),
            strings(&["local", "staging", "prod"])
        );
        assert!(parse("").is_empty());
    }

    #[test]
    fn serialize_ends_every_line() {
        assert_eq!(serialize(&strings(&["local", "dev"])), "local\ndev\n");
        assert_eq!(serialize(&[]), "");
    }

    #[test]
    fn apply_order_puts_listed_names_first_and_keeps_the_rest_in_order() {
        let names = strings(&["dev", "local", "prod", "staging"]);
        assert_eq!(
            apply_order(names, &strings(&["local", "gone", "prod"])),
            strings(&["local", "prod", "dev", "staging"])
        );
    }

    #[test]
    fn apply_order_without_an_order_keeps_the_names() {
        let names = strings(&["dev", "local"]);
        assert_eq!(apply_order(names.clone(), &[]), names);
    }
}
