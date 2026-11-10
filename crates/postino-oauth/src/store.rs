//! Where tokens live between sends: never in the workspace. [`SystemStore`] uses the system
//! credential store (Keychain, Credential Manager, Secret Service) and falls back to a file only
//! the user can read in Postino's data folder when there is none, as on a Linux desktop without
//! a Secret Service.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::token::TokenSet;

/// The service name of every entry Postino writes in the system credential store.
const KEYRING_SERVICE: &str = "postino";

/// The name of the fallback file inside the data folder.
pub const TOKENS_FILE: &str = "oauth-tokens.json";

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
    /// when the scope is overridden. Used as the credential's account name and as the key of
    /// the fallback file.
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

/// A failure reading or writing a token.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The system credential store failed.
    #[error("system credential store: {0}")]
    Keyring(String),
    /// The fallback file could not be read or written.
    #[error("{path}: {source}")]
    File {
        /// The file.
        path: PathBuf,
        /// What failed.
        #[source]
        source: io::Error,
    },
    /// What was stored is not a token Postino wrote.
    #[error("stored token is not valid: {0}")]
    Corrupt(String),
}

/// Which backend a [`TokenStore`] uses, shown under the token card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    /// The system credential store.
    System,
    /// A file in Postino's data folder, readable only by the user.
    File,
    /// Memory only, nothing survives the process (tests, and the command line when asked).
    Memory,
}

/// Reads and writes tokens. Implementations are shared between threads.
pub trait TokenStore: Send + Sync {
    /// The token stored under `key`, `None` when there is none.
    fn get(&self, key: &TokenKey) -> Result<Option<TokenSet>, StoreError>;
    /// Stores `token` under `key`, replacing what was there.
    fn set(&self, key: &TokenKey, token: &TokenSet) -> Result<(), StoreError>;
    /// Removes the token under `key`. Removing a missing token is not an error.
    fn remove(&self, key: &TokenKey) -> Result<(), StoreError>;
    /// Which backend this is.
    fn kind(&self) -> StoreKind;
}

/// A store that keeps tokens in memory.
#[derive(Debug, Default)]
pub struct MemoryStore {
    tokens: Mutex<BTreeMap<TokenKey, TokenSet>>,
}

impl TokenStore for MemoryStore {
    fn get(&self, key: &TokenKey) -> Result<Option<TokenSet>, StoreError> {
        Ok(lock(&self.tokens).get(key).cloned())
    }

    fn set(&self, key: &TokenKey, token: &TokenSet) -> Result<(), StoreError> {
        lock(&self.tokens).insert(key.clone(), token.clone());
        Ok(())
    }

    fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        lock(&self.tokens).remove(key);
        Ok(())
    }

    fn kind(&self) -> StoreKind {
        StoreKind::Memory
    }
}

/// Locks `mutex`, recovering the data if a thread panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A store in one JSON file, `{ "<key uri>": <token>, ... }`, created with permissions 0600 on
/// Unix. Every call reads the file again, so two Postino windows see each other's tokens.
#[derive(Debug)]
pub struct FileStore {
    path: PathBuf,
    /// Serializes the read-modify-write of [`TokenStore::set`] within the process.
    guard: Mutex<()>,
}

