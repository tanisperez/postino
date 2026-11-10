//! Where tokens live between sends: in memory, never in the workspace and never on disk. A token
//! lasts until Postino closes, and the next send after that obtains a new one (#228).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::token::TokenSet;

/// Identifies the tokens of one profile, for one environment, of one workspace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenKey {
    /// The workspace, see [`workspace_id`].
    pub workspace: String,
    /// The profile name.
    pub profile: String,
    /// The environment name, empty for "No environment".
    pub environment: String,
    /// The scope a request asks for instead of the profile's, `None` when it uses the profile's.
    pub scope: Option<String>,
}

impl TokenKey {
    /// The key as text, `postino://<workspace>/<profile>/<environment>`, with `?scope=<scope>`
    /// when the scope is overridden.
    pub fn to_uri(&self) -> String {
        let environment = if self.environment.is_empty() {
            "-"
        } else {
            &self.environment
        };
        let mut uri = format!(
            "postino://{}/{}/{}",
            self.workspace, self.profile, environment
        );
        if let Some(scope) = &self.scope {
            uri.push_str("?scope=");
            uri.push_str(&postino_core::functions::url_encode(scope));
        }
        uri
    }
}

/// The id of the workspace at `root` in token keys: its folder name and the first 6 hex digits
/// of the SHA-256 of its canonical path, so two folders with the same name never share tokens.
pub fn workspace_id(root: &Path) -> String {
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let hash: String = digest
        .iter()
        .take(3)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{name}-{hash}")
}

/// Reads and writes tokens. Implementations are shared between threads.
pub trait TokenStore: Send + Sync {
    /// The token kept under `key`, `None` when there is none.
    fn get(&self, key: &TokenKey) -> Option<TokenSet>;
    /// Keeps `token` under `key`, replacing what was there.
    fn set(&self, key: &TokenKey, token: &TokenSet);
    /// Removes the token under `key`, if there is one.
    fn remove(&self, key: &TokenKey);
}

/// The store: tokens in memory, gone when the process ends.
#[derive(Debug, Default)]
pub struct MemoryStore {
    tokens: Mutex<BTreeMap<TokenKey, TokenSet>>,
}

impl TokenStore for MemoryStore {
    fn get(&self, key: &TokenKey) -> Option<TokenSet> {
        lock(&self.tokens).get(key).cloned()
    }

    fn set(&self, key: &TokenKey, token: &TokenSet) {
        lock(&self.tokens).insert(key.clone(), token.clone());
    }

    fn remove(&self, key: &TokenKey) {
        lock(&self.tokens).remove(key);
    }
}

/// Locks `mutex`, recovering the data if a thread panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn key(scope: Option<&str>) -> TokenKey {
        TokenKey {
            workspace: "acme-payments-1a2b3c".into(),
            profile: "acme-sso".into(),
            environment: "dev".into(),
            scope: scope.map(str::to_string),
        }
    }

    fn token(access: &str) -> TokenSet {
        TokenSet {
            access_token: access.into(),
            refresh_token: Some("r".into()),
            id_token: None,
            token_type: "Bearer".into(),
            expires_at: Some(100),
            obtained_at: 50,
            scope: "openid".into(),
        }
    }

    #[test]
    fn key_uris() {
        assert_eq!(
            key(None).to_uri(),
            "postino://acme-payments-1a2b3c/acme-sso/dev"
        );
        assert_eq!(
            key(Some("payments:write admin")).to_uri(),
            "postino://acme-payments-1a2b3c/acme-sso/dev?scope=payments%3Awrite%20admin"
        );
        let mut none = key(None);
        none.environment.clear();
        assert!(none.to_uri().ends_with("/acme-sso/-"));
    }

    #[test]
    fn workspace_ids_differ_by_path() {
        let a = tempfile::tempdir().expect("dir");
        let b = tempfile::tempdir().expect("dir");
        let a_id = workspace_id(a.path());
        assert_eq!(a_id, workspace_id(a.path()));
        assert_ne!(a_id, workspace_id(b.path()));
        let name = a
            .path()
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert!(a_id.starts_with(&name), "{a_id}");
        assert_eq!(a_id.len(), name.len() + 7);
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::default();
        store.set(&key(None), &token("one"));
        store.set(&key(Some("x")), &token("two"));
        assert_eq!(store.get(&key(None)), Some(token("one")));
        assert_eq!(store.get(&key(Some("x"))), Some(token("two")));
        store.remove(&key(None));
        store.remove(&key(None));
        assert_eq!(store.get(&key(None)), None);
    }
}
