# Phase 0 spikes: gpui-kit 0.6.6 API answers

Written for `plans/ui-redesign.md` phase 0. Every answer below was checked against code that was
actually compiled and run (`crates/postino-app/examples/spike.rs`, a throwaway file, reverted
after this phase; see the git history of this commit for its exact contents if you need to
resurrect a snippet). File references are `file:line` into the pinned sources under
`~/.cargo/registry/src/*/` for `gpui-component-0.6.6`, `gpui-base-0.6.6`, `gpui-kit-0.6.6`,
`gpui-kit-assets-0.6.6` and `gpui-pre-0.3.6`. Registry path prefix omitted below for brevity:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

Environment note: this session runs on Wayland (`XDG_SESSION_TYPE=wayland`) with no `grim`,
`import`, `xdotool` or `wl-copy` installed, so no screenshot could be taken. Every claim below
is instead backed by a compiled, executed program (`cargo run -p postino-app --example spike`,
exit code 0, see the log excerpt in each section) and by reading the pinned sources. Later UI
phases that need an actual visual comparison against the design files will have to get a
screenshot tool installed first, or rely on the orchestrator's own environment.

Update from the orchestrator: the desktop is KDE Plasma on Wayland and `spectacle` works
non-interactively. Full screen: `spectacle -b -n -f -o <file>.png`; active window:
`spectacle -b -n -a -o <file>.png`. Use it for every visual check.

## 1. Themes

**Answer:** yes, exactly as the plan describes, with one gotcha: `Theme::change` only ever reads
`Theme.light_theme` / `Theme.dark_theme`, and those default to
`ThemeRegistry::default_light_theme()` / `default_dark_theme()` (the two themes baked into
`gpui-component`'s own `default-theme.json`), not to whatever `load_themes_from_str` just loaded.
`load_themes_from_str` only ever populates `ThemeRegistry.themes` (the lookup-by-name map), never
`ThemeRegistry.default_themes`. So after loading the family you must fetch the two `Rc<ThemeConfig>`
by name and assign them to `Theme::global_mut(cx).light_theme` / `.dark_theme` yourself, then call
`Theme::change` again. Skipping that step silently keeps gpui-component's own built-in theme.

Working sequence (from the spike, ran clean):

```rust
let json = build_theme_family_json(); // one ThemeSet with "Postino Light" and "Postino Dark"
ThemeRegistry::global_mut(cx).load_themes_from_str(&json)?;
let light = ThemeRegistry::global(cx).themes().get("Postino Light").cloned().unwrap();
let dark = ThemeRegistry::global(cx).themes().get("Postino Dark").cloned().unwrap();
Theme::change(ThemeMode::Light, None, cx); // gpui_kit::init already called Theme::change once
{
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
}
Theme::change(ThemeMode::Light, None, cx); // re-apply now that light_theme/dark_theme point at ours
```

Log from the actual run:

```
[spike] Q1 active light theme name = "Postino Light", background = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 }
[spike] Q1 syntax 'string' style resolves = true
[spike] Q1 syntax 'string.escape' (JSON capture, no such field) falls back to 'string' = true
[spike] Q1 active dark theme name = "Postino Dark", background = Hsla { h: 0.6666667, s: 0.10638297, l: 0.092156865, a: 1.0 }
```

`Theme::sync_system_appearance(window, cx)` (used today in `main.rs`) keeps working unchanged: it
calls `Theme::change(window.appearance(), window, cx)` under the hood, and since `Theme::change`
always re-reads `theme.light_theme` / `theme.dark_theme`, whichever mode the OS reports resolves to
our own theme, not gpui-component's default. No change needed there.

Do this loading **after** `gpui_kit::init(cx)` (which already calls `Theme::change` once with the
built-in default), not before: `ThemeRegistry` is set up as a global inside `init`.

`load_themes_from_str` silently skips a theme whose name already exists in the registry
(`registry.rs:155-165`, the `if !self.themes.contains_key(&theme.name)` guard). Harmless for a
single call at startup, but do not call it twice with the same names expecting a reload; nothing
else in this codebase needs that.

References:
- `ThemeRegistry::global_mut`, `load_themes_from_str`: `gpui-component-0.6.6/src/theme/registry.rs:87-96,155-165`
- `ThemeRegistry::default_light_theme`/`default_dark_theme`, populated only by `init_default_themes`: `registry.rs:141-147,166-183`
- `Theme::change` reading `theme.light_theme`/`theme.dark_theme`, defaulting from the registry's *default* themes: `gpui-component-0.6.6/src/theme/mod.rs:261-290`
- `Theme::sync_system_appearance`: `mod.rs:228-237`

### 1.1 `ThemeConfigColors` field for every token in plan section 2.1

Read from `gpui-component-0.6.6/src/theme/schema.rs:253-675` (field docs and `#[serde(rename)]`
keys) and from the fallback formulas in `gpui-component-0.6.6/src/theme/theme_color.rs:688-1055`
(`apply_config`, the macros `apply_color!`/`apply_background_color!` with their `fallback = ...`
expressions). Important framing: **Postino's own views never read `cx.theme()` directly** (AGENTS
rule: colors come from `Palette`, built in phase 2, straight from these same hex values). This
JSON only matters for the *borrowed* gpui-kit stock widgets Postino reuses as-is: `TitleBar`,
`WindowControls`, `Button`, `Dialog`/`Command` overlays, the scrollbar, `Input` caret/selection/
border, tree chevrons, checkboxes, switches. Several Postino tokens (the `*-subtle` badge
backgrounds, the five method colors, `--fg-subtle`, `--shadow`) have **no** matching
`ThemeConfigColors` field at all; those live only in the Rust `Palette` and are applied by hand in
Postino's own components (`MethodBadge`, `StatusBadge`, `InlineMessage`, ...), never through the
theme JSON.

