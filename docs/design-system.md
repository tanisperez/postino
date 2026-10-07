# Postino design system

Reference for anyone (human or agent) touching the UI. It replaces the Claude Design HTML mockups
that used to live in `postino_design_system/`: the design is implemented, so the code is the
source of truth and this file is its readable summary. If this file and the code disagree, the
code wins; fix this file.

Where things live (all under `crates/postino-app/src/`):

| What | File |
| --- | --- |
| Color tokens (light and dark), `Palette`, gpui-kit theme JSON | `theme/palette.rs` |
| Sizes, heights, widths, radii | `theme/metrics.rs` |
| Fonts, theme registration, live settings apply | `theme/mod.rs` |
| Reusable widgets | `views/components/` (see the gallery: `POSTINO_OPEN=components`) |
| Environment dot color rule | `state/env_color.rs` |

## Rules

- Colors only through `cx.palette()` (a `Palette`), never from `cx.theme()` and never as a literal.
- Sizes only from `theme::metrics`, never as a literal.
- The two themes ("Postino Light", "Postino Dark") are built from the same token list. The hex
  literals are written once, in `LIGHT` and `DARK` in `theme/palette.rs`. Add a token there, not
  in a second place.
- Panels are flat (border only). Inputs and cards use `raised` plus a border. Menus, popovers and
  modals use `overlay` plus the shadow token. No shadows on fixed panels.
- The accent (indigo) is for the primary action, the selected row or tab, links and focus.
  Do not use it as decoration.
- State colors are semantic: `success` green, `warning` amber, `danger` red, `info` blue. Each has
  a `*-subtle` tint used as the badge background.
- No em dash anywhere (code, comments, docs, strings).

## Palette

Hex values copied from `theme/palette.rs`.

### Surfaces, text and borders

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `bg` | `#fcfcfd` | `#15161a` | Editor and content panels |
| `surface` | `#f4f5f7` | `#1b1c21` | Sidebar, title bar, tab bars, status bar |
| `raised` | `#ffffff` | `#23252c` | Inputs, cards, code editor |
| `overlay` | `#ffffff` | `#262830` | Menus, popovers, modals (fill) |
| `scrim` | `#0a0b1073` | `#0a0b1073` | Dimmed backdrop behind a modal (same in both) |
| `border` | `#e3e5ea` | `#2c2e36` | Default separator and border |
| `border_strong` | `#cdd0d8` | `#3b3e48` | Control borders (inputs, buttons) |
| `fg` | `#1b1c22` | `#e9eaee` | Primary text |
| `fg_muted` | `#5c606e` | `#a0a3ae` | Secondary text, inactive tabs |
| `fg_subtle` | `#8b8e9a` | `#6f7280` | Placeholders, icons, line numbers, counts |
| `hover` | `#eceef2` | `#25272e` | Hover background |

Derived at runtime (not hex literals): `pressed` is `hover` blended with 2/3 of `border_strong`.
`accent_hover` and `danger_hover` shift the base color one step (0.07 lightness) toward black in
light and toward white in dark. `accent_pressed` and `danger_pressed` shift two steps.

### Accent and states

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `accent` | `#4f57d8` | `#5b63e6` | Primary button, focus, logo square |
| `accent_fg` | `#ffffff` | `#ffffff` | Text on `accent` and `danger` fills |
| `accent_text` | `#3f47c4` | `#a3a9ff` | Links, selected row label |
| `accent_subtle` | `#e8e9fd` | `#272b4d` | Selected row or item background |
| `success` | `#1d8657` | `#4fc88f` | 2xx, passing tests |
| `success_subtle` | `#e2f4ea` | `#15302a` | Success badge background |
| `warning` | `#9a6500` | `#e8b75a` | 4xx, warnings |
| `warning_subtle` | `#fbf0d9` | `#352b15` | Warning badge background |
| `danger` | `#cf3535` | `#f27171` | 5xx, errors, destructive actions |
| `danger_subtle` | `#fde7e7` | `#3b1e21` | Danger badge background |
| `info` | `#1c6fb0` | `#62b3f0` | 3xx, informational |
| `info_subtle` | `#e1eef9` | `#162a3b` | Info badge background |

