//! The workspace configuration file, `.postino/project.toml` (#215): settings that belong to the
//! workspace and are shared with it through git, unlike the user's own `settings.toml`. Today the
//! order of the environments (#216), their colors (#217) and the OAuth 2.0 profiles (#225).
//!
//! The file is optional: a workspace without it behaves as if it were empty. [`ProjectConfig`]
//! keeps the whole TOML table it was parsed from, so keys this version does not know (written by
//! a newer Postino, or by hand) survive a save.

use postino_core::{
    ClientAuth, DEFAULT_TOKEN_PARAM, DEFAULT_TOKEN_PREFIX, ExtraParam, OAuthGrant, OAuthProfile,
    OAuthRedirect, ParamTarget, TokenPlacement,
};
use toml::{Table, Value};

/// The path of the file, relative to the workspace root, with `/` separators.
pub const PROJECT_FILE: &str = ".postino/project.toml";

/// The format version written in new files.
pub const PROJECT_VERSION: i64 = 1;

/// The comment on top of the file.
const HEADER: &str = "# Postino workspace configuration, shared with everyone who uses this workspace.\n# See https://postino.tanis.codes/docs/files-and-folders.html#project-file\n\n";

/// The table of the environment settings.
const ENVIRONMENTS: &str = "environments";
/// The key of the environment order, inside [`ENVIRONMENTS`].
const ORDER: &str = "order";
/// The table of the environment colors, inside [`ENVIRONMENTS`].
const COLORS: &str = "colors";

/// The table of the OAuth 2.0 profiles, one sub-table per profile.
const OAUTH: &str = "oauth";
/// The keys of a profile table this version writes. Any other key is kept as it is.
const PROFILE_KEYS: [&str; 17] = [
    "auth_params",
    "auth_url",
    "client_auth",
    "client_id",
    "client_secret",
    "grant",
    "password",
    "pkce",
    "redirect",
    "redirect_url",
    "scope",
    "token_in",
    "token_param",
    "token_params",
    "token_prefix",
    "token_url",
    "username",
];

/// A `project.toml` that is not valid TOML.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{PROJECT_FILE}: {0}")]
pub struct ProjectError(String);

/// The content of a `project.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectConfig {
    table: Table,
}

impl Default for ProjectConfig {
    /// A new file: only the format version.
    fn default() -> Self {
        let mut table = Table::new();
        table.insert("version".to_string(), Value::Integer(PROJECT_VERSION));
        ProjectConfig { table }
    }
}

impl ProjectConfig {
    /// Parses the text of a `project.toml`. Values of the wrong type are ignored by the getters,
    /// not rejected here, so a hand edit never makes the whole file unreadable.
    pub fn parse(text: &str) -> Result<Self, ProjectError> {
        text.parse::<Table>()
            .map(|table| ProjectConfig { table })
            .map_err(|error| ProjectError(error.message().to_string()))
    }

    /// The text of the file: a short header, then the table with its keys sorted, so the same
    /// configuration always gives the same text.
    pub fn to_toml(&self) -> String {
        let body = toml::to_string(&self.table).unwrap_or_default();
        format!("{HEADER}{body}")
    }

