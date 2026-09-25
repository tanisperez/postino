# Postino MVP plan

Status: approved in the planning session of 2026-09-25. This document is the source of truth for
scope and architecture. It is meant to be executed phase by phase by subagents. Each phase lists
its acceptance criteria; a phase is done only when all of them pass.

## 0. Rules for whoever executes this plan

- Read `AGENTS.md` first. Its working rules apply (no em dash anywhere, `make format`,
  `make lint`, `make test`, max width 100, 4 spaces).
- GPUI evolves quickly and your memory of its API is probably wrong. Before writing any UI code,
  read the actual source of the pinned versions in `~/.cargo/registry/src/*/gpui-kit-0.6.6`,
  `gpui-component-0.6.6`, `gpui-base-0.6.6` and `gpui-pre-0.3.6` (run `cargo fetch` first if
  they are missing). Also read `examples/` in the upstream repo if you need a reference
  (`https://github.com/longbridge/gpui-kit`).
- Do not add dependencies that are not listed here without a clear reason. If one is needed,
  prefer small, maintained crates and write down why in the phase report.
- The owner of the project has limited Rust experience. Favor plain, readable code over clever
  code: explicit types at public boundaries, small modules, doc comments on every public item,
  no macro magic of our own, no `unsafe`.
- Error handling: `thiserror` enums in library crates, `anyhow` only in `postino-app`.
  No `unwrap()` or `expect()` outside tests, except for truly impossible states, with a comment.
- Each phase ends with a short report: what was built, deviations from the plan and why, and the
  output summary of `make format`, `make lint` and `make test`.

## 1. Decisions taken in the planning session

| Topic | Decision |
|---|---|
| Request file format | Custom plain text, extension `.postino`, one request per file. Inspired by `.http` but deliberately not compatible with it. Spec in section 3. |
| Dynamic values | Both levels. Level A: built-in template functions inside `{{ }}` (for example `{{ uuid() }}`), implemented in Rust, see section 3.6. Level B: pre-request scripts that set variables (`vars.set("id", util.uuid())`) used as `{{id}}`. The same Rust built-ins back both levels. |
| Scripting | JavaScript on QuickJS via `rquickjs`, hidden behind a `ScriptEngine` trait. Scripts are inline in the request file (`::: pre`, `::: post`). |
| Sandbox | Scripts are isolated: no network, no filesystem, no timers, no process access. Only the request, the response, variables, environment, tests and a small `util` library. Memory and time limits enforced. |
| Tests | `test()` and `expect()` in post scripts are part of the MVP. |
| Environments | `environments/<name>.env` (versioned) plus optional `environments/<name>.local.env` (secrets, meant to be ignored by git), merged with local winning. |
| Widgets | `gpui-kit` 0.6.6 (bundles `gpui-component`, `gpui-base` and pins `gpui-pre` 0.3.6). |
| Postman import | In the MVP. Postman Collection format v2.1 (v2.0 accepted when it parses the same) and Postman environment exports. |
| Architecture | Cargo workspace with small crates, UI kept thin. See section 2. |
| HTTP client | `ureq` 3 (blocking, rustls), run on GPUI's background executor. Lightweight, no tokio. |

Out of scope for the MVP: multipart bodies, file uploads, cookies jar UI, auth helpers beyond
plain headers, OAuth flows, WebSockets, GraphQL-specific UI, gRPC, collection runner, CLI,
`pm.*` compatibility shim, request history, persisting script-set environment values to disk,
shared script code between requests, custom sidebar ordering, responses larger than 10 MiB (the
current `ureq` read limit), creating a body from a pre script when the request has none. The same
list lives in `README.md` under "Future (after the MVP)"; keep both in sync.

## 2. Architecture

```
postino/
├── Cargo.toml                 workspace manifest, shared [workspace.dependencies] and lints
├── Makefile                   already exists, do not change targets
├── crates/
│   ├── postino-core/          domain model and variable interpolation. No IO.
│   ├── postino-format/        .postino and .env parse/serialize, Postman import. No IO.
│   ├── postino-workspace/     filesystem: scan a folder, load and save requests, environments.
│   ├── postino-script/        ScriptEngine trait + QuickJS implementation.
│   ├── postino-http/          send a resolved request with ureq, measure timing.
│   ├── postino-runner/        the pipeline: vars -> pre script -> interpolate -> send -> post script.
│   └── postino-app/           the gpui binary (`postino`). Only UI and glue.
└── plans/
```