### HTTP methods

| Token | Light | Dark | Method |
| --- | --- | --- | --- |
| `m_get` | `#1d8657` | `#4fc88f` | GET |
| `m_post` | `#9a6500` | `#e8b75a` | POST |
| `m_put` | `#1c6fb0` | `#62b3f0` | PUT |
| `m_patch` | `#7a4fd6` | `#b597f7` | PATCH |
| `m_delete` | `#cf3535` | `#f27171` | DELETE |

HEAD, OPTIONS and custom methods use `fg_muted`. The method badge background is the method color
at 14% alpha (`Palette::method_badge_bg`).

### Syntax highlight

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `syn_key` | `#3f47c4` | `#a3a9ff` | JSON keys and properties |
| `syn_str` | `#1d7a4f` | `#86d7a5` | Strings |
| `syn_num` | `#9a6500` | `#e8b75a` | Numbers |
| `syn_bool` | `#b03a78` | `#f3a2c6` | Booleans |

### Shadow (menus, popovers, modals)

Two layers, horizontal offset always 0.

| Layer | Light | Dark |
| --- | --- | --- |
| 1 | y 12, blur 32, spread 0, `rgba(20,22,40,0.14)` | y 12, blur 32, spread 0, `rgba(0,0,0,0.45)` |
| 2 | y 0, blur 0, spread 1, `rgba(20,22,40,0.06)` | y 0, blur 0, spread 1, `rgba(255,255,255,0.04)` |

### Semantic mappings

- Response status (`Palette::status_colors`): 2xx `success`, 3xx `info`, 4xx `warning`, 5xx
  `danger`. Background is the matching `*-subtle`. "Not sent" or a send failure is `fg_muted` on
  `hover`.
- Environment dot (`state::env_color`), from the name, case-insensitive: contains `prod` is
  `danger`; contains `stag`, `test`, `qa` or `uat` is `warning`; anything else is `success`. "No
  environment" is a hollow ring in `fg_subtle`.
- Tests tab count: `success` when all pass, `danger` otherwise.

## Typography

- UI font **Geist**, base 13 px (user setting 11 to 16). Mono font **Geist Mono**, 12.5 px in
  editors (user setting 10 to 18, 0.5 steps). Both are bundled in `assets/fonts/` (OFL).
- Scale: 20/600 run title, 15/600 dialog title, 13/500 buttons and active tabs, 13/400 body,
  12/400 secondary (timings, sizes, hints), 11/600 uppercase with 0.06em tracking for section
  labels ("COLLECTIONS"), mono 12.5/400 for code, URL and key-value cells, mono 10/600 for method
  labels in the tree and tabs. Status bar text is 11.5 px in `fg_subtle`.

## Metrics (`theme/metrics.rs`)

Heights (px): title bar 40, open-tabs bar 36, response tab bar 36, request inner tabs 32, URL bar
32, Send button 32, controls 30, tree row 26, menu row 28, env panel row 30, status bar 24,
segmented control inner item 22, status badge 22, env pill 26, sidebar filter 28.

Widths (px): sidebar 248 (resizable 180 to 480), activity rail 48 (buttons 34), search trigger
360, window control 40, Send button min 100, method label 34, key-value side columns 30.

Icon buttons (px): 24 small, 28 large, 22 in sidebar section headers. Status bar shortcuts button
20.

Radii (px): 4 chips and checkboxes (`RADIUS_XS`), 6 segmented items, tree rows and badges
(`RADIUS_SM`), 8 inputs, buttons and editor cards (`RADIUS_MD`), 10 menus, popovers and dashboard
cards (`RADIUS_LG`), 14 modals (`RADIUS_MODAL`). gpui-kit's own widgets get 8 general and 14 large.

