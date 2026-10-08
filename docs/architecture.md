# Architecture

How Postino is put together and why. For the project rules (working style, performance, logging)
read `AGENTS.md`; for the file format, `docs/format.md`; for scripting, `docs/scripting.md`; for
the look of the UI, `docs/design-system.md`.

The original planning documents (MVP, UI redesign, spikes, translations, releases) were removed once
executed and are kept in git history under the `plans-archive` tag
(`git show plans-archive:plans/mvp.md`).

## Principles

- **Plain local files.** One request per file (`.postino`), folders are collections, environments
  are `.env` files. Everything diffs cleanly in git. No login, no cloud, no telemetry.
- **A thin UI.** Everything except `postino-app` is plain Rust, testable with `cargo test`: no
  window, no GPU, no network other than a local test server. UI logic that can be plain Rust
  lives under `crates/postino-app/src/state/` with unit tests, and views only render and forward
  input.
- **Readable code over clever code.** Explicit types at public boundaries, small modules, doc
  comments on every public item, no macro magic of our own, no `unsafe` (forbidden by the
  workspace lints).
- **Lightweight.** No async runtime. An idle window does no work (see `AGENTS.md`, Performance).

## Crates

```
postino-app ──> postino-load ──> postino-runner ──> postino-script ──> postino-core
     │               │                  └─────────> postino-http ────> postino-core
     │               └──> postino-http, postino-script, postino-core
     ├──────> postino-runner
     ├──────> postino-script
     ├──────> postino-workspace ─> postino-format ─> postino-core
     ├──────> postino-format
     └──────> postino-update (standalone, depends on no other postino crate)
```

No cycles, and no crate depends on `postino-app`.

| Crate | Responsibility |
| --- | --- |
| `postino-core` | Domain model (`Request`, `Method`, `Body`, `Environment`, ...), `{{ }}` variable interpolation and the built-in template functions. No IO. |
| `postino-format` | `.postino` and `.env` parse and serialize, code snippets (curl, fetch, Python requests), Postman import. No IO. |
| `postino-workspace` | Filesystem access: creates new workspace folders (with the example request), scans a folder, loads and saves requests and environments, writes environment variables, detects the git branch, imports Postman collections to disk. |
| `postino-script` | The `ScriptEngine` trait and its QuickJS implementation (`rquickjs`). |
| `postino-http` | Sends a resolved request with `ureq` 3 (blocking, rustls) and measures timing. |
| `postino-runner` | The pipeline: variables, pre script, interpolate, send, post script. Also `preview`, which resolves a request without scripts or network. |
| `postino-load` | The load test engine, live metrics aggregation, run comparison and run history. |
| `postino-update` | The in-app updater: reads the release manifest, downloads and verifies the asset, and has the macOS and Windows install helpers. |
| `postino-app` | The `gpui` binary (`postino`). Only UI and glue. |

`postino-load` writes its own run history under `.postino/runs/` of the workspace. That is a
deliberate exception to "`postino-workspace` owns the filesystem": routing it through the
workspace crate would need a `workspace -> load -> runner` dependency edge.

`postino-update` is standalone so it can be tested with a local HTTP server and has no way to
touch request data.

## The run pipeline

`Runner::run(&Request, &Environment, &mut SessionEnv) -> RunResult`:

1. Build the variable scope (request variables, session overrides, active environment).
2. Run the pre script, if any, and apply its mutations to the request.
3. Interpolate into a `ResolvedRequest`: only enabled entries, final URL with the query appended.
4. Send it with `postino-http`. A 4xx or 5xx status is a normal response, not an error.
5. Run the post script, if any, and apply its environment changes to the `SessionEnv`.
6. Return everything the UI needs to render (response, tests, console lines, warnings), plus the
   stage that failed, if any.

An exception in the pre script aborts the run before sending. An exception in the post script is
reported and the response is still shown. Unknown variables are left untouched and reported as
warnings.

Request and response data crossing crate boundaries is plain owned data from `postino-core`. No
`rquickjs`, `ureq` or `gpui` types appear in a public API outside their own crate.

## Scripting sandbox

Scripts run on QuickJS behind the `ScriptEngine` trait, with a fresh runtime and context per run
and no state shared between runs. There is no network, no filesystem, no timers and no module
loader: only the request, the response, variables, the environment, `test`/`expect`, `console`
and a small `util` library. Memory (32 MiB) and wall time (5 s) are limited, and exceeding either
is a script error, never a crash. The JS-facing API (`expect`, `test`, header helpers) is written
in an embedded `prelude.js` on top of a few Rust-backed native functions, which keeps the Rust
side to data exchange and limits. The full API is in `docs/scripting.md`.