Dependency direction (arrows mean "depends on"). No cycles, no crate depends on `postino-app`:

```
postino-app ──> postino-runner ──> postino-script ──> postino-core
     │                 └────────> postino-http ────> postino-core
     └──────> postino-workspace ─> postino-format ─> postino-core
```

Why this split: everything except `postino-app` is testable with `cargo test`, with no window,
no GPU and no network other than a local test server. The UI is a thin layer over plain data.

### Workspace manifest

- `[workspace] resolver = "3"`, `members = ["crates/*"]`.
- `[workspace.package]`: `edition = "2024"`, `version = "0.1.0"`, `license` left unset
  (pending decision), `publish = false`.
- `[workspace.dependencies]` with the versions below; crates use `dep.workspace = true`.
- `[workspace.lints.clippy]`: `all = "warn"` (make lint turns warnings into errors),
  `unwrap_used = "warn"`, `expect_used = "warn"`. Tests may `#[allow]` them at module level.
  `[workspace.lints.rust]`: `unsafe_code = "forbid"`, `missing_docs = "warn"` for library crates
  (set per crate with `#![warn(missing_docs)]` if workspace level is too noisy for the app).
- `[profile.release]`: `lto = "thin"`, `codegen-units = 1`, `strip = true`.
- `default-members` must include `crates/postino-app` so `make run` (`cargo run`) starts the app.

### Pinned dependency versions (checked on 2026-09-25)

| Crate | Version | Used by |
|---|---|---|
| gpui-kit | 0.6.6 | app (re-exports gpui, use `gpui_kit::gpui` or the re-export it provides) |
| rquickjs | 0.14 | script |
| ureq | 3.4 | http |
| serde, serde_json | 1 | format (Postman), script (JSON bridge) |
| thiserror | 2 | libraries |
| anyhow | 1 | app |
| uuid (features v4) | 1.26 | core (built-in functions) |
| base64 | 0.23 | core (built-in functions) |
| fastrand | 2 | core (built-in functions) |
| pretty_assertions | 1.4 | dev, all crates |
| tempfile | 3 | dev, workspace, app |
| tiny_http | 0.12 | dev, http and runner (local test server) |

If `gpui-kit` does not expose gpui directly, depend on
`gpui = { package = "gpui-pre", version = "=0.3.6" }` so it matches exactly. Never mix two gpui
versions in the tree (`cargo tree -d | grep gpui` must be empty).

## 3. The `.postino` file format (v1)

### 3.1 Example

```
POST {{baseUrl}}/users
Content-Type: application/json
Authorization: Bearer {{token}}
X-Timestamp: {{ now() }}
# X-Debug: 1

::: query
page = 1
# verbose = true

::: body json
{ "name": "{{userName}}", "id": "{{requestId}}" }

::: pre
vars.set("requestId", util.uuid());

::: post
test("created", () => {
  expect(res.status).toBe(201);
});
env.set("userId", res.json().id);
```

### 3.2 Grammar

1. Encoding UTF-8. Line endings: accept LF and CRLF on read, always write LF.
2. **Request line** (first non-empty line): `<METHOD> <URL>`. `METHOD` is one of `GET POST PUT
   PATCH DELETE HEAD OPTIONS` or any other uppercase token (custom methods are kept verbatim).
   The URL is the rest of the line, trimmed. It may contain `{{vars}}` and a query string.
3. **Header block**: lines after the request line until the first blank line or the first
   section marker. Each line is `Name: value` (split on the first `:`, both sides trimmed).
   A line starting with `#` followed by optional spaces is a **disabled** header
   (`# X-Debug: 1`). Header order is preserved.
4. **Sections**: a line starting with `::: ` begins a section: `::: <kind> [argument]`.
   Allowed kinds and arguments:
   - `query`: lines `key = value` (split on the first `=`, trimmed). `#` prefix disables.
     An entry without `=` is a key with empty value.
   - `body <type>`: `type` is `json`, `text`, `xml` or `form`. For `json`, `text` and `xml` the
     content is raw text. For `form` (urlencoded) the content uses the same `key = value` lines
     as `query`.
   - `pre`: JavaScript, raw text.
   - `post`: JavaScript, raw text.
   - `docs`: free Markdown notes, raw text.
   Each kind may appear at most once. Unknown kinds are a parse error with line number.
