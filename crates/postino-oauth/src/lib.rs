//! OAuth 2.0 for Postino (#225, #226): getting, renewing and storing the token of a workspace
//! profile ([`postino_core::OAuthProfile`]) for the active environment, as Send does before the
//! request leaves.
//!
//! - [`ensure_token`] reuses a valid stored token, refreshes an expired one, or runs the
//!   profile's grant (client credentials, password, or authorization code with PKCE in the
//!   system browser), and records every step for the console.
//! - [`TokenStore`] keeps tokens outside the workspace: [`SystemStore`] in the system credential
//!   store, or in a file only the user can read when there is none.
//!
//! No `gpui`: the browser part of the authorization code grant reaches the UI through
//! [`SignInHandler`]. Token requests go through `postino-http`, with the same TLS options as the
//! requests themselves.
#![warn(missing_docs)]

mod browser;
mod error;
mod exchange;
mod manager;
mod resolve;
mod store;
mod token;

pub use browser::{
    CallbackPage, Pkce, SIGN_IN_TIMEOUT, SignInPoll, authorization_url, callback_html,
    code_from_redirect, query_params,
};
pub use error::{OAuthError, ProfileField};
pub use exchange::{MASK, TokenExchange};
pub use manager::{
    Force, NoSignIn, OAuthEnv, OAuthNote, OAuthRun, OAuthStep, SignInHandler, SignInPrompt,
    TokenEvent, ensure_token, token_key,
};
pub use resolve::{ResolvedProfile, resolve_profile};
pub use store::{
    FileStore, MemoryStore, StoreError, StoreKind, SystemStore, TOKENS_FILE, TokenKey, TokenStore,
    workspace_id,
};
pub use token::{EXPIRY_MARGIN, TokenSet, from_unix_seconds, jwt_claims, unix_seconds};