Template functions inside `{{ }}` (`{{ uuid() }}`) and `util.*` in scripts are backed by the same
Rust functions in `postino-core::functions`, so both levels always behave identically.

## The load test

The load test engine uses one OS thread per virtual user, because `ureq` is blocking. Shared
state is a handful of atomics (stop flag, active users) and one mutex-protected aggregator that
each user locks briefly to record a sample. There are no channels and no async runtime. The UI
polls a snapshot while a run is active; stopping works by an atomic flag that every wait checks
in short sleeps. Finished runs are saved as JSON in `<workspace>/.postino/runs/<NNNN>.json`. The
workspace scanner skips hidden folders, so the history never shows up as a collection.

## Files Postino writes

| File | Content |
| --- | --- |
| `<config dir>/postino/settings.toml` | User settings, applied live, no Save button. On Linux `~/.config/postino/settings.toml`. |
| `<config dir>/postino/recent-workspaces.txt` | One absolute path per line, most recent first, at most 10. |
| `<workspace>/.postino/runs/<NNNN>.json` | Load test history. |
| `<workspace>/environments/<name>.env` | Versioned environment values. |
| `<workspace>/environments/<name>.local.env` | Secrets, merged over the `.env`, meant to be ignored by git (`*.local.env`). |
| Logs | See `AGENTS.md`, Logging. |

## Conventions

- **Errors:** `thiserror` enums in library crates, `anyhow` only in `postino-app`. No `unwrap()`
  or `expect()` outside tests, except for truly impossible states, with a comment saying why.
- **Lints:** `[workspace.lints]` sets clippy `all`, `unwrap_used` and `expect_used` to warn
  (`make lint` turns warnings into errors) and forbids `unsafe_code`. Tests may allow the first
  two at module level.
- **Dependencies:** versions live once in `[workspace.dependencies]` and crates use
  `dep.workspace = true`. Do not add a dependency without a clear reason; prefer small,
  maintained crates. Never mix two `gpui` versions in the tree: `cargo tree -d --depth 0 | grep
  gpui` must stay empty.
- **Tests:** libraries use fixtures and round-trip tests (`serialize(parse(text)) == text`); HTTP
  tests use a local `tiny_http` server; nothing touches the internet. See `AGENTS.md`, Testing.
- **Writing:** no em dash anywhere (code, comments, docs, translations).
- **Releases:** see `docs/releasing.md`. **Translations:** see `docs/i18n.md`. **Working with
  `gpui-kit`:** see `docs/gpui-notes.md`.

## Decisions and their reasons

| Decision | Why |
| --- | --- |
| Own `.postino` format, not `.http` | Inspired by `.http` but deliberately not compatible: Postino needs sections for scripts, docs and typed bodies, and a strict spec with a round-trip guarantee keeps git diffs clean. |
| JavaScript on QuickJS | JavaScript is what users of other HTTP clients already write in scripts, and QuickJS is small, embeddable and sandboxable. It sits behind a trait so the engine can change. |
| `ureq` 3, blocking | Lightweight, no `tokio`. Requests run on `gpui`'s background executor, load tests use one thread per user. |
| Two levels of dynamic values | Template functions cover the common case with no script (`{{ uuid() }}`). Scripts cover anything more involved. Both use the same Rust built-ins. |
| Environments as `.env` files | Trivial to read and diff, and `.local.env` gives secrets a place that git can ignore. |
| One Rust palette for both themes | Colors are written once (`theme/palette.rs`) and the `gpui-kit` theme JSON is derived from it at startup, so there is no second copy to keep in sync. |
| Fonts bundled in the binary | Geist and Geist Mono (SIL OFL 1.1) so the look does not depend on what the OS has installed. |
| `rust-i18n` for translations | `gpui-component` already ships it, so it adds no second i18n system. See `docs/i18n.md`. |

## Out of scope for now

Each item is tracked as a GitHub issue:

- Auth tab with Basic, Bearer and API key (#14), OAuth 2.0 (#15)
- Multipart bodies and file uploads (#51), cookie jar UI (#52)
- WebSocket (#53), GraphQL UI (#54), gRPC (#55)
- Collection runner (#56), command line interface (#57)
- `pm.*` compatibility shim for imported Postman scripts (#58)
- Request history (#59), persisting environment values set by scripts (#60)
- Shared script code between requests (#61), custom ordering in the sidebar (#62)
- Creating a body from a pre script when the request has none (#63)
- Signing and notarizing the macOS app (#46)