5. **Raw section content** runs until the next line that starts with `::: ` or EOF. Trailing
   blank lines of a section are not part of its content. A content line that would start with
   `::: ` is written escaped as `\::: ` and unescaped on read (and a literal `\::: ` is written
   as `\\::: `). This is the only escape rule.
6. **Canonical serialization order**: request line, headers, blank line, then sections in the
   order `query`, `body`, `pre`, `post`, `docs`, each separated by one blank line, file ends with
   a single `\n`. Empty sections are omitted. The parser accepts any section order, the
   serializer always writes the canonical one. This is what keeps git diffs clean.
7. Lines starting with `#` are only meaningful inside the header block and `query`/`form`
   sections (disabled entries). There are no comments elsewhere, so every file round-trips.
8. The request **name** is the file name without `.postino`. Display order in the sidebar is
   natural sort of names, folders first.

### 3.3 Round-trip guarantee

`serialize(parse(text)) == text` for every canonically formatted file, and
`parse(serialize(model)) == model` for every model. Both are tested with fixtures and with a
set of hand-written edge cases (CRLF, escaped markers, empty values, unicode, `=` inside values,
`:` inside header values such as URLs).

### 3.4 Environment files

`environments/<name>.env`, one `KEY=value` per line (split on the first `=`, key trimmed, value
kept verbatim except the line ending). `#` starts a comment line. Blank lines ignored. No
quoting, no escapes, no interpolation between variables in the MVP.
`environments/<name>.local.env` is optional and overrides keys of `<name>.env`. The README must
recommend adding `*.local.env` to `.gitignore`. The `environments/` folder is not shown as a
collection in the sidebar.

### 3.5 Variable resolution

`{{name}}` (whitespace inside braces allowed: `{{ name }}`) is replaced in URL, header names and
values, query keys and values, form fields and raw bodies. Lookup order, first hit wins:

1. request variables set by the pre script (`vars.set`), valid only for this execution;
2. runtime environment overrides set by scripts (`env.set`), in memory for the app session;
3. the active environment (`.local.env` merged over `.env`).

Unknown variables are left untouched and reported as warnings in the result (the UI shows them).
Interpolation is single pass: a value containing `{{x}}` is not expanded again.

### 3.6 Template functions (level A)

Inside `{{ }}`, an expression with parentheses is a function call instead of a variable:
`{{ uuid() }}`, `{{ randomInt(1, 100) }}`, `{{ base64Encode("user:pass") }}`,
`{{ base64Encode(username) }}`.

- Grammar: `name(arg, arg, ...)`. An argument is a double-quoted string literal (with `\"` and
  `\\` escapes), an integer literal, or a bare identifier that is looked up as a variable with
  the rules of section 3.5. No nesting of calls, no operators. This keeps it tiny and safe.
- Built-in functions: `uuid()`, `now()` (ms since epoch), `isoDate()` (UTC, RFC 3339),
  `randomInt(min, max)` (inclusive), `randomString(len)` (alphanumeric), `base64Encode(str)`,
  `base64Decode(str)`, `urlEncode(str)`.
- Errors (unknown function, wrong arity, bad argument, unknown variable argument) do not abort:
  the expression is left untouched and a warning is reported, like a missing variable.
- Each occurrence is evaluated independently: two `{{ uuid() }}` in one request give two values.
- Implemented in `postino-core::functions` as plain Rust. `postino-script` exposes the very same
  functions as `util.*`, so both levels always behave identically.

## 4. Script API (JavaScript, QuickJS)

Globals available in both `pre` and `post` unless stated:

| Name | Description |
|---|---|
| `req` | The request about to be sent. `req.method`, `req.url` (strings, writable in pre), `req.headers` (object helpers: `get(name)`, `set(name, value)`, `remove(name)`), `req.body` (string, writable in pre). In post it is read-only and reflects what was actually sent. |
| `res` | Only in post. `res.status` (number), `res.headers.get(name)`, `res.body` (string), `res.json()` (parses body, throws on invalid JSON), `res.timeMs`, `res.size`. |
| `vars` | `get(name)`, `set(name, value)`. Request-scoped variables. |
| `env` | `get(name)`, `set(name, value)`, `unset(name)`. Session overrides of the active environment. Not written to disk in the MVP. |
| `test(name, fn)` | Runs `fn`, records pass or fail with the error message. A failing test does not stop the script. |
| `expect(value)` | Returns matchers: `toBe`, `toEqual` (deep, via JSON), `toBeTruthy`, `toBeFalsy`, `toContain` (string or array), `toMatch` (regex or string), `toBeGreaterThan`, `toBeLessThan`, `toHaveProperty(name)`, and `.not` negating any of them. |
| `console.log/info/warn/error` | Captured into the result, shown in the UI console tab. |
| `util` | The built-ins of section 3.6: `uuid()`, `now()`, `isoDate()`, `randomInt(min, max)`, `randomString(len)`, `base64Encode(str)`, `base64Decode(str)`, `urlEncode(str)`. Backed by `postino_core::functions`. |

