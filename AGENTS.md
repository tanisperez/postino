# AGENTS.md

Guidance for AI agents working on Postino. See `README.md` for the project overview.

## Project

Postino is an open source, lightweight, native HTTP client (a Postman alternative) written in
Rust with the `gpui` crate. Target platforms: Linux, Windows and macOS. No login, no cloud:
requests are plain local files that the user may version with git or store anywhere.

It is a desktop project, unrelated to the Expo mobile apps in `~/dev`. Do not reuse the
Expo/React Native conventions here.

## Decisions taken

- Name: **Postino**. Legal caveat: it is the Italian word for "postman". Do not use postman
  imagery or branding, and never present the tool as "Postman in Italian". Mentioning that it
  imports Postman collections is fine.
- Stack: Rust (edition 2024, stable toolchain) + `gpui`.
- UI: close to Postman's layout (sidebar of collections, request editor, response viewer),
  without login and without online storage.
- Storage: local text files, one request per file, folders as collections. Must give clean git diffs.
- Second future project (not here): a Winamp reimplementation, classic `.wsz` skins first.
- Request file format: custom plain text, extension `.postino`, one request per file. Inspired by
  `.http` but deliberately not compatible with it. Full spec in `docs/format.md`.
- Scripting: JavaScript on QuickJS via `rquickjs`, behind a `ScriptEngine` trait
  (`postino-script`). Two levels share the same Rust built-ins: template functions inline in
  `{{ }}` markers (`util.*`-equivalent, no script needed), and full `::: pre`/`::: post` scripts
  for anything more involved. See `docs/scripting.md`.