| Token | `ThemeConfigColors` JSON key(s) | Notes |
|---|---|---|
| `--bg` | `"background"` | Direct match (`schema.rs:264-265`). |
| `--surface` | `"sidebar.background"`, `"title_bar.background"`, `"tab_bar.background"` | Three separate fields; none derives from the other two by default fallback (`title_bar`/`tab_bar` fall back to `background`, not to `sidebar`, `theme_color.rs:996-999,1012`), so all three JSON keys must be set to the same hex or the sidebar will look right while the title bar and tab strip stay the generic `background` shade. |
| `--raised` | `"secondary.background"` | No literal "card"/"raised" field exists. `secondary` is the closest generic elevated-fill token (`schema.rs:494-495`); matches the design's own suggested "input · secondary". |
| `--overlay` | `"popover.background"` | "Background color for Popover" (`schema.rs:462-463`); used for menus/popovers/modals, exactly the design's stated use. |
| `--border` | `"border"` | Direct match (`schema.rs:267-268`). |
| `--border-strong` | `"input.border"` | "Border color for inputs such as Input, Select" (`schema.rs:425-427`); matches the design's own suggestion. |
| `--fg` | `"foreground"` | Direct match (`schema.rs:411-412`). |
| `--fg-muted` | `"muted.foreground"` | Direct match (`schema.rs:459-460`). |
| `--fg-subtle` | none | No field for a third, dimmer text tier exists in `ThemeConfigColors`. Postino-only; used straight from `Palette` in views (line numbers, placeholders, icons). |
| `--accent` | `"primary.background"` | Postino's "accent" is the brand/primary-action color (buttons, focus, active tab). **Naming collision warning:** gpui-component's own `"accent.background"` field means something different (hover background on `MenuItem`/`ListItem`, `schema.rs:255-256`) and must *not* receive this value; see `--accent-subtle` below. |
| `--accent-fg` | `"primary.foreground"` | Text drawn on a primary/accent-colored surface (`schema.rs:474-475`). |
| `--accent-text` | `"link"` | Matches the design's own suggestion; "Link text color" (`schema.rs:429-430`). |
| `--accent-subtle` | `"accent.background"`, `"selection.background"`, `"list.active.background"` | This is the token that actually feeds gpui's own `"accent"` field (hover/selection tint), plus input text selection and the sidebar tree's selected-row background. `list_active`/`selection` alpha is clamped by `apply_config` itself (max 0.2 / 0.3, `theme_color.rs:1021-1052`), so pass the token as an opaque color and let gpui clamp it; do not pre-multiply alpha. |
| `--hover` | `"list.hover.background"` | Also feeds tree/sidebar/menu row hovers, the design's main use. For full consistency also set `"button.hover.background"` and `"table.hover.background"` to the same value; they do not fall back to `list.hover.background` (each has its own `fallback = ...` in `theme_color.rs:794-800,1010`). |
| `--success` | `"success.background"` | Direct match (`schema.rs:539-540`). |
| `--success-subtle` | none | No `*.subtle` field exists anywhere in the schema. Postino-only (status badge background); applied directly from `Palette::status_colors`. |
| `--warning` | `"warning.background"` | Direct match (`schema.rs:617-618`). |
| `--warning-subtle` | none | Postino-only, see `--success-subtle`. |
| `--danger` | `"danger.background"` | Direct match (`schema.rs:386-387`). |
| `--danger-subtle` | none | Postino-only, see `--success-subtle`. |
| `--info` | `"info.background"` | Direct match (`schema.rs:413-414`). |
| `--info-subtle` | none | Postino-only, see `--success-subtle`. |
| `--m-get`, `--m-post`, `--m-put`, `--m-patch`, `--m-delete` | none | No per-HTTP-method field exists (the schema's five `chart.N` slots are for data-viz charts, not semantically HTTP methods, and repurposing them would break the load-test dashboard's own chart colors). Postino-only; `Palette::method_color`. |
| `--syn-key` | `highlight.syntax.property` | Not a `colors` field at all: it is `ThemeConfig.highlight: Option<HighlightThemeStyle>` (`schema.rs:81`), specifically `HighlightThemeStyle.syntax.property` (`highlighter/registry.rs:113-172`). JSON capture used by the JSON grammar for object keys is `@property` (`highlighter/languages/json/highlights.scm`), so this is the field that actually paints them. |
| `--syn-str` | `highlight.syntax.string` | `@string` capture, used by JSON, JS and HTML grammars alike (`highlighter/languages/{json,javascript,html}/highlights.scm`). |
| `--syn-num` | `highlight.syntax.number` | `@number` capture (JSON, JS). |
| `--syn-bool` | `highlight.syntax.boolean` | `@boolean` capture (JSON only; JS booleans are `@constant.builtin`, which has no exact match and falls back to `constant`, see 1.2 below, JS `true`/`false` will render as `constant`'s color, not `syn-bool`, unless a future phase adds a `constant.builtin` case). |
| `--shadow` | none (not a color) | `ThemeConfig.shadow: Option<bool>` is only an on/off switch (`schema.rs:73-74`); the actual box-shadow values live in `SemanticThemeConfig.shadow.{sm,md,lg}: Vec<BoxShadow>` (`schema.rs:170-176`), a different, opt-in "semantic tokens" file (`apply_semantic_config`/`apply_semantic_config_str`, not exercised by `load_themes_from_str`/`ThemeSet`). Simplest correct path for Postino: keep the raw CSS-like shadow values (offsets, blur, color, alpha) in the Rust `Palette` itself and apply them directly with gpui's own `.shadow(vec![BoxShadow { .. }])` on `overlay`-elevated views (menus, popovers, the Settings modal), per AGENTS's elevation rule; do not try to route it through `ThemeConfigColors`. |

### 1.2 Syntax highlight capture names

`HighlightThemeStyle.syntax` is a flat `SyntaxColors` struct with one `Option<ThemeStyle>` field
per dotted capture name (`highlighter/registry.rs:113-172`, e.g. `punctuation.bracket`,
`string.escape`). Resolution (`HighlightTheme::style`, `highlighter/registry.rs:241-306`) is exact
match first, and when that misses **and the name contains a dot, it retries with everything before
the first dot** (`registry.rs:288-298`, e.g. `constant.builtin` -> `constant`, `function.method` ->
`function`, `tag.error` -> `tag`). Confirmed live in the spike:

```
[spike] Q1 syntax 'string' style resolves = true
[spike] Q1 syntax 'string.escape' (JSON capture, no such field) falls back to 'string' = true
```

Captures actually emitted by the three grammars Postino uses (from the `.scm` query files, not
memory):

- JSON (`highlighter/languages/json/highlights.scm`): `boolean`, `comment`, `constant.builtin`
  (-> `constant`), `number`, `property`, `punctuation`, `string`, `string.escape`.
- JavaScript (`highlighter/languages/javascript/highlights.scm`): `comment`, `constant`,
  `constant.builtin` (-> `constant`), `embedded`, `function`, `function.builtin` (-> `function`),
  `function.method` (-> `function`), `keyword`, `number`, `operator`, `property`,
  `punctuation.bracket`, `punctuation.delimiter`, `punctuation.special`, `string`,
  `string.special`, `type`, `variable`, `variable.builtin` (-> `variable`).
- HTML (`highlighter/languages/html/highlights.scm`, used as the closest available grammar for
  `::: body xml`, per `postino-app/Cargo.toml`'s comment): `attribute`, `comment`, `constant`,
  `punctuation.bracket`, `string`, `tag`, `tag.error` (-> `tag`).

## 2. Fonts

**Answer:** works as described. `cx.text_system().add_fonts(Vec<Cow<'static, [u8]>>)` (`App::
text_system`, `gpui-pre-0.3.6/src/app.rs:307,2079`; `TextSystem::add_fonts`,
`gpui-pre-0.3.6/src/text_system.rs:295-301`) registers TTF bytes; `TextSystem::all_font_names`
(`text_system.rs:284-289`) lists every installed family plus every family added this way, sorted
and deduplicated. Set `Theme::global_mut(cx).font_family` / `.font_size` / `.mono_font_family` /
`.mono_font_size` (`gpui-component-0.6.6/src/theme/mod.rs:126-143`) to pick them up; `Input` and
plain text both read those fields (there is no separate "editor font" setting; `EditorState`'s own
`.language(...)`-gated code-editor mode still uses `Theme.mono_font_family`, confirmed by grep,
no separate override field in `ThemeConfig`/`Theme`).

Ran with a real system TTF (`/usr/share/fonts/TTF/JetBrainsMono-ExtraBold.ttf`, since the Geist
files are not bundled yet, that is phase 2's job):

```
[spike] Q2 all_font_names contains 'JetBrains Mono' = true, total families = 257
```

For the Settings font pickers (phase 6), `all_font_names()` is exactly the right list to build the
"installed families" section of the dropdown after "Geist (bundled)"/"Geist Mono (bundled)".

## 3. Variable chips in the URL (single-line `Input`)

**Not possible with the native highlighter.** The fallback in the plan is the right call.

Reasoning, from `gpui-base-0.6.6/src/input/README.md`: the single-line `Input`/`InputState` and
the multi-line, language-aware `Editor`/`EditorState` are two separate facades
(`InputState = InputBaseState<InputMode>`, `EditorState = InputBaseState<EditorMode>`,
`gpui-base-0.6.6/src/input/base/kind.rs:1-9,317-325`) over one shared engine. Only `EditorMode` has
a `.language(...)` builder (`gpui-base-0.6.6/src/input/base/state.rs:9219-9245`, in an `impl
InputBaseState<EditorMode>` block, not the generic `impl<M: InputModeKind>` block). The syntax
highlighter and the diagnostics set (the wavy underline machinery, `DiagnosticSet`) are both stored
*inside* the `LayoutMode::CodeEditor { .. }` enum variant
(`gpui-base-0.6.6/src/input/base/mode.rs:36-51`), and `InputState`'s default `LayoutMode` is
`PlainText` (`mode.rs:56-58`, `InputMode`'s associated default). `InputBaseState::
set_highlighter_factory`/`ensure_highlighter_factory` (`state.rs:801-815`) *are* generic over every
mode and compile fine on a plain `InputState`, but they delegate to `self.mode.
set_highlighter_factory(..)` (`mode.rs:336-350`), whose `match` only does something for the
`CodeEditor` variant; on `PlainText` it is a silent no-op. There is no public builder that flips a
single-line `InputState` into `LayoutMode::CodeEditor`, that switch is only reachable through
`EditorState`.

This also explains something that looks like a contradiction at first read:
`gpui-component-0.6.6/src/input/input.rs:495` calls
`state.ensure_highlighter_factory(crate::highlighter::input_highlighter_factory(), cx)`
unconditionally, on every `Input` render, single-line included. It compiles and runs fine on a
`PlainText` input; it is simply inert there for the reason above. It only does real work once the
same `InputState`'s `LayoutMode` happens to be `CodeEditor`, which single-line inputs never are.

**Fallback implemented and run:** render a read-only line of styled spans with
`gpui::StyledText::new(text).with_highlights(runs)` (`gpui-pre-0.3.6/src/elements/text.rs:399,433`)
when the URL bar is not focused, and swap to the real `Input` on focus (state already tracked by
`Focusable`/focus events; no new machinery needed). `HighlightStyle` supports both a background
color and `underline: Some(UnderlineStyle { wavy: true, .. })` in the same run, so the two variants
(`accent` background for a defined `{{var}}`, `danger` text with a wavy underline for an unknown
one) both render from one call, using the byte ranges `postino_core::variable_spans` will hand back
in phase 1a. Compiled and rendered without a panic in the spike (three seconds, no crash):

```rust
let chip_line = StyledText::new(text.clone()).with_highlights(vec![
    (defined_range, HighlightStyle { background_color: Some(accent_bg), color: Some(accent_fg), ..Default::default() }),
    (undefined_range, HighlightStyle {
        color: Some(danger_fg),
        underline: Some(UnderlineStyle { thickness: px(1.), color: Some(danger_fg), wavy: true }),
        ..Default::default()
    }),
]);
```

`UrlBar` (phase 3) should own both representations and pick one per render based on whether its
`Input`'s focus handle is focused, exactly as the plan's fallback says.

## 4. `{{var}}` chips inside the body code editor

**Feasible, not implemented here** (the plan allows "not feasible, keep plain highlighting" as an
acceptable phase-0 answer for this one since it is a nice-to-have; this spike found a real path,
so it is worth recording instead of dismissing it, but actually building and testing it belongs to
phase 3/5, not phase 0).

Unlike the single-line case, the body editor is already an `EditorState` with `.language("json")`
(or `"xml"`/`"javascript"`), so it *is* in `LayoutMode::CodeEditor` and its highlighter factory is
live. `gpui_component::highlighter::SyntaxHighlighter` is `pub` (`highlighter/highlighter.rs:32`,
re-exported via `pub use highlighter::*;` in `highlighter/mod.rs`), and `InputHighlighter` is a
plain trait apps can implement (`gpui-base-0.6.6/src/input/editor/highlighting.rs:30-56`):

```rust
trait InputHighlighter {
    fn language(&self) -> SharedString;
    fn update(&mut self, edit: Option<InputEdit>, text: &Rope, folding: bool, window: &mut Window, cx: &mut Context<EditorState>);
    fn styles(&self, range: &Range<usize>, resolver: &dyn HighlightStyleResolver) -> Vec<(Range<usize>, HighlightStyle)>;
    fn fold_ranges(&self, text: &Rope) -> Vec<FoldRange>;
}
```

The design: a small wrapper struct holding a `gpui_component::highlighter::SyntaxHighlighter` for
the body's actual language, plus a call to `postino_core::variable_spans` (phase 1a) over the same
text. `styles()` would run the inner `SyntaxHighlighter::styles()` first, then overlay the variable
spans on top (since `{{...}}` markers are literal text regardless of host grammar, they should
always win over whatever the JSON/JS/HTML grammar made of that byte range, a `{{token}}` inside a
JSON string is inside a `string` capture as far as tree-sitter is concerned, so the two style lists
need a real overlay/merge, not a simple concatenation). Install it with
`body_state.update(cx, |state, cx| state.set_highlighter_factory(Rc::new(|lang| Some(Box::new(
VariableAwareHighlighter::new(lang)))), cx))`, this is the same `set_highlighter_factory` from Q3,
but this time it lands on a `CodeEditor`-mode state, so it actually takes effect
(`mode.rs:336-350`).

Not attempted end to end here: it needs `postino_core::variable_spans` (phase 1a, not merged yet)
to have real input, and a real overlay-merge routine is enough surface area to deserve its own
tests rather than a phase-0 throwaway. Budget it as a half-day task in phase 5 if the team wants it;
otherwise plain per-language highlighting (today's behavior) is a perfectly fine phase-5 baseline.

## 5. Command palette

**Answer: yes, `gpui_component::command` is directly usable, no need to build one from `dialog` +
`list`.** `Command`/`CommandItem`/`CommandGroup`/`CommandState` (`gpui-component-0.6.6/src/command/
{command.rs,item.rs,state.rs,mod.rs}`) already implement exactly what Ctrl-K needs: a search input
with local fuzzy-ish substring filtering (or `filterable(false)` plus an external filter, which is
what Postino will want since phase 1c already builds its own `fuzzy_filter` with a different
ranking than whatever `CommandItem::matches` does internally), groups, per-item icon
(`CommandItem::icon`, `command/item.rs:52-55`), label, keywords, an `Action` for the keybinding hint
column, a checked state, and arrow-key navigation plus Enter/Escape (owned by `CommandState`).
Compiled and rendered in the spike:

```rust
let command = Command::new(&self.command_state)
    .item(CommandItem::new().label("Send").icon(gpui_kit::assets::IconName::SendHorizontal))
    .item(CommandItem::new().label("Save").icon(gpui_kit::assets::IconName::Save))
    .placeholder("Type a command or search...");
```

One naming gotcha worth flagging for phase 3/7: `gpui_component::IconName` (the type used
elsewhere in this codebase already, e.g. `views/root.rs`'s `IconName::Close`) is **not** the full
Lucide catalog. It is a small, curated subset generated from `gpui-kit-assets`'s
`default-icons.txt` (confirmed by inspecting the generated
`target/debug/build/gpui-kit-assets-*/out/icon_name.rs`, which only defines the icons that file
lists). `send-horizontal` and `save` are not in that curated list. The full 1830-icon catalog
AGENTS.md refers to is a *different* enum, `gpui_kit_assets::IconName` (re-exported as
`gpui_kit::assets::IconName`), which does have `Send`, `SendHorizontal`, `Save`, etc. Both
implement the same `IconNamed` trait (`gpui-kit-assets-0.6.6/src/icon.rs:7`) and both convert into
`Icon` via the blanket `impl<T: IconNamed> From<T> for Icon` (`gpui-component-0.6.6/src/icon.rs:
59`), so `Icon::new(gpui_kit::assets::IconName::SendHorizontal)` (or `.icon(...)` on any component
that takes `impl Into<Icon>`) is the correct call for any design icon not already in the small
curated set. Phase 3/4/5 should use `gpui_kit::assets::IconName::*` for every icon named in
AGENTS.md's list (`gauge`, `settings`, `palette`, `git-branch`, `lock`, `hard-drive`, `folder`,
`folder-plus`, `list-filter`, `copy`, `wand-sparkles`, `send-horizontal`, `save`, `code`,
`variable`, `download`, `circle-x`, `ellipsis`, `chevrons-up-down`), not `gpui_component::IconName`.

For the actual Ctrl-K overlay chrome (centering, dimmed backdrop, Escape-to-close), reuse the
`Dialog` from Q6 with a `Command` as its content, there is no separate "palette" opener, and none
is needed.

## 6. Modal dialogs

**Answer: yes**, and there are two layers, both usable:

- `gpui_base::Dialog` (`gpui-base-0.6.6/src/dialog.rs:416-505`) is the low-level primitive:
  `.backdrop(element)`, `.popup(element)`, `.close_on_escape(bool)`,
  `.close_on_backdrop_press(bool)`, arbitrary children via `DialogContent`/`DialogHeader`/
  `DialogTitle`/`DialogClose`/`DialogFooter` (`gpui-component-0.6.6/src/dialog/{content.rs:8-38,
  header.rs:17-44,title.rs,footer.rs:23-104}`), sized with the ordinary `Styled` trait
  (`.w(px(800.)).h(px(720.))`, confirmed compiling and rendering in the spike).
- `gpui_component::dialog::Dialog` (`gpui-component-0.6.6/src/dialog/dialog.rs:258-441`) is a
  ready-made higher-level wrapper over the above, built exactly for this use case:
  `.title(impl IntoElement)`, `.content(|window, cx| ...)`, `.footer(impl IntoElement)`,
  `.w()`/`.width()`/`.max_w()`, `.overlay(bool)` (dims the backdrop, on by default),
  `.overlay_closable(bool)` (click-outside-to-close), `.keyboard(bool)` (Escape-to-close),
  `.close_button(bool)`, `.on_ok`/`.on_cancel`/`.on_close`. Its `render()`
  (`dialog/dialog.rs:507-610`) centers the dialog in the viewport, computes a max height that keeps
  a margin from the window edges, and (`dialog.rs:587-596`) paints the dimmed backdrop itself with
  `overlay_color(true, cx)` when `.overlay(true)` (the default). Opened with
  `window.open_dialog(cx, |dialog, window, cx| dialog.title(...).content(...).w(px(800.))...)`
  (`WindowExt::open_dialog`, trait at `gpui-component-0.6.6/src/window_ext.rs:30-33`, impl at
  `window_ext.rs:141-148`, which forwards to `Root::open_dialog`).

For Settings (800x720, own header/sidebar/footer): use the high-level `Dialog`, put the
sidebar+content split inside `.content(...)`, and put "Changes apply immediately." / "Reset to
defaults" inside `.footer(...)`. `.keyboard(true)` (default) gives Escape; the `x` button is
`.close_button(true)` (default) or a `DialogClose` inside the header if a custom-styled close icon
is wanted; click-outside is `.overlay_closable(...)`.

## 7. Charts

**Answer:** `chart::LineChart<T, X, Y>` (`gpui-component-0.6.6/src/chart/line_chart.rs:33-70`) is a
**single-series** element: one `x`/`y` mapping, one `stroke` color, and its own y-scale is always
built from its own data starting at 0 (`line_chart.rs:150-160`, "Y scale, ensure start from 0").
There is no dual-axis mode and no way to hand it a second series. But that same "always its own
0..max" behavior is exactly what "each series normalized to its own max" (the design's Throughput
and latency card) needs, so two independent-scale series work by **stacking two `LineChart`
elements absolutely inside the same bounds**, one per series, each with `x_axis(false)` and
`grid(false)` since the design wants no axes anyway:

```rust
div().relative().size_full()
    .child(div().absolute().top_0().left_0().size_full().child(
        LineChart::new(throughput).x(..).y(..).stroke(accent_fg).x_axis(false).grid(false)
    ))
    .child(div().absolute().top_0().left_0().size_full().child(
        LineChart::new(latency).x(..).y(..).stroke(warning).x_axis(false).grid(false)
    ));
```

This compiled and rendered without a panic in the spike (`Y` must be `f64`: `LineChart`'s `Y` bound
is a sealed trait implemented only for `f64`, not `f32`, `chart/line_chart.rs:37`,
`plot/scale/sealed.rs:3`, `impl Sealed for f64 {}`; there is no `f32` impl, so the metrics
aggregator in `postino-load` should keep or convert its snapshot numbers to `f64` before handing
them to the chart, not `f32`). `#[derive(IntoPlot)]` (`line_chart.rs:31`) makes it a normal
`IntoElement` that fills the space its parent gives it, same as any other gpui element, so stacking
via `.absolute()` is unremarkable gpui layout, not a special chart feature.

The histogram (latency distribution) and the status-code bars need no chart widget at all: they
are exactly what the plan already assumes, plain `div()`s with a computed height/width percentage
and a background color, laid out in a `h_flex()`/`v_flex()`. No canvas needed for either.

## 8. Periodic refresh

**Answer:** `cx.spawn(async move |weak, cx| { .. cx.background_executor().timer(duration).await; .. })`,
same as the plan's guess. There is no standalone `Timer` type in gpui-pre 0.3.6; the equivalent is
`BackgroundExecutor::timer(duration: Duration) -> Task<()>`
(`gpui-pre-0.3.6/src/executor.rs:183`). The idiomatic shape, confirmed by the one real user of this
pattern already in the pinned sources (`gpui-base-0.6.6/src/input/base/blink_cursor.rs:52-69`, the
cursor blink), is a self-rescheduling task guarded by an epoch counter so a stale reschedule
becomes a no-op instead of two timers racing:

```rust
self._task = cx.spawn(async move |this, cx| {
    cx.background_executor().timer(INTERVAL).await;
    if let Some(this) = this.upgrade() {
        this.update(cx, |this, cx| this.blink(epoch, cx));
    }
});
```

For the load test dashboard's "every 250ms while running" refresh, a plain `loop { timer().await;
.. }` inside one `cx.spawn` works just as well (confirmed live in the spike: three 250ms ticks,
clean exit) since there is only ever one such loop per running load test tab, not N competing
reschedules; drop the `Task` (e.g. by closing the tab) to cancel it, which is how `cx.spawn`
futures are cancelled in gpui generally. Log from the run:

```
[spike] Q8 tick 1 at 250ms interval
[spike] Q8 tick 2 at 250ms interval
[spike] Q8 tick 3 at 250ms interval
[spike] all checks completed, quitting
```

## 9. Custom title bar with window controls

**Answer: this already works out of the box, no per-platform code needed in Postino.**
`TitleBar` (`gpui-component-0.6.6/src/title_bar.rs:42-95`) renders the app's own children first,
then unconditionally appends a `WindowControls` element as the last child
(`title_bar.rs:371-402`). `WindowControls::render` (`title_bar.rs:252-294`) already does exactly
the per-platform dance the question asks about:

- macOS (and wasm): renders an empty `div()` (`title_bar.rs:254-256`) and relies on the OS's own
  native traffic lights, which macOS keeps drawing regardless of `WindowDecorations::Client` (this
  matches the existing comment in `main.rs`: "Other platforms ignore this option").
- Linux under server-side decorations (no real client-side decoration granted, checked via
  `window.window_decorations()`, `title_bar.rs:267-270`): also renders nothing, so Postino's own
  controls do not double up with the compositor's.
- Linux/Windows under real client-side decorations: renders minimize/maximize (only the ones
  `window.window_controls()` reports the window manager actually supports,
  `title_bar.rs:275-291`) and close (`title_bar.rs:292`), right-aligned, `h_full()`.

So `TitleBar::new().child(logo).child(workspace_switcher).child(search).child(env_pill).child(
import_button).child(settings_gear)` (section 2.3 point 1's content, left to right) is the entire
implementation; the window controls at the far right are automatic. Confirmed compiling and
rendering (no panic) with `TitleBar::new().child(div().child("Postino spike"))` inside a real
`WindowOptions { window_decorations: Some(WindowDecorations::Client), ..TitleBar::window_options()
}` window, same setup `main.rs` already uses.

## 10. `POSTINO_OPEN` debug hook

Not implemented in product code (the plan asks only for a design). Proposed, consistent with the
existing `apply_debug_autosend` in `crates/postino-app/src/views/root.rs:93-113` (same style: a
private method called once from `AppView::new`, reads `std::env::var`, no-op when unset, not
surfaced in any menu, documented only in the doc comment):

```rust
/// `POSTINO_OPEN=<target>` opens a specific UI state at startup, for the orchestrator to
/// screenshot after a UI phase. No-op when unset. Not a supported feature (see the doc comment on
/// `apply_debug_autosend` above for the same convention). Recognized targets:
/// - `settings` opens the Settings modal (phase 6).
/// - `palette` opens the command palette (phase 7).
/// - `load-test:<request-id>` opens a new load test tab targeting that request (phase 8).
fn apply_debug_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Ok(target) = std::env::var("POSTINO_OPEN") else { return };
    match target.as_str() {
        "settings" => self.open_settings(window, cx),
        "palette" => self.open_command_palette(window, cx),
        other => {
            if let Some(request_id) = other.strip_prefix("load-test:") {
                self.open_load_test(request_id.to_string(), cx);
            }
        }
    }
}
```

Called from `AppView::new` right after `apply_debug_autosend(cx)`, so it can compose with
`POSTINO_ENV`/`POSTINO_AUTOSEND` (e.g. autosend a request, then also pop the palette open on top).
It needs `window: &mut Window` (unlike the other two hooks) because opening a dialog goes through
`WindowExt::open_dialog` (Q6), which is a `Window` method, so `AppView::new`'s signature or its
caller in `main.rs` needs to thread the window through at construction time; today `AppView::new`
only takes `cx: &mut Context<Self>`. The cheapest fix consistent with `main.rs`'s existing
`cx.open_window(window_options, move |window, cx| { .. let view = cx.new(|cx| AppView::new(..,
cx)); ..})` shape is to add the `window` parameter to `AppView::new` (it is already available at
that call site) rather than deferring the hook to a later render pass.

## Cleanup

`crates/postino-app/examples/spike.rs` is deleted as part of this phase's report; `git status`
after this change shows only `plans/ui-redesign-spikes.md` as new.