Semantics:

- Values passed to `set` are converted to strings (`String(value)`).
- An uncaught exception in `pre` aborts the execution: the request is not sent and the error is
  shown. An uncaught exception in `post` is reported but the response is still shown.
- Limits: 32 MiB of memory and 5 seconds of wall time per script run, enforced with the QuickJS
  memory limit and an interrupt handler. Exceeding them is a script error, never a crash.
- A fresh runtime and context per script run. No state leaks between executions.
- Isolation: do not expose any module loader, `fetch`, timers, `std`/`os` modules or anything
  that touches the host. Only the globals above plus the standard ECMAScript built-ins.
- Implementation hint: write the JS-facing API (`expect`, `test`, header helpers) in an embedded
  `prelude.js` (`include_str!`) on top of a few Rust-backed native functions. Keep Rust to
  data exchange and limits.

### ScriptEngine trait (postino-script)

```rust
pub trait ScriptEngine: Send + Sync {
    fn run_pre(&self, script: &str, ctx: PreContext) -> Result<PreOutcome, ScriptError>;
    fn run_post(&self, script: &str, ctx: PostContext) -> Result<PostOutcome, ScriptError>;
}
```

`PreContext` carries the unresolved-but-editable request, vars and env snapshot. `PreOutcome`
returns the possibly modified request, new vars, env changes, console lines and test results.
`PostContext`/`PostOutcome` are analogous with the response. Exact field names are up to the
implementer, but all of them are plain owned data from `postino-core` (no rquickjs types in the
public API). `QuickJsEngine` is the only implementation. A `NoopEngine` for tests of other
crates is welcome.

## 5. Domain model (postino-core)

Minimum set of types. Use owned `String`s, derive `Debug, Clone, PartialEq, Eq` and `Default`
where sensible.

- `Method` (enum with the standard methods plus `Custom(String)`), with `FromStr`/`Display`.
- `KeyValue { key, value, enabled }`, used for headers, query and form.
- `Body` enum: `None`, `Json(String)`, `Text(String)`, `Xml(String)`, `Form(Vec<KeyValue>)`.
- `Request { method, url, headers, query, body, pre_script, post_script, docs }`.
- `Environment { name, variables: Vec<KeyValue-like ordered pairs> }`.
- `VarScope` or similar holding the three lookup layers of section 3.5, and
  `interpolate(&str, &VarScope) -> Interpolated { text, warnings: Vec<TemplateWarning> }`
  covering both variables and the functions of section 3.6.
- `functions` module with the built-ins of section 3.6, reusable from `postino-script`.
- `ResolvedRequest` (everything interpolated, only enabled entries, final URL with query
  appended), `Response { status, headers, body: Vec<u8>, time: Duration, size }`,
  `TestResult { name, passed, message }`, `ConsoleLine { level, text }`.

## 6. Phases

Phases 2, 4 and 5 only depend on phase 1 and may run in parallel. Everything else is sequential
in the order given.

### Phase 1. Workspace bootstrap and `postino-core`

- Create the workspace manifest and the seven crates (empty `lib.rs` for libraries,
  `main.rs` that prints nothing yet for the app, or a minimal gpui window if trivial).
- Implement the domain model of section 5, variable interpolation of section 3.5 and template
  functions of section 3.6.
- Tests: interpolation (simple, spaces inside braces, missing vars, layer precedence, no double
  expansion, unicode, adjacent vars, unmatched braces left untouched), every template function
  (format checks for random ones), arguments as literals and as variables, error cases left
  untouched with a warning, `Method` parse/display.

Acceptance: `make check`, `make lint`, `make test` pass; `cargo tree -d | grep gpui` empty.

### Phase 2. `postino-format`

- `postino::parse(&str) -> Result<Request, ParseError>` and `serialize(&Request) -> String`,
  exactly as section 3. `ParseError` carries line number and a readable message.