impl FileStore {
    /// A store in `path`. The file and its folder are created on the first write.
    pub fn new(path: PathBuf) -> Self {
        FileStore {
            path,
            guard: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<BTreeMap<String, TokenSet>, StoreError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|error| StoreError::Corrupt(error.to_string()))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(source) => Err(StoreError::File {
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn write(&self, tokens: &BTreeMap<String, TokenSet>) -> Result<(), StoreError> {
        let file_error = |source| StoreError::File {
            path: self.path.clone(),
            source,
        };
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(file_error)?;
        }
        let text = serde_json::to_string_pretty(tokens)
            .map_err(|error| StoreError::Corrupt(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        write_private(&temporary, text.as_bytes()).map_err(file_error)?;
        fs::rename(&temporary, &self.path).map_err(file_error)
    }
}

/// Writes `bytes` to a new file at `path` that only the user can read (0600 on Unix).
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()
}

impl TokenStore for FileStore {
    fn get(&self, key: &TokenKey) -> Result<Option<TokenSet>, StoreError> {
        let _guard = lock(&self.guard);
        Ok(self.read()?.remove(&key.to_uri()))
    }

    fn set(&self, key: &TokenKey, token: &TokenSet) -> Result<(), StoreError> {
        let _guard = lock(&self.guard);
        let mut tokens = self.read().unwrap_or_default();
        tokens.insert(key.to_uri(), token.clone());
        self.write(&tokens)
    }

    fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        let _guard = lock(&self.guard);
        let mut tokens = self.read()?;
        if tokens.remove(&key.to_uri()).is_some() {
            self.write(&tokens)?;
        }
        Ok(())
    }

    fn kind(&self) -> StoreKind {
        StoreKind::File
    }
}

/// The system credential store, or the [`FileStore`] in `data_dir` when there is none. Which
/// one is decided on first use (it may talk to D-Bus, so never on the UI thread) and kept for
/// the life of the process.
pub struct SystemStore {
    file: FileStore,
    available: OnceLock<bool>,
}

impl SystemStore {
    /// A store that falls back to `data_dir/oauth-tokens.json`.
    pub fn new(data_dir: &Path) -> Self {
        SystemStore {
            file: FileStore::new(data_dir.join(TOKENS_FILE)),
            available: OnceLock::new(),
        }
    }

    /// Whether the system credential store works, probed once.
    fn system_available(&self) -> bool {
        *self.available.get_or_init(|| {
            let available = match keyring::Entry::new(KEYRING_SERVICE, "postino://probe") {
                Ok(entry) => matches!(entry.get_password(), Ok(_) | Err(keyring::Error::NoEntry)),
                Err(_) => false,
            };
            if available {
                log::info!("oauth: tokens kept in the system credential store");
            } else {
                log::warn!(
                    "oauth: no system credential store, tokens kept in {}",
                    self.file.path.display()
                );
            }
            available
        })
    }

    fn entry(key: &TokenKey) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(KEYRING_SERVICE, &key.to_uri())
            .map_err(|error| StoreError::Keyring(error.to_string()))
    }
}

impl TokenStore for SystemStore {
    fn get(&self, key: &TokenKey) -> Result<Option<TokenSet>, StoreError> {
        if !self.system_available() {
            return self.file.get(key);
        }
        match Self::entry(key)?.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|error| StoreError::Corrupt(error.to_string())),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(StoreError::Keyring(error.to_string())),
        }
    }

    fn set(&self, key: &TokenKey, token: &TokenSet) -> Result<(), StoreError> {
        if !self.system_available() {
            return self.file.set(key, token);
        }
        let json =
            serde_json::to_string(token).map_err(|error| StoreError::Corrupt(error.to_string()))?;
        Self::entry(key)?
            .set_password(&json)
            .map_err(|error| StoreError::Keyring(error.to_string()))
    }

    fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        if !self.system_available() {
            return self.file.remove(key);
        }
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(StoreError::Keyring(error.to_string())),
        }
    }

    fn kind(&self) -> StoreKind {
        if self.system_available() {
            StoreKind::System
        } else {
            StoreKind::File
        }
    }
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
    fn file_store_round_trips_and_is_private() {
        let dir = tempfile::tempdir().expect("dir");
        let store = FileStore::new(dir.path().join("sub").join(TOKENS_FILE));
        assert_eq!(store.get(&key(None)).expect("get"), None);
        store.set(&key(None), &token("one")).expect("set");
        store.set(&key(Some("x")), &token("two")).expect("set");
        assert_eq!(store.get(&key(None)).expect("get"), Some(token("one")));
        assert_eq!(store.get(&key(Some("x"))).expect("get"), Some(token("two")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("sub").join(TOKENS_FILE))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.remove(&key(None)).expect("remove");
        store.remove(&key(None)).expect("remove twice");
        assert_eq!(store.get(&key(None)).expect("get"), None);
        assert_eq!(store.kind(), StoreKind::File);
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::default();
        store.set(&key(None), &token("one")).expect("set");
        assert_eq!(store.get(&key(None)).expect("get"), Some(token("one")));
        store.remove(&key(None)).expect("remove");
        assert_eq!(store.get(&key(None)).expect("get"), None);
    }
}