## Layout, top to bottom

1. **Title bar** (`surface`, bottom border): "P" logo square (18 px, radius 5, `accent`),
   "Postino" (600), 1x16 separator, workspace switcher (folder icon, folder name, chevron),
   centered 360 px search trigger ("Search requests and actions", `Ctrl K` hint), environment
   pill (dot, name, chevron, on `raised` with border), "Import" ghost button, settings gear,
   window controls. On Linux the app draws its own chrome (client decorations).
2. **Activity rail** (48 px) left of the **sidebar** (`surface`, right border): header
   "COLLECTIONS" with new request and new folder icon buttons, a filter input with the
   `list-filter` icon, the tree (chevron for folders, 34 px method label for requests; selected
   row `accent_subtle` background with `accent_text` label, hover `hover`), and a footer with a
   top border: `hard-drive` icon and the `~`-shortened workspace path, then `git-branch` and the
   branch name (omitted outside a git repo).
3. **Open-tabs bar** (`surface`): icon (optional, `gauge` for load tests), method label, name,
   then a filled 8 px dot when dirty or an `x` when clean (hovering a dirty tab shows the `x`).
   Active tab: `bg` background, 2 px `accent` line on top, `fg` text. Inactive: transparent,
   `fg_muted`. A "+" after the last tab.
4. **Request pane**: URL bar (method selector and URL joined in one bordered `raised` box, method
   in its color, variables shown as chips) plus the primary Send button with its `Ctrl ↵` hint
   (`Cmd ↵` on macOS). Underline tabs: Params, Headers, Body, Pre-request, Post-response, Docs,
   with counts in `fg_subtle`. Body tab: segmented control (JSON, Text, XML, Form, None) on the
   left, a "Format" ghost button on the right, then the code editor in a bordered `raised` box
   with line numbers.
5. **Response pane**: tab bar Body, Headers (count), Tests (`passed/total`), Console (count); on
   the right the status badge, time and size. Below, a Pretty/Raw segmented control on the left
   and `copy` and `search` icon buttons on the right, then the body with line numbers.
6. **Status bar** (24 px, `surface`, top border): `lock` icon and "Local only", the active file
   id, a spacer, "Unsaved changes" when the active tab is dirty, "UTF-8".

Dialogs (Settings, Code snippet, Define variable) are modals: `overlay` fill, radius 14, shadow,
`scrim` behind. The command palette is a popover with the same surface rules.

## Components (`views/components/`)

`buttons` (primary, secondary, danger, ghost), `card`, `document_tabs`, `edit_menu`, `env_pill`,
`icon_button`, `inline_message`, `key_value_table`, `method_badge`, `section_label`,
`segmented_control`, `select_trigger`, `status_badge`, `text_field`, `underline_tabs`, `url_bar`,
`variable_chip`.
Reuse one before writing a new widget, and add new ones to the gallery.

Icons are Lucide, through `gpui_kit::assets::IconName`. The app must keep
`.with_assets(gpui_kit::assets::AllAssets)` or icons render empty.

## Performance constraint

Render paths must stay cheap and an idle window must do no work. See the Performance section of
`AGENTS.md`. A new animation, timer or hover effect that calls `cx.notify()` without a state
change breaks that rule.

## History

The original mockups (Claude Design HTML files in `postino_design_system/`: Postino Screens,
Postino Design System, Main A and B, Components, Settings, Navigation Options, Performance,
Current UI) are kept only in git history, under the tag `design-mockups` (`git checkout design-mockups -- postino_design_system`).

Decisions taken in the redesign: the main screen is the "flat panes" direction (request above,
response below, panels separated by borders, no shadows); the other direction explored (a
boxed layout with its own error panel) was discarded. Errors and warnings are `InlineMessage`
strips (warning, danger, success, with a "Define" action on unknown variables), never a
full-width red banner. Auth and OAuth 2.0 had no screen designed and are tracked as issues #14
and #15.