- `env::parse` / `env::serialize` for section 3.4.
- Fixtures in `crates/postino-format/tests/fixtures/*.postino` covering every section, and the
  round-trip tests of section 3.3. At least 25 test cases in total, including the error cases
  (missing request line, duplicated section, unknown section, bad body type).

Acceptance: all round-trip tests green, `make lint` clean.

### Phase 3. `postino-workspace`

- `Workspace::open(path)` scans a folder recursively: directories become collections, `.postino`
  files become requests, `environments/` is read as environments and skipped from the tree.
  Hidden folders and `target/` are ignored.
- Tree type with stable ids (relative paths) and natural sort, folders first.
- `load_request`, `save_request` (atomic write: temp file in the same dir then rename),
  `create_request`, `create_folder`, `rename`, `delete` (only inside the workspace root, reject
  paths that escape it), `list_environments`, `load_environment(name)` merging `.local.env`.
- File name sanitization helper shared with the Postman importer (Windows-reserved names and
  characters, trailing dots and spaces, max length 120).
- Tests with `tempfile`. Parse errors of individual files must not break the whole scan: they
  are collected and shown as broken entries.

Acceptance: tests green, including path traversal rejection and a scan with a broken file.

### Phase 4. `postino-script`

- `ScriptEngine` trait, `QuickJsEngine` and the API of section 4, with `prelude.js`.
- Tests: every `expect` matcher (pass and fail and `.not`), `test` isolation, `vars`/`env`
  round-trip, request mutation in pre, read-only in post, `res.json()`, console capture,
  `util` functions (format checks, not exact values), syntax errors with line info, runtime
  exceptions, infinite loop killed by timeout, memory bomb stopped by limit, absence of
  `fetch`/`require`/`import`/`setTimeout`, no state shared between two runs.

Acceptance: tests green, timeout test finishes under 10 seconds.

### Phase 5. `postino-http`

- `send(&ResolvedRequest, &SendOptions) -> Result<Response, HttpError>` with `ureq` 3 and rustls.
  `SendOptions`: timeout (default 30 s), follow redirects (default on, max 10), verify TLS
  (default on). Do not treat 4xx/5xx as errors: they are normal responses.
- Measure total time. Keep the body as bytes plus the content type.
- Tests against a local `tiny_http` server on `127.0.0.1:0`: each method, headers sent and
  received, JSON/text/form bodies, 404 and 500 returned as responses, timeout, redirect.

Acceptance: tests green, no test touches the internet.

### Phase 6. `postino-runner`

- `Runner::new(engine: Arc<dyn ScriptEngine>, options)` and
  `run(&Request, &Environment, &mut SessionEnv) -> RunResult`.
- Pipeline: build scope, run pre (skip if empty), apply request mutations, interpolate into a
  `ResolvedRequest`, send, run post (skip if empty), apply env changes to `SessionEnv`, collect
  tests, console lines and missing-variable warnings.
- `RunResult` must be complete enough for the UI to render everything without extra calls, and
  must report which stage failed if any.
- End-to-end tests with `tiny_http` and `QuickJsEngine`: token from a login response stored via
  `env.set` and used in the next request, pre script generating a header, failing tests
  reported, pre exception prevents sending.

Acceptance: tests green.

### Phase 7. Postman import (`postino-format::postman` and `postino-workspace`)

- Parse Postman Collection v2.1 JSON with serde (only the fields we map, ignore the rest).
  Mapping: folders to directories, requests to `.postino` files (name sanitized, duplicates get
  ` (2)`, ` (3)` suffixes), method, `url.raw` without query string plus `url.query` into
  `query`, headers with `disabled` into `#` entries, body modes `raw` (language json/xml/text,
  default text) and `urlencoded`. `formdata`, `file` and `graphql` bodies produce a warning and
  are imported as empty. Auth `bearer`, `basic` and `apikey` (header location) become headers,
  collection and folder level auth inherited by requests that use `inherit` or no auth.
  Postman `{{var}}` syntax is compatible and kept as is. `description` goes to `docs`.
- Scripts: Postman `prerequest` and `test` events are copied into `pre`/`post` fully commented
  out, with a first line `// Imported from Postman. The pm.* API is not supported, adapt it.`
- Collection `variable` array goes to `environments/<collection name>.env`.
- Postman environment export (`*.postman_environment.json`): `values` go to
  `environments/<name>.env`, entries of type `secret` go to `<name>.local.env` instead.
