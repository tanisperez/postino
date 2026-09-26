# Postino UI redesign plan

Status: approved in the planning session of 2026-09-26. This document is the source of truth for
scope and approach of the UI redesign. It is executed phase by phase by Sonnet 5 subagents,
orchestrated by the main session (see section 4). A phase is done only when every acceptance
criterion passes.

The design lives in `postino_design_system/` (exported from Claude Design). Open the `.dc.html`
files in a browser to see them rendered, or read them as source: every size, color and spacing
is written inline in the markup, and the data behind each mockup is in the `renderVals()` script
at the bottom of each file.

| Design file | What it defines |
|---|---|
| `Postino Design System.dc.html` | Color tokens (light and dark), type scale, spacing, radii, elevation, the suggested gpui-kit `Theme` field for each token |
| `Components.dc.html` | Buttons, inputs, variable chips, segmented control, underline tabs, open-tab strip, method and status badges, checkbox, radio, switch, tree rows, menu, key-value table, inline messages |
| `Main A.dc.html` | **The chosen main screen** (flat panes, request above, response below) |
| `Main B.dc.html` | Rejected alternative. Do not implement. |
| `Settings.dc.html` | Settings modal, Appearance section |
| `Performance.dc.html` | Load test tab |
| `Current UI.dc.html` | Today's UI, for reference only |

## 0. Rules for whoever executes this plan

- Read `AGENTS.md` and `plans/mvp.md` section 0 first. Their rules apply unchanged: no em dash
  anywhere, `make format`, `make lint`, `make test`, max width 100, no `unwrap()`/`expect()`
  outside tests, doc comments on every public item, plain readable Rust, no `unsafe`.
- GPUI and gpui-kit evolve quickly and your memory of their API is probably wrong. Read the
  pinned sources in `~/.cargo/registry/src/*/gpui-component-0.6.6`, `gpui-base-0.6.6`,
  `gpui-kit-0.6.6`, `gpui-kit-assets-0.6.6` and `gpui-pre-0.3.6` before writing UI code. Read
  `plans/ui-redesign-spikes.md` (written in phase 0) before any UI phase.
- Icons: the full Lucide catalog (1830 icons) ships in `gpui-kit-assets` as
  `gpui_kit_assets::IconName`. Every icon the design uses (`gauge`, `settings`, `palette`,
  `git-branch`, `lock`, `hard-drive`, `folder`, `folder-plus`, `list-filter`, `copy`,
  `wand-sparkles`, `send-horizontal`, `save`, `code`, `variable`, `download`, `circle-x`,
  `ellipsis`, `chevrons-up-down`, ...) exists there. Do not add icon files.
- UI logic that can be plain Rust goes under `crates/postino-app/src/state/` with unit tests, as
  today. Views only render and forward input.
- Never hardcode a color in a view. Every color comes from the palette (phase 2). Every size
  comes from the constants in `theme/metrics.rs` (phase 2) unless it is a one-off in the design.
- Keep the existing debug hooks (`POSTINO_ENV`, `POSTINO_AUTOSEND`) working.
- Each phase ends with a short report: what was built, deviations from the plan and why, and the
  summary of `make format`, `make lint` and `make test`.

## 1. Decisions taken in the planning session

| Topic | Decision |
|---|---|
| Main screen | Direction A, "Flat panes". Direction B is discarded, including its error panel. |
| Error states | The `InlineMessage` component from `Components.dc.html` (warning, danger, success strips), with a "Define" action on unknown variables. No full-width red banner. |
| Performance | In scope, complete: new `postino-load` crate (engine, metrics, run history) plus the load test tab. |
| Extras shown in the mockups | All implemented: command palette (Ctrl K), workspace switcher with recent folders, sidebar filter, status bar, git branch in the sidebar footer, "Define" variable action, "Code" snippet generator. |
| Fonts | Geist and Geist Mono (SIL OFL 1.1), bundled in the binary, registered at startup. |
| Themes | Two themes (Postino Light, Postino Dark) built at startup from one Rust palette, loaded into gpui-kit's `ThemeRegistry`. "System" keeps using `Theme::sync_system_appearance`. |
| Settings file | `<config dir>/postino/settings.toml` (on Linux `~/.config/postino/settings.toml`), via the `toml` crate. Applied live, no Save button. |
| Recent workspaces | `<config dir>/postino/recent-workspaces.txt`, one absolute path per line, most recent first, max 10. Replaces `last-workspace.txt` (read once for migration, then ignored). |
| Load test history | `<workspace>/.postino/runs/<NNNN>.json`. The workspace scanner already skips hidden folders. |
| Out of scope | Auth tab and OAuth 2.0 (design plan item 06, no screen designed yet), Main B, the "Current UI" file. They become GitHub issues for a later milestone. |

## 2. Design reference, condensed

The executing agent must still open the design files. This section only fixes the decisions a
reader could otherwise interpret in different ways.

### 2.1 Tokens

Light and dark values are in `Postino Design System.dc.html` (the `tokens` list) and, complete,
in the inline `--bg:...` custom properties of `Postino Screens.dc.html` (the full set includes
`--success-subtle`, `--warning-subtle`, `--danger-subtle`, `--info-subtle`, `--m-get`,
`--m-post`, `--m-put`, `--m-patch`, `--m-delete`, `--syn-key`, `--syn-str`, `--syn-num`,
`--syn-bool`, `--accent-fg`, `--shadow`). Copy the values from there, do not retype them from
memory.

