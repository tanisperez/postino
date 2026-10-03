# Postino

A lightweight, native, open source HTTP client in the spirit of Postman, without accounts and
without cloud storage. Your requests live as plain files on your disk, and you decide where
they go: git, a shared folder, or nowhere else.

> Status: the MVP described below is implemented and has been verified on Linux. Windows and
> macOS builds have not been verified yet.

## Goals

- Cross-platform desktop app: Linux, Windows and macOS.
- Written in Rust, with the UI built on [`gpui`](https://crates.io/crates/gpui), the GPU-accelerated
  framework behind the Zed editor.
- A UI familiar to Postman users: collections, requests, environments, response viewer.
- **No login, no telemetry, no online sync.** Nothing leaves your machine except the requests you send.
- Requests are stored locally as human readable text files, designed to produce clean diffs in git.
- Fully open source.

## Non-goals (for now)

- Team workspaces, cloud sync or any hosted service.

## The MVP

- HTTP requests: any method (the standard ones, or a custom token), URL, query parameters,
  headers (each individually enabled or disabled), and a body of type JSON, text, XML or form.
- Collections stored as folders, one `.postino` file per request, natural sort order in the
  sidebar.
- Environments with variables, plus a `.local.env` override file for secrets (see below).
- Pre-request and post-response scripts in JavaScript, sandboxed on QuickJS, with `test()`/
  `expect()` assertions.
- The same built-in functions (`uuid()`, `now()`, `base64Encode()`, ...) available both as
  `{{ }}` template calls and as `util.*` inside a script.
- A response viewer: status, headers, timing, a pretty-printed or raw body, test results and
  captured console output.
- Importing Postman collections (v2.1) and Postman environment exports.

The `.postino` request file format is documented in full in [`docs/format.md`](docs/format.md);
the script API (`req`, `res`, `vars`, `env`, `test`, `expect`, `console`, `util`) is documented in
[`docs/scripting.md`](docs/scripting.md).

### Environments

Each environment is a pair of files under `environments/` at the root of a workspace:
`<name>.env` (plain variables, meant to be versioned) and an optional `<name>.local.env`
(secrets, meant to stay local; its values override `<name>.env`). Add this to your workspace's
`.gitignore`:

```
target/
*.local.env
```

### Running Postino

```
cargo run -- examples/sample-workspace
```

opens the bundled sample workspace directly. Two environment variables are read at startup for
manual end-to-end checks, and have no effect if unset:

- `POSTINO_ENV=<name>` selects an environment (a name under `environments/` in the opened
  workspace) before sending anything.
- `POSTINO_AUTOSEND=<request id>` opens that request (a workspace-relative id, for example
  `auth/login.postino`) and sends it immediately at startup.

## Future (after the MVP)

Left out of the MVP on purpose, candidates for later versions:

- Multipart bodies and file uploads.
- Cookie jar UI.
- Auth helpers beyond plain headers (Basic, Bearer, API key forms) and OAuth flows.
- WebSockets, GraphQL-specific UI and gRPC.
- Collection runner (run a whole folder in sequence).
- Command line interface to run requests without the UI.
- A `pm.*` compatibility shim so imported Postman scripts run unchanged.
- Request history.
- Persisting environment values set by scripts to disk.
- Sharing script code between requests (imports or collection-level scripts).
- Custom ordering of requests in the sidebar (today it is natural sort by name).
- Responses larger than 10 MiB (the current `ureq` read limit).
- Creating a body from a pre script when the request has none.

A few decisions are still open: final name (trademark check), domain and GitHub
organization, and the language of the docs. See
`AGENTS.md` for the full list.

## Landscape

Similar tools exist and are worth studying: Bruno, Yaak, Hoppscotch and Insomnia. Postino's
angle is a native, fast GPUI app with an open and diff-friendly file format.

## Updates

The macOS and Windows builds downloaded from GitHub check for a new version once, ten seconds
after startup. The check is a single download of `latest.json` from GitHub: no identifiers, no
telemetry. A newer version is downloaded and verified in the background, and Postino only
installs it when you click "restart" in the status bar. Turn the automatic check off in
Settings, Advanced; "Check for updates" in the command palette still works. Linux packages and
builds from source have no updater.

## Development

Requires the Rust toolchain (via `rustup`) and the system libraries that GPUI needs on your
platform.

```
make help
```

Releases are built by CI from version tags, see [docs/releasing.md](docs/releasing.md).

## License

Postino is licensed under the [Apache License 2.0](LICENSE).
