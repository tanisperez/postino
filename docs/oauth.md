# OAuth 2.0

OAuth 2.0 profiles (#225, #226, #227). Design: page "OAuth" of the design artifact. The user
guide is `site/docs-src/pages/oauth.html`; this document covers how it is built.

## Model

- A **profile** (`postino_core::OAuthProfile`) is one authorization server and client: grant,
  URLs, client, scope, PKCE, redirect, client authentication, where the token goes and extra
  parameters. It is an `[oauth.<name>]` table of `.postino/project.toml`
  (`postino_format::project::ProjectConfig::oauth_profiles`), shared through git, with secrets
  in `.local.env` variables. The keys are listed in the user guide (Files and folders).
- A request references it with `::: auth oauth2` and `profile = <name>`
  (`Auth::OAuth2 { profile, scope }`). A non-empty `scope` asks for another scope.
- A **token** (`postino_oauth::TokenSet`) is stored per workspace, profile, environment and, when
  a request overrides it, scope (`TokenKey`, `postino://<workspace>/<profile>/<env>[?scope=]`).
  The workspace id is the folder name plus the first 6 hex digits of the SHA-256 of its
  canonical path, so two folders with the same name never share tokens.

## Getting a token

`postino_oauth::ensure_token` is what Send runs before the pre script (`Runner::run`, stage 0):

1. Resolve the profile's markers against the session environment and the active environment
   (the pre script has not run, scripts have no network). An unknown variable or a missing
   required field is an error that names the field.
2. A stored token that does not expire in the next 30 s (`EXPIRY_MARGIN`) is reused.
3. An expired one is refreshed when it has a refresh token. The old refresh token is kept when
   the server sends none (RFC 6749, section 6). `invalid_grant` falls through to the grant.
4. Otherwise the profile's grant runs: client credentials or password, one form POST; the
   authorization code grant opens the browser through `SignInHandler`.
5. The token is stored and the request goes out with it, as an `Auth::ApiKey` header
   (`Authorization: <prefix> <token>`) or query parameter, so the usual "an enabled header or
   entry of the same name wins" rule applies.

One lock per token key serializes `ensure_token`, so the workers of a load test never refresh
the same token twice. `Force::Refresh` and `Force::Grant` are the buttons of the token card and
"Get new token and resend"; a 401 from the API never retries by itself.

Every step is recorded in `OAuthRun::steps` (`OAuthNote` lines and `TokenExchange` requests),
which the console shows. Exchanges are masked when recorded: the `Authorization` value, the form
fields `client_secret`, `code`, `code_verifier`, `refresh_token`, `password` and `assertion`, and
the `access_token`, `refresh_token` and `id_token` of the response. Nothing secret reaches
`RunResult`.

## Browser sign-in

RFC 8252: the system browser, never an embedded view (gpui has no web view, and the browser has
the user's sessions and password manager).

- PKCE S256 with a 32 byte verifier from the OS generator (`getrandom`), new on each sign-in, and
  a 16 byte `state`, checked on the redirect.
- `Loopback` listens with `tiny_http` on `127.0.0.1:0` and redirects to
  `http://127.0.0.1:<port>/callback`. `Fixed` listens on the port of a `http://localhost` or
  `http://127.0.0.1` URL. `Manual` listens on nothing: the user pastes the URL.
- The listener answers the browser with a small page (`callback_html`), only on its path; other
  paths get a 404. It closes after the redirect, a cancel, or 5 minutes (`SIGN_IN_TIMEOUT`).
- `SignInHandler` is the UI side: `open` (the app opens the browser and the waiting panel),
  `poll` (cancel, pasted URL) and `page` (translated texts). `NoSignIn` is for places without a
  UI: load tests fail with "sign in from the Postino app first".

## Imports and other places

- `postino_format::oauth_import` turns the OAuth settings of a Postman, Bruno or OpenCollection
  auth (`profile_from_fields`, field names compared ignoring case, `_` and `-`) or an OpenAPI
  `oauth2` scheme into a profile. `ProfileBuilder` shares equal settings, never repeats a name and
  moves a secret written in clear to a `.local.env` variable. The plan carries the profiles, and
  `Workspace::import_openapi` writes them to `project.toml`, reusing a profile with the same
  settings and renaming one whose name is taken.
- `{{ oauthToken("name") }}` and `oauthToken("name", "id_token")`: the runner finds the calls
  (`postino_core::oauth_token_calls`) before the pre script, gets the tokens, and hands them to
  interpolation as request variables whose names start with a control character
  (`oauth_token_variable`), so no environment can collide with them.
- Load tests: `Runner::prepare_oauth` gets every token once before the virtual users start; the
  app gives the load runner its store and `NoSignIn`.
- Command line (#240): the runner with `ConsoleSignIn` (`postino-cli`), which prints the
  authorization URL to stderr and opens the system browser (`xdg-open`, `open`, `rundll32`); the
  loopback listener receives the redirect as in the app. Tokens live in memory (#228), so every
  process signs in again. Without a display (no `DISPLAY` nor `WAYLAND_DISPLAY` on Linux and the
  BSDs) `available()` is false and the run fails with a message that names the grants that work
  headless, client credentials and password.

## Storage

Tokens live in memory only (`MemoryStore`, #228): nothing is written to disk or to a system
credential store, and they are gone when Postino closes. The next Send obtains a new one, which
for the authorization code grant means the browser again. Postino 0.10.0 used the system
credential store through the `keyring` crate, but the Windows Credential Manager holds at most
2560 bytes per entry (1280 UTF-16 characters), less than one access token of many servers.
The app removes the `oauth-tokens.json` file 0.10.0 could leave in the config folder. The app
caches what it read per key (`views/oauth`), so render never locks the store.

## Logging

Info: `oauth: token expired`, `oauth: new token` and `oauth: browser sign-in`, with the profile,
the environment, the grant and lifetimes. Warn: `oauth: no token` with an error code or kind.
Never a token, a secret, a code or a user name.

## Testing

- `postino-oauth`: unit tests for each module and `tests/ensure.rs` against a local token
  endpoint, including the whole authorization code grant through the loopback listener with a
  handler that plays the browser.
- `postino-runner/tests/oauth.rs`: the token in a header and in the query, and an unknown profile.
- The sample suite has an OAuth provider (`/oauth/*`, `docs/sample-suite.md`) and the
  `samples/workspace/oauth/` requests: client credentials, query token, scope override, renewal
  of an expired token, a revoked token (401), and the browser grant, which the automated suite
  expects to ask for a sign-in.