- Widgets: [`gpui-kit`](https://crates.io/crates/gpui-kit) 0.7.0, which bundles `gpui-component`
  and `gpui-base` and pins `gpui-pre` 0.3.7 (`cargo tree -d --depth 0 | grep gpui` must stay
  empty). Since 0.7.0 `Root` renders the dialog, sheet and notification layers itself; views
  must not render them again.
- MVP scope: HTTP methods (standard or custom), URL, query params, headers, body (json/text/
  xml/form), environments and variables, pre/post JavaScript scripts with `test()`/`expect()`,
  a response viewer, Postman collection import. See `plans/mvp.md` section 1 for the full table.
- Postman import: Collection format v2.1 (v2.0 accepted when it parses the same) and Postman
  environment exports.
- HTTP client: `ureq` 3 (blocking, rustls), run on GPUI's background executor.
- License: Apache-2.0 (`LICENSE`, `license` in `[workspace.package]`). Permissive, compatible with
  GPUI, includes a patent grant and does not grant trademark rights. Contributor terms (CLA or
  DCO) are still undecided, ask before accepting external contributions.

## Pending decisions (ask before choosing)

These are to be settled in a dedicated planning session. Do not pick one silently.

- Final name, after checking trademark databases (USPTO, EUIPO, OEPM) for "Postino". Backup
  candidates: Corriere, Piccione.
- Domain and GitHub organization for the project.
- Language of the docs (currently English).

## Architecture

A Cargo workspace of seven small crates, UI kept thin. Dependency direction (arrows mean
"depends on"), no cycles, no crate depends on `postino-app`:

```
postino-app ──> postino-runner ──> postino-script ──> postino-core
     │                 └────────> postino-http ────> postino-core
     ├──────> postino-workspace ─> postino-format ─> postino-core
     └──────> postino-format (the Code snippet dialog renders `render_snippet` directly)
```

- `postino-core`: domain model (`Request`, `Method`, `Body`, ...) and `{{ }}` variable
  interpolation. No IO.
- `postino-format`: `.postino` and `.env` parse/serialize, Postman import. No IO.
- `postino-workspace`: filesystem access, scans a folder, loads/saves requests and environments.
- `postino-script`: the `ScriptEngine` trait and its QuickJS implementation.
- `postino-http`: sends a resolved request with `ureq`, measures timing.
- `postino-runner`: the pipeline, vars, pre script, interpolate, send, post script.
- `postino-app`: the `gpui` binary (`postino`). Only UI and glue.

Everything except `postino-app` is testable with `cargo test`, with no window, no GPU and no
network other than a local test server.

`postino-app` must call `.with_assets(gpui_kit::assets::AllAssets)` when building the `gpui`
application (`main.rs`), or every `Icon` (tree chevrons, window controls, checkboxes, ...) renders
as empty space. `AllAssets` embeds the full Lucide catalog used through
`gpui_kit::assets::IconName`; the smaller `Assets` only has gpui-component's curated subset, and
any other icon silently renders empty. On Linux, the window is opened with `WindowDecorations::Client`, so `gpui-kit`'s
`TitleBar` and `Root` draw the whole window chrome themselves instead of the compositor.

## Testing

- HTTP tests run against a local `tiny_http` server bound to `127.0.0.1:0`, behind
  `postino-http`'s `test-support` feature. No test in the workspace touches the internet.
- UI logic (open tabs, dirty state, the active environment, ...) lives in plain Rust under
  `postino-app/src/state`, not in `gpui` views, specifically so it can be unit tested without a
  window or a GPU.

## Performance

Found in the 2026-09-30 investigation (#32, #33, #34). Keep these rules in every change.

- An idle window does no work. gpui re-renders the whole window whenever any entity calls
  `cx.notify()`, so never add timers, polling or animations that notify while nothing changes.
  Check it: with the app idle for 10 s, the main thread CPU ticks (fields 14 and 15 of
  `/proc/<pid>/task/<pid>/stat`) must barely move and the GPU counters (`drm-engine-render` in
  `/proc/<pid>/fdinfo/*`) must not move at all.
- `render` runs on every frame: every hover, keystroke and cursor blink. Nothing in a render path
  may cost in proportion to the data size. No parsing, pretty printing, syntax work or full text
  comparison of request or response bodies, and no file IO. Compute derived data once when its
  source changes (a new response, a toggle), cache it under a cheap key (a tab id plus a
  generation counter, see `state/response_render.rs`) and share big texts as `Arc<str>` or
  `SharedString`, so clones are O(1).
- Resync an editor (`set_value`) only when that cheap key changes, never by reading its whole
  value and comparing it with the new text on every render.
- Test with big data, not only `examples/sample-workspace`: a 5 MB JSON response (below the
  10 MB response body limit), measuring main thread CPU while idle and while typing, and RSS.
- Expected memory on Linux: about 115 MB RSS at startup in release, of which only about 22 MB is
  the app's own heap; the rest is shared libraries (Mesa, LLVM) and the binary itself. gpui also
  allocates GPU memory that is not in RSS but lives in system RAM on integrated GPUs: the
  swapchain plus two window-sized path textures (one with 4x MSAA), about five framebuffers.
  That part is gpui's design and grows with the window size.
- After upgrading `gpui-kit`, re-check the idle rule above. gpui-base 0.6.6 had a bug where an
  input whose text was set while unfocused kept blinking its cursor forever, re-rendering the
  window twice per second.
- Release profile: fat LTO, one codegen unit, stripped (see the comment in `Cargo.toml`). Put
  alternate builds (another profile, a dependency upgrade) under a subdirectory of `target/`:
  a full gpui build takes several GB.

## Logging

Added in #36. `postino-app/src/logging.rs` installs a small logger behind the `log` facade, so
gpui, wgpu, ureq and rustls messages reach it too. Library crates only depend on `log`.

- File: `~/.local/state/postino/postino.log` on Linux (`$XDG_STATE_HOME`),
  `~/Library/Logs/Postino/` on macOS, `%LOCALAPPDATA%\Postino\logs\` on Windows. Rotated at
  5 MB, 10 files at most.
- Level: Settings, "Advanced" (`log_level` in `settings.toml`, Info by default). `POSTINO_LOG`
  overrides it at startup. Crates other than `postino*` and `gpui*` are capped at Warn, or Info
  at Trace (`naga` alone writes about 20 MB at Debug on every startup).
- What goes where: Info for startup, workspace opened, one line per request sent from a tab,
  imports, load test start and end, settings changes. Warn for failed operations. Debug for
  tabs, environments, saves and response render cache rebuilds. Trace for runner stage timings,
  request headers, and one line per `AppView` render: a render count that grows while the app is
  idle breaks the idle rule below.
- Privacy: never log a body, a variable value, a query string or a sensitive header value. Use
  `postino_core::log_safe` (`url_for_log`, `header_value_for_log`) and
  `state::workspace_log::log_workspace_error`, since parse errors quote file lines.
- Load tests run `Runner::run` thousands of times: nothing per request in the runner or
  `postino-http` above Trace.

## Working rules

- Git is enabled (local repository, no remote yet). Commits are fine; never `git push` unless
  the user says so explicitly. Commit messages in English.
- Keep changes small and traceable to the request. Do not refactor unrelated code.
- Run `make format` and `make lint` before considering a task done, and `make test` when there
  are tests.
- Match the style of the surrounding code. Formatting is enforced by `rustfmt.toml` and
  `.editorconfig` (4 spaces, LF, max width 100).
- GPUI evolves quickly. Check the version pinned in `Cargo.toml` and read its actual source or
  examples instead of relying on memory of its API.
- Writing style: never use the em dash in code, comments or docs. Use a comma or a period.

## Commands

See `make help`. Main targets: `run`, `build`, `release`, `check`, `lint`, `format`, `test`.