    /// The environment names in the order the user chose, as written. Empty when the user never
    /// reordered them. May name environments that no longer exist.
    pub fn environment_order(&self) -> Vec<String> {
        self.environments()
            .and_then(|environments| environments.get(ORDER))
            .and_then(Value::as_array)
            .map(|order| {
                order
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sets the order of the environments.
    pub fn set_environment_order(&mut self, order: &[String]) {
        let order = order.iter().cloned().map(Value::String).collect();
        self.update_environments(|environments| {
            environments.insert(ORDER.to_string(), Value::Array(order));
        });
    }

    /// The color chosen for the environment `name`, as written (a color name such as `"teal"`),
    /// `None` when it has none.
    pub fn environment_color(&self, name: &str) -> Option<&str> {
        self.environments()?
            .get(COLORS)?
            .as_table()?
            .get(name)?
            .as_str()
    }

    /// Sets the color of the environment `name`, or removes it with `None`.
    pub fn set_environment_color(&mut self, name: &str, color: Option<&str>) {
        self.update_environments(|environments| match color {
            Some(color) => {
                let colors = environments
                    .entry(COLORS)
                    .or_insert_with(|| Value::Table(Table::new()));
                if !colors.is_table() {
                    *colors = Value::Table(Table::new());
                }
                if let Some(colors) = colors.as_table_mut() {
                    colors.insert(name.to_string(), Value::String(color.to_string()));
                }
            }
            None => {
                if let Some(colors) = environments.get_mut(COLORS).and_then(Value::as_table_mut) {
                    colors.remove(name);
                    if colors.is_empty() {
                        environments.remove(COLORS);
                    }
                }
            }
        });
    }

    /// Follows the rename of an environment: its place in the order and its color move to `new`.
    pub fn rename_environment(&mut self, old: &str, new: &str) {
        let order = self.environment_order();
        if order.iter().any(|name| name == old) {
            let renamed: Vec<String> = order
                .into_iter()
                .map(|name| if name == old { new.to_string() } else { name })
                .collect();
            self.set_environment_order(&renamed);
        }
        if let Some(color) = self.environment_color(old).map(str::to_string) {
            self.set_environment_color(old, None);
            self.set_environment_color(new, Some(&color));
        }
    }

    /// Forgets a deleted environment: its place in the order and its color.
    pub fn remove_environment(&mut self, name: &str) {
        let order = self.environment_order();
        if order.iter().any(|entry| entry == name) {
            let kept: Vec<String> = order.into_iter().filter(|entry| entry != name).collect();
            self.set_environment_order(&kept);
        }
        self.set_environment_color(name, None);
    }

    /// The OAuth 2.0 profiles, sorted by name. A profile whose `grant` is missing or unknown is
    /// read as authorization code, and values of the wrong type as empty, so a hand edit never
    /// hides a profile its requests use.
    pub fn oauth_profiles(&self) -> Vec<OAuthProfile> {
        let Some(oauth) = self.table.get(OAUTH).and_then(Value::as_table) else {
            return Vec::new();
        };
        oauth
            .iter()
            .filter_map(|(name, value)| Some(read_profile(name, value.as_table()?)))
            .collect()
    }

    /// The profile named `name`, if there is one.
    pub fn oauth_profile(&self, name: &str) -> Option<OAuthProfile> {
        let table = self.table.get(OAUTH)?.as_table()?.get(name)?.as_table()?;
        Some(read_profile(name, table))
    }

    /// Writes `profile` under its name, replacing what that name held. Keys this version does not
    /// know are kept.
    pub fn set_oauth_profile(&mut self, profile: &OAuthProfile) {
        let mut oauth = match self.table.remove(OAUTH) {
            Some(Value::Table(table)) => table,
            _ => Table::new(),
        };
        let mut table = match oauth.remove(&profile.name) {
            Some(Value::Table(table)) => table,
            _ => Table::new(),
        };
        for key in PROFILE_KEYS {
            table.remove(key);
        }
        write_profile(profile, &mut table);
        oauth.insert(profile.name.clone(), Value::Table(table));
        self.table.insert(OAUTH.to_string(), Value::Table(oauth));
    }

    /// Removes the profile `name`, and the `[oauth]` table when it was the last one.
    pub fn remove_oauth_profile(&mut self, name: &str) {
        if let Some(oauth) = self.table.get_mut(OAUTH).and_then(Value::as_table_mut) {
            oauth.remove(name);
            if oauth.is_empty() {
                self.table.remove(OAUTH);
            }
        }
    }

    /// Renames the profile `old` to `new`, keeping every key. Does nothing when `old` does not
    /// exist.
    pub fn rename_oauth_profile(&mut self, old: &str, new: &str) {
        let Some(oauth) = self.table.get_mut(OAUTH).and_then(Value::as_table_mut) else {
            return;
        };
        if let Some(value) = oauth.remove(old) {
            oauth.insert(new.to_string(), value);
        }
    }

    fn environments(&self) -> Option<&Table> {
        self.table.get(ENVIRONMENTS)?.as_table()
    }

    /// Runs `edit` on the `[environments]` table (a new one when it is missing or not a table),
    /// and drops the table when it ends up empty.
    fn update_environments(&mut self, edit: impl FnOnce(&mut Table)) {
        let mut environments = match self.table.remove(ENVIRONMENTS) {
            Some(Value::Table(table)) => table,
            _ => Table::new(),
        };
        edit(&mut environments);
        if !environments.is_empty() {
            self.table
                .insert(ENVIRONMENTS.to_string(), Value::Table(environments));
        }
    }
}

/// Reads the profile table `table` named `name`.
fn read_profile(name: &str, table: &Table) -> OAuthProfile {
    let text = |key: &str| {
        table
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let grant = OAuthGrant::parse(&text("grant")).unwrap_or_default();
    let mut profile = OAuthProfile::new(name, grant);
    profile.auth_url = text("auth_url");
    profile.token_url = text("token_url");
    profile.client_id = text("client_id");
    profile.client_secret = text("client_secret");
    profile.scope = text("scope");
    profile.username = text("username");
    profile.password = text("password");
    profile.pkce = table.get("pkce").and_then(Value::as_bool).unwrap_or(true);
    profile.redirect = OAuthRedirect::parse(&text("redirect")).unwrap_or_default();
    profile.redirect_url = text("redirect_url");
    profile.client_auth = ClientAuth::parse(&text("client_auth")).unwrap_or_default();
    profile.token_placement = TokenPlacement::parse(&text("token_in")).unwrap_or_default();
    let prefix = text("token_prefix");
    if !prefix.is_empty() {
        profile.token_prefix = prefix;
    }
    let param = text("token_param");
    if !param.is_empty() {
        profile.token_param = param;
    }
    for (key, target) in [
        ("token_params", ParamTarget::Token),
        ("auth_params", ParamTarget::Auth),
    ] {
        if let Some(params) = table.get(key).and_then(Value::as_table) {
            profile
                .extra_params
                .extend(params.iter().filter_map(|(key, value)| {
                    Some(ExtraParam {
                        key: key.clone(),
                        value: value.as_str()?.to_string(),
                        target,
                    })
                }));
        }
    }
    profile
}

/// Writes the keys of `profile` into `table`. Empty texts and default options are left out, so a
/// profile only shows what it sets; `grant` is always written, and so are `pkce` and `redirect`
/// for the authorization code grant.
fn write_profile(profile: &OAuthProfile, table: &mut Table) {
    let mut put = |key: &str, value: &str| {
        if !value.is_empty() {
            table.insert(key.to_string(), Value::String(value.to_string()));
        }
    };
    put("grant", profile.grant.as_str());
    put("auth_url", &profile.auth_url);
    put("token_url", &profile.token_url);
    put("client_id", &profile.client_id);
    put("client_secret", &profile.client_secret);
    put("scope", &profile.scope);
    put("username", &profile.username);
    put("password", &profile.password);
    put("redirect_url", &profile.redirect_url);
    if profile.client_auth != ClientAuth::default() {
        put("client_auth", profile.client_auth.as_str());
    }
    if profile.token_placement != TokenPlacement::default() {
        put("token_in", profile.token_placement.as_str());
    }
    if profile.token_prefix != DEFAULT_TOKEN_PREFIX {
        put("token_prefix", &profile.token_prefix);
    }
    if profile.token_param != DEFAULT_TOKEN_PARAM {
        put("token_param", &profile.token_param);
    }
    if profile.grant == OAuthGrant::AuthorizationCode {
        put("redirect", profile.redirect.as_str());
        table.insert("pkce".to_string(), Value::Boolean(profile.pkce));
    }
    for (key, target) in [
        ("token_params", ParamTarget::Token),
        ("auth_params", ParamTarget::Auth),
    ] {
        let params: Table = profile
            .extra_params
            .iter()
            .filter(|param| param.target == target && !param.key.trim().is_empty())
            .map(|param| (param.key.clone(), Value::String(param.value.clone())))
            .collect();
        if !params.is_empty() {
            table.insert(key.to_string(), Value::Table(params));
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_new_file_only_has_the_version() {
        let text = ProjectConfig::default().to_toml();
        assert!(text.starts_with("# Postino workspace configuration"));
        assert!(text.ends_with("version = 1\n"), "{text}");
        assert_eq!(ProjectConfig::parse(&text), Ok(ProjectConfig::default()));
    }

    #[test]
    fn order_and_colors_round_trip() {
        let mut config = ProjectConfig::default();
        config.set_environment_order(&names(&["local", "dev", "prod"]));
        config.set_environment_color("local", Some("teal"));
        config.set_environment_color("staging env", Some("violet"));
        let text = config.to_toml();
        assert!(
            text.contains("order = [\"local\", \"dev\", \"prod\"]"),
            "{text}"
        );
        assert!(text.contains("local = \"teal\""), "{text}");
        assert!(text.contains("\"staging env\" = \"violet\""), "{text}");

        let parsed = ProjectConfig::parse(&text).expect("parse");
        assert_eq!(parsed.environment_order(), names(&["local", "dev", "prod"]));
        assert_eq!(parsed.environment_color("local"), Some("teal"));
        assert_eq!(parsed.environment_color("staging env"), Some("violet"));
        assert_eq!(parsed.environment_color("prod"), None);
        assert_eq!(parsed.to_toml(), text);
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let text = "version = 2\nfuture = \"x\"\n\n[environments]\nicons = { dev = \"bug\" }\n";
        let mut config = ProjectConfig::parse(text).expect("parse");
        config.set_environment_color("dev", Some("blue"));
        let saved = ProjectConfig::parse(&config.to_toml()).expect("parse");
        assert_eq!(saved.table["version"].as_integer(), Some(2));
        assert_eq!(saved.table["future"].as_str(), Some("x"));
        assert_eq!(
            saved.environments().expect("environments")["icons"]["dev"].as_str(),
            Some("bug")
        );
        assert_eq!(saved.environment_color("dev"), Some("blue"));
    }

    #[test]
    fn values_of_the_wrong_type_are_ignored() {
        let config = ProjectConfig::parse(
            "[environments]\norder = [\"dev\", 3, \"prod\"]\ncolors = { dev = 5, prod = \"red\" }\n",
        )
        .expect("parse");
        assert_eq!(config.environment_order(), names(&["dev", "prod"]));
        assert_eq!(config.environment_color("dev"), None);
        assert_eq!(config.environment_color("prod"), Some("red"));

        let mut broken = ProjectConfig::parse("environments = \"oops\"\n").expect("parse");
        assert!(broken.environment_order().is_empty());
        broken.set_environment_color("dev", Some("blue"));
        assert_eq!(broken.environment_color("dev"), Some("blue"));
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(ProjectConfig::parse("[environments\n").is_err());
    }

    #[test]
    fn clearing_the_last_color_removes_the_table() {
        let mut config = ProjectConfig::default();
        config.set_environment_color("dev", Some("blue"));
        config.set_environment_color("dev", None);
        assert!(!config.to_toml().contains("colors"));
        // Clearing a color that was never set changes nothing.
        config.set_environment_color("prod", None);
    }

    #[test]
    fn rename_and_remove_follow_the_environment() {
        let mut config = ProjectConfig::default();
        config.set_environment_order(&names(&["local", "dev", "prod"]));
        config.set_environment_color("dev", Some("blue"));

        config.rename_environment("dev", "development");
        assert_eq!(
            config.environment_order(),
            names(&["local", "development", "prod"])
        );
        assert_eq!(config.environment_color("dev"), None);
        assert_eq!(config.environment_color("development"), Some("blue"));

        config.remove_environment("development");
        assert_eq!(config.environment_order(), names(&["local", "prod"]));
        assert_eq!(config.environment_color("development"), None);

        // An environment the file does not mention: nothing to do, and no empty keys appear.
        let mut empty = ProjectConfig::default();
        empty.rename_environment("a", "b");
        empty.remove_environment("a");
        assert_eq!(empty, ProjectConfig::default());
    }

    fn acme_sso() -> OAuthProfile {
        let mut profile = OAuthProfile::new("acme-sso", OAuthGrant::AuthorizationCode);
        profile.auth_url = "{{authBaseUrl}}/authorize".into();
        profile.token_url = "{{authBaseUrl}}/oauth/token".into();
        profile.client_id = "{{clientId}}".into();
        profile.scope = "openid profile payments:read".into();
        profile.extra_params = vec![ExtraParam {
            key: "audience".into(),
            value: "https://api.acme.dev".into(),
            target: ParamTarget::Token,
        }];
        profile
    }

    #[test]
    fn a_profile_is_written_as_in_the_design() {
        let mut config = ProjectConfig::default();
        config.set_environment_order(&names(&["local", "dev", "staging", "prod"]));
        config.set_oauth_profile(&acme_sso());
        let text = config.to_toml();
        let expected = "version = 1\n\n[environments]\norder = [\"local\", \"dev\", \"staging\", \"prod\"]\n\n[oauth.acme-sso]\nauth_url = \"{{authBaseUrl}}/authorize\"\nclient_id = \"{{clientId}}\"\ngrant = \"authorization_code\"\npkce = true\nredirect = \"loopback\"\nscope = \"openid profile payments:read\"\ntoken_url = \"{{authBaseUrl}}/oauth/token\"\n\n[oauth.acme-sso.token_params]\naudience = \"https://api.acme.dev\"\n";
        assert!(text.ends_with(expected), "{text}");
        let parsed = ProjectConfig::parse(&text).expect("parse");
        assert_eq!(parsed.oauth_profiles(), vec![acme_sso()]);
        assert_eq!(parsed.oauth_profile("acme-sso"), Some(acme_sso()));
        assert_eq!(parsed.oauth_profile("other"), None);
    }

    #[test]
    fn every_option_of_a_profile_round_trips() {
        let mut profile = OAuthProfile::new("machine", OAuthGrant::Password);
        profile.token_url = "https://auth/token".into();
        profile.client_secret = "{{clientSecret}}".into();
        profile.username = "{{apiUser}}".into();
        profile.password = "{{apiPassword}}".into();
        profile.client_auth = ClientAuth::InBody;
        profile.token_placement = TokenPlacement::Query;
        profile.token_prefix = "Token".into();
        profile.token_param = "token".into();
        profile.extra_params = vec![ExtraParam {
            key: "prompt".into(),
            value: "login".into(),
            target: ParamTarget::Auth,
        }];
        let mut config = ProjectConfig::default();
        config.set_oauth_profile(&profile);
        let parsed = ProjectConfig::parse(&config.to_toml()).expect("parse");
        assert_eq!(parsed.oauth_profile("machine"), Some(profile));
        // Not the authorization code grant: no pkce nor redirect keys.
        assert!(!config.to_toml().contains("pkce"));
    }

    #[test]
    fn profiles_keep_unknown_keys_and_can_be_renamed_and_removed() {
        let mut config = ProjectConfig::parse(
            "[oauth.a]\ngrant = \"client_credentials\"\nfuture = 1\n\n[oauth.b]\ngrant = \"nope\"\n",
        )
        .expect("parse");
        let names_of = |config: &ProjectConfig| {
            config
                .oauth_profiles()
                .into_iter()
                .map(|profile| profile.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names_of(&config), names(&["a", "b"]));
        assert_eq!(
            config.oauth_profile("b").map(|profile| profile.grant),
            Some(OAuthGrant::AuthorizationCode)
        );
        let mut a = config.oauth_profile("a").expect("a");
        a.scope = "read".into();
        config.set_oauth_profile(&a);
        assert!(config.to_toml().contains("future = 1"));

        config.rename_oauth_profile("a", "c");
        assert_eq!(names_of(&config), names(&["b", "c"]));
        assert!(config.to_toml().contains("future = 1"));
        config.remove_oauth_profile("b");
        config.remove_oauth_profile("c");
        assert!(!config.to_toml().contains("oauth"));
    }
}