Method colors: GET `m-get`, POST `m-post`, PUT `m-put`, PATCH `m-patch`, DELETE `m-delete`,
HEAD, OPTIONS and custom methods `fg-muted`. Method badge background is the method color at 14%
alpha (the design uses `color-mix(... 14%, transparent)`).

Status colors: 2xx `success`, 3xx `info`, 4xx `warning`, 5xx `danger`, "Not sent" or a send
failure `fg-muted` on `hover`. The badge background is the matching `*-subtle` token.

Environment dot colors (the design colors `local` green, `staging` amber, `production` red):
derived from the environment name, case-insensitive. Contains `prod` -> `danger`; contains
`stag`, `test`, `qa` or `uat` -> `warning`; anything else -> `success`. "No environment" is a
hollow ring in `fg-subtle`.

### 2.2 Type and metrics

- UI font Geist, base 13 px (user-adjustable 11 to 16 in Settings). Mono font Geist Mono,
  12.5 px in editors (adjustable 10 to 18 in 0.5 steps).
- Type scale: 20/600 (run title), 15/600 (dialog titles), 13/500 (buttons, active tabs),
  13/400 (body), 12/400 (secondary: timings, sizes, hints), 11/600 uppercase with 0.06em
  tracking (section labels such as "COLLECTIONS"), mono 12.5/400 (code, URL, key-value cells),
  mono 10/600 (method labels in the tree and tabs).
- Heights: title bar 40, open-tabs bar 36, response tab bar 36, request inner tabs 32, URL bar
  and Send button 32, controls 30, tree rows 26, menu rows 28, status bar 24, segmented control
  inner items 22.