- Import returns a report (created files, warnings). It writes into a new folder named after
  the collection inside the workspace, never overwriting existing files.
- Tests with a realistic fixture collection (nested folders, all body modes, auth inheritance,
  disabled headers, scripts, variables) and an environment export.

Acceptance: tests green, importing the fixture produces files that parse back without errors.

### Phase 8. App shell (`postino-app`)

Read gpui-kit source before starting (see section 0). Structure the crate so logic stays out of
views:

```
postino-app/src/
├── main.rs            app init, theme, window, actions and key bindings
├── state/             plain Rust state, no gpui types where avoidable, unit-tested
│   ├── mod.rs         AppState: workspace, open tabs, active env, session env
│   ├── tabs.rs        open requests, dirty flag, active tab
│   └── ...
├── views/             gpui views, one file per panel
│   ├── root.rs        overall layout
│   ├── sidebar.rs     collection tree
│   ├── request_editor.rs
│   ├── response_view.rs
│   └── env_picker.rs
└── actions.rs         action definitions and handlers
```

Build in this phase:

- Window with title bar, resizable sidebar and main area (gpui-kit dock or resizable panels).
- Open a workspace folder (native dialog through gpui's `prompt_for_paths` or equivalent in the
  pinned version). Remember the last opened folder in the OS config dir
  (`dirs::config_dir()/postino/state.toml` or JSON; add `dirs` if needed).
- Sidebar tree from `postino-workspace`, broken files marked, context actions: new request, new
  folder, rename, delete (with confirmation inside the app, not a native blocking dialog).
- Tabs of open requests with dirty indicator. `Ctrl+S`/`Cmd+S` saves the active tab.
- Environment picker in the title bar, "No environment" option.
- Light and dark theme following the system, using the gpui-kit theme.
- Unit tests for everything in `state/`.

Acceptance: `make run` opens the window on Linux; opening a folder with fixtures shows the tree;
`make lint` and `make test` clean. Report must include a screenshot or a precise description of
what was verified manually, because UI cannot be fully tested automatically.

### Phase 9. Request editor and response viewer

- Request editor: method select, URL input, Send button (`Ctrl+Enter`), tabs Params, Headers,
  Body, Pre-request, Post-response, Docs. Params/Headers/Form as editable key-value tables with
  enable checkbox. Body type select plus the gpui-kit code editor with JSON/XML highlighting
  where available. Scripts in the code editor with JavaScript highlighting (enable only the
  gpui-kit tree-sitter features that are needed: json, javascript, and xml/html if present).
- Params tab and the URL query string stay in sync in one direction at least: editing the table
  updates the URL. Document the chosen behavior.
- Sending runs `postino-runner` on the background executor, shows a spinner, allows cancel
  (drop the result if cancelled).
- Response viewer: status with color by class, time, size, tabs Body (pretty printed JSON with
  highlighting, raw toggle), Headers, Tests (pass/fail list with messages), Console, plus a
  warnings strip for missing variables and script errors.
- Import menu entry: Postman collection and environment, showing the import report.
- Any edit marks the tab dirty; saving goes through `postino-format` serialize so files stay
  canonical.

Acceptance: manual end-to-end check against a local server (for example
`python3 -m http.server` or the `tiny_http` test helper): open fixture workspace, pick an
environment, send a request with pre and post scripts, see tests results; import the Postman
fixture and send one imported request. `make lint` and `make test` clean.

### Phase 10. Documentation

- Update `README.md`: remove "Scripting and test runners" from non-goals, describe the MVP as
  built, document the `.postino` format, the script API, environments and the recommended
  `.gitignore` lines (`target/`, `*.local.env`).
- Move the decisions of section 1 from "Pending decisions" to "Decisions taken" in `AGENTS.md`,
  and add a short "Architecture" section pointing to the crates. Keep the pending ones that are
  still open (license, final name, domain, docs language).
- Add `docs/format.md` (full spec from section 3) and `docs/scripting.md` (section 4 with
  examples).

Acceptance: docs match the code (every script global and every format rule documented exists).

## 7. Definition of done for the MVP

- All phases accepted.
- `make format-check`, `make lint` and `make test` pass from a clean `cargo clean`.
- `cargo build --release` works on Linux. Windows and macOS builds are not verified in this
  environment; note it in the final report.