- Widths: sidebar 264 (resizable, keep today's 180..480 range), search trigger 360, window
  control buttons 40.
- Radii: 4 (chips, checkbox), 5 to 6 (segmented items, tree rows, badges), 7 to 8 (inputs,
  buttons, cards in editors), 10 (menus, dashboard cards), 14 (modal).
- Elevation: panels are flat (border only); inputs and cards use `raised` plus a border; menus,
  popovers and modals use `overlay` plus the shadow token. No shadows on fixed panels.

### 2.3 Main A layout, top to bottom

1. **Title bar** (40, `surface`, bottom border): "P" logo square (18, radius 5, `accent`), "Postino"
   (600), a 1x16 separator, workspace switcher (folder icon, workspace folder name, chevron), a
   centered 360-wide search trigger ("Search requests and actions", `Ctrl K` key hint), the
   environment pill (dot, name, chevron, on `raised` with border), "Import" ghost button, the
   settings gear, and the window controls (40x40 each).
2. **Sidebar** (`surface`, right border): header "COLLECTIONS" with "+" (new request) and
   "folder-plus" (new folder) icon buttons; a filter input (28 high, `list-filter` icon); the
   tree (26-high rows, chevron for folders, method label 34 wide for requests, selected row
   `accent-subtle` background with `accent-text` label, hover `hover`); footer with a top border:
   `hard-drive` icon, workspace path shortened with `~`, then `git-branch` icon and branch name
   (omitted when the workspace is not in a git repository).
3. **Open-tabs bar** (36, `surface`): each tab shows an optional icon (`gauge` for load tests),
   the method label, the name, and at the end a filled 8 px circle when dirty or an `x` when
   clean (hovering a dirty tab shows the `x`). Active tab: `bg` background, 2 px `accent` line on
   top, `fg` text; inactive: transparent, `fg-muted`. A "+" button after the last tab opens the
   new request flow.
4. **Request pane** (flex 1.1): URL bar (method selector and URL joined in one bordered `raised`
   box, method in its color, variables in the URL shown as chips) plus the primary Send button with
   its `Ctrl ↵` hint (`Cmd ↵` on macOS). Below, underline tabs Params, Headers, Body, Pre-request,
   Post-response, Docs with counts in `fg-subtle`. Body tab: segmented control JSON, Text, XML,
   Form, None on the left and a "Format" ghost button (`wand-sparkles`) on the right, then the
   code editor in a bordered `raised` box with line numbers.
5. **Response pane** (flex 1): tab bar Body, Headers (count), Tests (`passed/total`, colored
   `success` when all pass, `danger` otherwise), Console (count); on the right the status badge,
   time and size. Below, a Pretty/Raw segmented control on the left and `copy` and `search` icon
   buttons on the right, then the body with line numbers.
6. **Status bar** (24, `surface`, top border, 11.5 px `fg-subtle`): `lock` icon and "Local only",
   the active file id, a spacer, "Unsaved changes" when the active tab is dirty, "UTF-8".

## 3. Architecture changes

```
postino-app ──> postino-load ─────> postino-runner ──> postino-script ──> postino-core
     │               (new)                 └─────────> postino-http ────> postino-core
     ├──────> postino-runner
     └──────> postino-workspace ─> postino-format ─> postino-core
```

- New crate `postino-load`: the load test engine, metrics aggregation and run history
  persistence. It does its own file IO for `.postino/runs/` (a documented exception to
  "`postino-workspace` owns the filesystem", to avoid a `workspace -> load -> runner` edge).
- `postino-runner` gains `preview` (resolve without scripts or network).
- `postino-format` gains `snippet` (curl, fetch, Python requests).
- `postino-workspace` gains environment variable writes and git branch detection.
- `postino-app` gains `theme/` (palette, metrics, fonts, theme JSON), `views/components/`
  (reusable styled components), new views (settings, palette, snippet dialog, define dialog,
  load test tab) and new state modules.

New dependencies (workspace level): `toml` (settings file). Nothing else is expected. If a phase
needs another crate it must justify it in its report.

## 4. Execution model

The main session (Opus) orchestrates. Every phase is delegated to a subagent started with the
`Agent` tool, `model: "sonnet"`, `subagent_type: "general-purpose"`. The orchestrator never
implements a phase itself; it prepares, reviews, integrates and verifies.

### 4.1 Before phase 0

1. Work lands directly on `main` (decided by the owner: no feature branch for this plan).
2. Create the GitHub milestone `0.1.0` and one issue per phase (1a to 1d, 2 to 8, 9), titled
   `UI redesign: <phase name>`, body = the phase's goal and acceptance criteria copied from this
   plan. Create one extra issue per out-of-scope item (Auth tab, OAuth 2.0) in no milestone.

### 4.2 Order and parallelism

```
Phase 0 (spikes) ──> Phase 2 (foundations) ──> 3 ──> 4 ──> 5 ──> 6 ──> 7 ──> 8 ──> 9
Phase 1a, 1b, 1c, 1d (pure logic, parallel, worktrees) ─────────┘ (all merged before 5)
```

- Phase 0 runs in the main checkout.
- Phases 1a, 1b, 1c and 1d need nothing from phase 0, so they start at the same time, in parallel, each with
  `isolation: "worktree"`. They touch disjoint crates and modules and need no UI. Each commits
  on its worktree branch; the orchestrator reviews and merges into `main`
  (fast-forward or merge commit, resolving `Cargo.toml`/`Cargo.lock` conflicts by hand).
- Phases 2 to 8 run sequentially in the main checkout. Phase 2 may start while 1a to 1d are
  still running. Phase 5 needs 1a merged; phase 6 needs 1c; phase 7 needs 1a, 1b and 1c; phase
  8 needs 1d.
- At most one UI phase runs at a time: they all touch `views/root.rs`.

### 4.3 What the orchestrator gives each subagent

The prompt for each phase contains: the path of this plan and the phase number to execute, the
instruction to read section 0 and 2 plus `plans/ui-redesign-spikes.md`, the list of files the
phase is expected to touch, the acceptance criteria verbatim, and the report format. Subagents do
not commit; the orchestrator commits after review (worktree agents in 1a to 1d do commit on
their own branch, since the orchestrator merges branches).

### 4.4 What the orchestrator does after each phase

1. Read the full diff. Check it against the phase scope (no unrelated changes, no hardcoded
   colors, no em dash: `grep -rnP '\x{2014}' crates docs plans` must be empty).
2. Run `make format-check`, `make lint`, `make test`.
3. For UI phases, visual check: run the app on `examples/sample-workspace` (use the debug hooks
   of section 5, `POSTINO_OPEN`), take a screenshot (`grim` on Wayland, `import -window root` on
   X11) and compare it with the design file rendered in the browser, in both light and dark.
   List the differences; small ones go back to the same subagent via `SendMessage`, the rest
   become notes for the user.
4. Commit on `main` with an English message, close the phase issue with a comment
   pointing to the commit.
5. **User checkpoints**: stop and show screenshots to the user after phases 2, 5, 6 and 8 before
   continuing.
6. Never `git push` without the user's explicit permission.

## 5. Phases

### Phase 0. Spikes: verify the risky gpui-kit APIs

Goal: settle, with throwaway code, every API question the later phases depend on. Output is a
notes file, not product code.

Questions to answer, each with a minimal working snippet and file/line references in the gpui
sources:

1. Themes: build a theme family JSON string in Rust, load it with
   `ThemeRegistry::global_mut(cx).load_themes_from_str`, make it the light and dark theme used by
   `Theme::change` and `Theme::sync_system_appearance`. List every `ThemeConfigColors` field name
   that maps to the tokens of section 2.1 (the "gpui" column of the design is only a
   suggestion). Find how syntax highlight colors are set (`highlight` in `ThemeConfig`,
   `HighlightThemeStyle`) and which capture names JSON, JavaScript and HTML use.
2. Fonts: register bundled TTFs with `cx.text_system().add_fonts(...)` and set
   `Theme::font_family`, `font_size`, `mono_font_family`, `mono_font_size`. Confirm the change is
   picked up by `Input` code editors and plain text, and how to list installed font families
   (`all_font_names` or equivalent) for the Settings font pickers.
3. Variable chips in the URL: can a single-line `Input` show styled ranges (background plus
   foreground for `{{var}}`, wavy danger underline for undefined ones)? Candidates: the
   highlighter with a custom language, diagnostics (`DiagnosticSeverity`), or a custom element.
   If none works in a single-line input, the fallback is: render a styled read-only line of
   spans when the URL is not focused, swap to the plain `Input` on focus. Pick one and justify.
4. Same question for `{{var}}` inside the body code editor (nice to have; acceptable answer is
   "not feasible, keep plain highlighting").
5. Command palette: is `gpui_component::command` usable for a Ctrl K palette (fuzzy input, list
   of items with icon, label, hint, keyboard navigation)? If not, what to build it from
   (`dialog` plus `list`).
6. Modal dialogs: how to open a custom-content modal of a fixed size (Settings is 800x720 with
   its own header, sidebar and footer) with the overlay dimming behind it, and close it with
   Escape and a close button.
7. Charts: can `chart::LineChart` draw two series with independent vertical scales and no axes
   (Throughput and latency card)? If not, confirm a custom `canvas` polyline is simple enough.
   The histogram and status bars are plain divs.
8. Periodic refresh: the idiomatic way to re-render a view every 250 ms while a background task
   runs (`cx.spawn` loop with `Timer::after`, or equivalent in gpui-pre 0.3.6).
9. Custom title bar content with the window controls at the right on Linux and Windows and the
   traffic lights on macOS, keeping `WindowDecorations::Client`.
10. A way to add a debug hook `POSTINO_OPEN=<target>` to open a specific UI state at startup
    (settings, palette, a load test tab) so the orchestrator can screenshot it.

Files: `plans/ui-redesign-spikes.md` (new). Throwaway code goes in the scratchpad or is reverted.

Acceptance criteria:
- [ ] `plans/ui-redesign-spikes.md` answers all ten questions, each with a code snippet that was
      actually compiled and run, or an explicit "not possible, because ..." with the fallback.
- [ ] The working tree has no leftover spike code (`git status` shows only the notes file).

### Phase 1a. Runner preview

Goal: resolve a request against the current variables without running scripts or sending, so the
UI can underline unknown variables live and the snippet generator has a resolved request.

- Add `postino_runner::preview(request: &Request, environment: &Environment, session_env:
  &SessionEnv) -> Preview`, `Preview { resolved: ResolvedRequest, warnings: Vec<TemplateWarning>,
  unknown_variables: Vec<UnknownVariable> }`.
- `UnknownVariable { name: String, location: VariableLocation }`, `VariableLocation` is `Url`,
  `Query`, `Header(String)`, `Body`. Deduplicated by (name, location), in order of appearance.
  Template functions (`{{ uuid() }}`) are never unknown variables.
- Add `postino_core::variable_spans(text: &str) -> Vec<VariableSpan>` returning the byte range,
  name and kind (variable or function call) of every `{{ }}` marker, for the UI to style chips.
  Reuse the existing interpolation parser, do not write a second one.

Files: `crates/postino-runner/src/preview.rs`, `crates/postino-runner/src/lib.rs`,
`crates/postino-core/src/interpolate.rs` (or a new `spans.rs`), tests next to them.

Acceptance criteria:
- [ ] Unit tests: defined variable resolves; undefined in URL, query, header and body is reported
      with the right location; function calls are not reported; session overrides win over the
      environment; spans have correct byte ranges with multi-byte UTF-8 text around them.
- [ ] `preview` never runs a script and never opens a socket (test with a request whose pre
      script would throw: preview still succeeds).

### Phase 1b. Code snippets

Goal: turn a resolved request into copyable code.

- `postino_format::snippet::{SnippetLanguage, render_snippet}`. Languages: `Curl`, `JsFetch`,
  `PythonRequests`. Input is a `ResolvedRequest`. Unresolved `{{var}}` markers are kept literally.
- curl: one flag per line with ` \` continuations, `-X` omitted for GET, body with
  `--data-raw`, single quotes escaped correctly (`'\''`). fetch: `await fetch(url, {...})` with
  `method`, `headers`, `body`. Python: `requests.request(...)`.
- Form bodies: curl `--data-urlencode` per field, fetch `new URLSearchParams(...)`, Python
  `data={...}`.

Files: `crates/postino-format/src/snippet.rs`, `crates/postino-format/src/lib.rs`, tests.

Acceptance criteria:
- [ ] Snapshot-style tests (plain `assert_eq!` against expected strings) for GET, POST JSON,
      form body, headers with quotes, a body containing a single quote, custom method.

### Phase 1c. Workspace and app state additions

Goal: all non-UI state the new UI needs, unit tested.

1. `postino-workspace`:
   - `Workspace::set_environment_var(env: &str, key: &str, value: &str, local: bool)` writes to
     `environments/<env>.env` or `<env>.local.env`, creating the file (and the folder) if missing,
     replacing the key if present, preserving the order and comments of other lines (use
     `postino-format`'s env parser/serializer; extend it if it drops comments).
   - `Workspace::create_environment(name)`.
   - `git_branch(root: &Path) -> Option<String>`: walk up from `root` to find `.git`, read
     `HEAD`, return the branch name, or the short commit hash when detached. No git dependency;
     handle `.git` being a file (worktrees: `gitdir: <path>`).
2. `postino-app/src/state/`:
   - `settings.rs`: `Settings { theme: ThemeChoice (System, Light, Dark), ui_font: String,
     ui_font_size: f32, mono_font: String, mono_font_size: f32 }` with serde, `Default` (System,
     "Geist", 13.0, "Geist Mono", 12.5), clamping (11..=16 step 1, 10..=18 step 0.5), load and
     save to `<config dir>/postino/settings.toml`. A missing or invalid file yields defaults
     (invalid fields fall back individually, unknown fields ignored). Same testable split as
     `config.rs` (`read_*`/`write_*` taking a base dir).
   - `config.rs`: replace the single last workspace with a recent list (section 1). Migrate from
     `last-workspace.txt` when the new file does not exist. Keep `load_last_workspace` working
     (first entry that still exists on disk).
   - `palette.rs` (command palette model): `PaletteItem { kind: Request(id) | Action(ActionId) |
     Environment(Option<String>), label, detail, shortcut }` and `fuzzy_filter(query, items) ->
     Vec<(usize, score, matched_char_indices)>`, subsequence matching, case-insensitive, scoring
     word starts and contiguous runs higher. `ActionId`: Send, Save, NewRequest, NewFolder,
     ImportCollection, ImportEnvironment, OpenSettings, OpenWorkspace, NewLoadTest,
     ToggleTheme.
   - `env_color.rs`: environment name -> `EnvColor` (Success, Warning, Danger, None) per 2.1.
   - `format.rs` helpers: human size (`1.2 KB`), duration (`142 ms`, `1.4 s`), path shortening
     with `~`, relative day ("today", "yesterday", "3 days ago") from unix seconds.
3. Add `toml` to `[workspace.dependencies]` and `postino-app`.

Files: the modules above, `crates/postino-workspace/src/{workspace.rs,git.rs}`,
`crates/postino-format/src/env*.rs` if needed, `Cargo.toml` files.

Acceptance criteria:
- [ ] Unit tests for every item above, including: env file comments preserved on write, `.local`
      variant, git branch in a temp repo layout (plain files, no git binary), detached HEAD,
      settings round trip and clamping, invalid TOML, recent list ordering, dedup, max 10,
      migration, fuzzy ranking ("lgn" ranks "auth/login" above "legal notice"), env colors.

### Phase 1d. `postino-load` engine

Goal: a load test engine that runs a request or a collection with N virtual users and reports
live metrics, plus run history. Pure Rust, tested against the local `tiny_http` server.

1. Config: `LoadConfig { targets: Vec<LoadTarget>, vus: u32 (1..=500), duration: Duration,
   ramp_up: Duration, think_time: Duration, stop_on_error_rate: Option<f64> }`, `LoadTarget {
   id: String, request: Request }`. A single request is one target; a collection is every request
   of a folder in tree order.
2. Execution: `LoadRun::start(config, environment, session_env, runner: Arc<Runner>) -> LoadRun`.
   One OS thread per VU (ureq is blocking), started linearly over `ramp_up`. Each VU clones the
   session env once and loops over the targets in order (so a login token set by a post script
   flows to the next request of the same VU), sleeps `think_time` after each iteration. Stops at
   `duration`, on `LoadRun::stop()`, or when `stop_on_error_rate` is exceeded after at least 50
   samples. `LoadRun::is_finished()`, `LoadRun::join() -> LoadSummary`.
3. A sample is an error when there was no response (send failure, timeout, pre script failure) or
   the status is >= 400. Status keys: `Code(u16)`, `Timeout`, `Failed`.
4. Metrics: shared aggregator behind `Arc<Mutex<_>>`, latencies stored as `u32` microseconds.
   `LoadRun::snapshot() -> LoadSnapshot { elapsed, active_vus, total, rps (last complete
   second), p50, p95, p99, error_rate, series: Vec<SecondPoint { rps, p95 }>, histogram: [u64;
   26] (10 ms buckets, last one is 250 ms and above), status_counts: Vec<(StatusKey, u64)>,
   per_target: Vec<TargetStats { count, p50, p95, p99, error_rate }> }`. Percentiles use nearest
   rank on a sorted copy; document the cost.
5. History: `RunRecord { number, started_at_unix, target_label, config summary, final snapshot }`
   serialized with serde_json to `<workspace>/.postino/runs/<NNNN>.json` (4-digit, zero padded).
   `history::save(root, record)`, `history::list(root) -> Vec<RunRecordHeader>` (newest first),
   `history::load(root, number)`, `history::next_number(root)`. Corrupt files are skipped.
6. `compare(previous: &LoadSnapshot, current: &LoadSnapshot) -> Vec<Delta>` for Requests/s,
   p95, p99, Errors, with the sign convention of the design (higher rps is good, lower latency
   and errors are good; errors delta in percentage points "pt").

Files: `crates/postino-load/{Cargo.toml,src/*.rs}`, workspace `Cargo.toml` members.

Acceptance criteria:
- [ ] Tests (each under 3 s): a 1 s run with 4 VUs against the local server produces samples for
      every target and a 200 status count; ramp-up starts fewer VUs early than late; `stop()`
      ends the run promptly (< 500 ms); a server returning 500 with `stop_on_error_rate` stops the
      run early; post script env changes flow between targets of the same VU; percentile and
      histogram bucketing unit tests with fixed inputs; history round trip, numbering and corrupt
      file skipping in a temp dir; compare signs.
- [ ] No test touches the internet.

### Phase 2. Foundations: palette, metrics, fonts, themes

Goal: the app renders with the Postino tokens and fonts; nothing else changes visually on
purpose yet.

1. `crates/postino-app/assets/fonts/`: Geist (Regular, Medium, SemiBold, Bold) and Geist Mono
   (Regular, Medium, SemiBold) TTF files plus `OFL.txt`. Download them from the official Vercel
   `geist-font` GitHub release; record the version and source URL in `assets/fonts/README.md`.
   Embed them with `include_bytes!` and register them at startup (phase 0, question 2).
2. `src/theme/palette.rs`: `Palette` struct with one `Hsla` field per token of section 2.1, and
   `Palette::light()`, `Palette::dark()`. Helpers `method_color(&Method)`,
   `method_badge_bg(&Method)`, `status_colors(Option<u16>) -> (fg, bg)`,
   `env_color(EnvColor)`. Access from views through a small extension trait
   (`cx.palette()`) that picks light or dark from `cx.theme().is_dark()`.
3. `src/theme/metrics.rs`: named constants for the heights, widths and radii of section 2.2.
4. `src/theme/mod.rs`: build the gpui-kit theme family JSON (Postino Light and Postino Dark) from
   `Palette` with `serde_json::json!`, including fonts, sizes, radius and the syntax highlight
   colors (`syn-*`, punctuation `fg-muted`), load it into `ThemeRegistry` and make it the default
   light and dark theme. The palette is the single source of truth: no hand-written theme JSON
   file.
5. `main.rs`: register fonts and themes after `gpui_kit::init`, load `Settings` (phase 1c, if
   merged; otherwise defaults and wire it in phase 6) and apply theme mode and font sizes.

Acceptance criteria:
- [ ] Test: the generated JSON parses with `ThemeRegistry::load_themes_from_str` (or the
      `ThemeSet` schema type directly) and contains both modes; `Palette::light()` values match a
      handful of hex values copied from the design (bg, accent, m-post, syn-key).
- [ ] The app starts, uses Geist everywhere and Geist Mono in editors, and switches light/dark
      with the OS setting. Screenshot in both modes.
- [ ] `grep -rn 'rgb(\|hsla(\|#[0-9a-fA-F]\{6\}' crates/postino-app/src/views` shows no new color
      literals.

User checkpoint after this phase.

### Phase 3. Components

Goal: one place for every visual decision. `src/views/components/`, one file per component, each
a `RenderOnce` struct with builder methods, reading colors only from the palette.

| Component | Design reference | Notes |
|---|---|---|
| `MethodBadge` | Components "Methods", tree rows | Two variants: `label` (mono 10/600, colored text, fixed width 34) and `pill` (with 14% background). |
| `StatusBadge` | Components "Status", Main A response bar | Dot plus text, 22 high, `*-subtle` background. Variants from status code, "Not sent", "Sending..." (with spinner). |
| `IconButton` | Components buttons (28 and 24 squares) | Ghost, sizes 24 and 28, optional tooltip. |
| `PrimaryButton` / `SecondaryButton` / `GhostButton` / `DangerButton` | Components "Buttons" | Heights 24/28/32, optional key hint (mono 10.5, 75% opacity), disabled at 45% opacity. Wrap gpui-kit `Button` if it can be styled to match, otherwise a styled div with click handling and focus. |
| `SegmentedControl` | Components inputs, body types, Pretty/Raw | `surface` track with border, selected item `raised` with a 1 px shadow. |
| `UnderlineTabs` | Components "Tabs" | 2 px `accent` inset underline on the active tab, optional count in `fg-subtle` or a custom color. |
| `DocumentTabs` | Components open-tab strip | Section 2.3 point 3 behavior, including dirty dot and hover `x`. |
| `VariableChip` | Components inputs | `accent-subtle` background, `accent-text`, radius 4; undefined variant: `danger` text with a wavy underline (or the phase 0 fallback). |
| `UrlBar` | Main A | Method selector (dropdown menu of standard methods plus "Custom...") joined to the URL field, chip rendering per phase 0 answer 3. |
| `KeyValueTable` | Components "Key-value table" | Header row on `surface`, checkbox column 30, key 1fr, value 1.4fr, delete 30, disabled rows `fg-subtle` with strikethrough key, a trailing "Add" row. Mono 12.5 cells. |
| `InlineMessage` | Components "Inline messages" | Kinds warning, danger, success, info; icon, text, optional action link on the right. |
| `EnvPill` and `EnvMenu` | Main A title bar, Components "Menu" | Menu rows 28, check on the active item, colored dots, shortcut hint on the right. |
| `SectionLabel` | "COLLECTIONS" | 11/600 uppercase, 0.06em tracking. |
| `Switch`, `Checkbox`, `Radio` | Components "Controls" | Wrap gpui-kit ones restyled through the theme if possible; custom only if needed. |
| `Card` | Performance cards | `raised`, border, radius 10. |

Add a hidden debug view `POSTINO_OPEN=components` that renders every component in all states,
mirroring `Components.dc.html`, for visual comparison. It is not reachable from any menu.

Acceptance criteria:
- [ ] Every component in the table exists, is documented, and appears in the components debug
      view in all states shown in the design.
- [ ] Screenshot of the debug view in light and dark next to `Components.dc.html` shows no
      differences in color, size or spacing beyond font rendering.
- [ ] Plain-logic parts (for example which status maps to which color) are unit tested in
      `state/` or `theme/`.

### Phase 4. Main screen shell

Goal: Main A's frame: title bar, sidebar, open-tabs bar and status bar, with all their behavior.

1. Title bar per section 2.3 point 1. The workspace switcher opens a menu: recent workspaces
   (name plus shortened path, the current one checked), a separator, "Open folder...". The
   search trigger opens the command palette (built in phase 7; until then it is inert with a
   `TODO(phase 7)` comment, the only allowed TODO). The environment pill uses `EnvPill`/`EnvMenu`
   with `Ctrl 1..9` shortcuts (`Cmd` on macOS) for the first nine environments and `Ctrl 0` for
   "No environment". Import keeps today's menu, restyled. The gear opens Settings (phase 6;
   inert until then, same TODO rule).
2. Sidebar per section 2.3 point 2. The filter narrows the tree to requests whose name or path
   matches (case-insensitive substring), keeping their parent folders expanded; empty filter
   restores the previous expansion state. Filter logic in `state/`, tested. Existing tree
   actions (context menu: rename, delete, new request, new folder) keep working and are
   restyled with the menu look.
3. Open-tabs bar per section 2.3 point 3, with `DocumentTabs`. Tab label is the request name (the
   file stem), not the full id; the full id goes in a tooltip.
4. Status bar per section 2.3 point 6.
5. Replace the red `workspace_error` banner with an `InlineMessage` (danger) at the top of the
   main area, dismissible.

Files: `views/root.rs`, `views/sidebar.rs`, `views/env_picker.rs`, `views/import_menu.rs`, new
`views/title_bar.rs`, `views/status_bar.rs`, `state/` additions.

Acceptance criteria:
- [ ] Screenshot on `examples/sample-workspace` matches `Main A.dc.html` chrome (title bar,
      sidebar, tabs, status bar) in light and dark.
- [ ] Switching workspace from the menu works and updates the recent list; filtering works;
      environment shortcuts work; git branch shows for this repo and hides for a folder outside
      any repo.
- [ ] Tests for the new state logic.

### Phase 5. Request editor and response viewer

Goal: Main A's two panes, including error and warning states.

1. Request pane per section 2.3 point 4, with `UrlBar`, `UnderlineTabs` (counts: enabled params,
   enabled headers), `SegmentedControl` for body type, "Format" (pretty-prints JSON bodies;
   disabled for other types), the code editor boxed in `raised` with line numbers, and
   `KeyValueTable` for Params, Headers and Form bodies. Keep the existing `v_resizable` split,
   with a 1.1 to 1 initial ratio.
2. Live variable feedback: on every URL, query, header or body edit, call
   `postino_runner::preview` (phase 1a) with the active environment and session env; chips in the
   URL are accent when defined and danger when unknown. Debounce is not needed unless profiling
   shows a problem.
3. Response pane per section 2.3 point 5. Copy copies the body shown (pretty or raw). Search opens
   the editor's find bar if the read-only editor supports it (phase 0), otherwise hide the icon
   and note it.
4. Error and warning states, above the response body, using `InlineMessage`:
   - Unknown variables (from the preview, before sending and after): one warning row per
     variable, "Unknown variable `name`" with a "Define" action (phase 7 wires the dialog; until
     then inert with the allowed TODO).
   - Send failure: danger message "Sending failed: <reason>", status badge "Not sent".
   - Pre or post script failure: danger message with the stage and error.
   - Tests: success strip "N of N tests passed" or danger "M of N tests failed" in the Tests tab.
5. Empty states: no tab open ("Open a request from the sidebar", 13/400 `fg-muted`, centered);
   not sent yet (response pane shows "Send the request to see the response" and the key hint).

Files: `views/request_editor.rs`, `views/response_view.rs`, `views/send.rs`, `state/` as needed.

Acceptance criteria:
- [ ] Screenshot with `POSTINO_ENV=local POSTINO_AUTOSEND=auth/login.postino` (or the closest
      request in the sample workspace) matches `Main A.dc.html` in light and dark.
- [ ] Screenshot of the same request without environment shows the unknown variable warnings
      and danger chips.
- [ ] Every existing editing capability still works: method change, custom method, URL, params,
      headers, all body types, pre/post scripts, docs, save with `Ctrl S`, send with `Ctrl ↵`.
- [ ] Existing tests still pass; new state logic is tested.

User checkpoint after this phase.

### Phase 6. Settings

Goal: the Settings modal of `Settings.dc.html`, applied live and persisted.

1. Opened from the title bar gear, from the palette (phase 7) and with `Ctrl ,` (`Cmd ,` on
   macOS). Closed with the `x`, Escape, or a click on the dimmed backdrop.
2. Layout: 800x720, radius 14, `overlay` background, shadow token; left nav 188 wide on
   `surface` with "Settings" title and the single "Appearance" item (`palette` icon, selected
   style), and at the bottom "Saved to" plus the real settings path (shortened with `~`); right
   side header (52, title "Appearance", close button), content, footer (52, "Changes apply
   immediately.", "Reset to defaults" ghost button).
3. Theme: three cards (System, Light, Dark) with the mini previews drawn from palette colors as
   in the design (System card is half light, half dark), accent ring and radio on the selected
   one. System follows the OS live (today's `observe_window_appearance` behavior); Light and Dark
   stop following it.
4. Interface: font select ("Geist (bundled)" first, then installed families), size stepper
   (`-` / `N px` / `+`, 11..16).
5. Editor: monospace font select ("Geist Mono (bundled)" first, then installed families), size
   stepper (10..18, step 0.5), live preview line of syntax-highlighted JSON using the chosen
   font and size.
6. Every change updates the global `Theme` and refreshes the window immediately, and writes
   `settings.toml`. "Reset to defaults" restores `Settings::default()`.

Files: new `views/settings.rs`, `main.rs`, `views/root.rs`, `actions.rs`.

Acceptance criteria:
- [ ] Screenshot matches `Settings.dc.html` in light and dark.
- [ ] Each control changes the UI immediately; restarting the app restores the saved values.
- [ ] Deleting or corrupting `settings.toml` starts the app with defaults and no error.

User checkpoint after this phase.

### Phase 7. Command palette, Code snippets, Define variable

Goal: the three extras that remain.

1. Command palette (`Ctrl K` / `Cmd K`, and the title bar trigger): a centered overlay with a
   search input and a list of `PaletteItem`s (phase 1c), grouped as Requests, Environments,
   Actions, filtered with `fuzzy_filter`, matched characters highlighted in `accent-text`,
   arrow keys plus Enter, Escape closes. Items show their shortcut hint on the right. Executing
   an item performs it (open request, switch environment, run action).
2. Code: a "Code" ghost button (`code` icon) at the right of the request inner tabs row opens a
   dialog with a `SegmentedControl` (cURL, fetch, Python), a read-only mono code view of
   `render_snippet(preview(...).resolved)`, and a "Copy" button. Unknown variables stay as
   `{{name}}`.
3. Define variable: the "Define" action (response pane warnings, and a click on a danger chip in
   the URL) opens a small dialog: variable name (prefilled, editable), value, environment select
   (existing environments plus "New environment..."), and a "Store in .local.env (not versioned)"
   switch, on by default when the name contains `token`, `secret`, `password` or `key`. Save
   writes with `set_environment_var`, reloads the environment, and re-runs the preview so the
   chip turns accent. If no environment was active, the chosen one becomes active.

Files: new `views/command_palette.rs`, `views/snippet_dialog.rs`, `views/define_variable.rs`,
`views/root.rs`, `actions.rs`, `main.rs` (key bindings).

Acceptance criteria:
- [ ] Each action listed in `ActionId` is reachable and works from the palette.
- [ ] Snippets for the sample workspace requests run correctly when pasted in a shell (check the
      curl one manually against a local server, for example `python3 -m http.server`).
- [ ] Defining a variable writes the right file (check with `git diff` on a copy of the sample
      workspace) and clears the warning.
- [ ] All inert TODOs from phases 4 and 5 are gone (`grep -rn 'TODO(phase' crates` is empty).

### Phase 8. Load test tab

Goal: `Performance.dc.html`, backed by `postino-load` (phase 1d).

1. Tabs become heterogeneous: in `state/tabs.rs` an open tab is `TabKind::Request(..)` or
   `TabKind::LoadTest(LoadTestTab)`. Dirty state and saving only apply to requests. Tests for the
   new behavior (open, activate, close, a request rename not touching load test tabs).
2. Opening: "New load test" from the palette, from a request's or folder's sidebar context menu
   ("Load test..."), which preselects Request or Collection target. Tab label
   `Load test · <name>` with the `gauge` icon.
3. Left config panel (280 wide, right border, padding 16): Target segmented control (Request /
   Collection) plus a target picker showing the name and "N requests"; Virtual users (VUs),
   Duration (s), Ramp-up (s, hint "0 → N VUs"), Think time (ms, hint "per iteration") as numeric
   inputs with a unit suffix; "Stop on errors" switch with "When error rate > 5%"; at the bottom
   the primary "Start run" button, which becomes the danger-text "Stop run" secondary button while
   running, and the note "Runs on this machine. Results are saved to `.postino/runs/`".
4. Right dashboard (padding 16/20, gap 16):
   - Header: "Run #N" (16/600, as in `Performance.dc.html`), state badge
     (Running in accent, Finished in success, Stopped in warning, Failed in danger), progress bar,
     `mm:ss / mm:ss` elapsed.
   - KPI strip: Requests/s, p50, p95, p99, Errors (warning color when > 0), Total, 22/600 tabular
     numbers.
   - Throughput and latency card: two polylines (Requests/s in `accent-text`, p95 in `warning`),
     each normalized to its own max, four horizontal grid lines, time labels 0 to duration.
   - Latency distribution: 26 bars from the histogram, `accent-text`, bars at 110 ms and above in
     `warning`; axis labels 0, 50, 100, 150, 200, 250+ ms.
   - Responses by status: one row per status key with a bar and a percentage.
   - Per request table: method label, name, Count, p50, p95, p99, Errors (warning when above 1%).
   - Compare with: a dropdown of previous runs of the same target ("Run #11 · yesterday"),
     default the latest previous one; rows Requests/s, p95, p99, Errors with previous, current and
     colored delta. Hidden when there is no previous run.
   - Before the first run: the dashboard shows the empty state "Configure the run and press
     Start", and the history list of previous runs for this target if any (click to view).
5. Live update every 250 ms while running (phase 0, question 8). Run on background threads; the
   UI never blocks. On finish, save the `RunRecord` and refresh the compare list.
6. Closing a tab with a running test stops it.

Files: new `views/load_test/` (config panel, dashboard, charts), `state/tabs.rs`,
`state/load_test.rs`, `views/root.rs`, `views/sidebar.rs`, `postino-app/Cargo.toml`.

Acceptance criteria:
- [ ] A 60 s, 50 VU run against a local server (for example a tiny local Python server) shows
      live metrics and ends with a saved `.postino/runs/0001.json`; a second run shows the
      comparison.
- [ ] Screenshot during a run matches `Performance.dc.html` in light and dark.
- [ ] Stop works mid-run; closing the tab stops the run; the app stays responsive at 50 VUs.
- [ ] Tests for the tab kind changes and any new state logic.

User checkpoint after this phase.

### Phase 9. Docs and wrap-up

1. `AGENTS.md`: architecture graph and crate list with `postino-load`, the `.postino/runs`
   exception, the theme and components rules (no color literals in views, palette is the single
   source), the fonts note, the new debug hook `POSTINO_OPEN`.
2. `README.md`: features (load testing, command palette, settings), screenshots in light and dark
   (saved under `docs/screenshots/`).
3. `docs/load-testing.md`: what a VU does, error definition, percentile method, history format,
   suggestion to add `.postino/` to `.gitignore` if runs should not be versioned.
4. Remove `TODO`s, dead code left by the redesign (only what this plan made dead), and the
   `last-workspace.txt` migration note if it is no longer needed (keep the migration code).

Acceptance criteria:
- [ ] `make format-check`, `make lint`, `make test` pass on `main`.
- [ ] No em dash in the repository outside `postino_design_system/`.
- [ ] Every issue in milestone `0.1.0` is closed with a reference to its commit.
- [ ] The orchestrator asks the user before pushing.
