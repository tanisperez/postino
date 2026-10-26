# Postino design system

Reference for anyone (human or agent) touching the UI. It replaces the Claude Design HTML mockups
that used to live in `postino_design_system/`: the design is implemented, so the code is the
source of truth and this file is its readable summary. If this file and the code disagree, the
code wins; fix this file.

The shared part (tokens, `Palette`, theme JSON, metrics, fonts and the standard components) lives
in the [telaio](https://github.com/tanisperez/telaio) crate, also used by Vano and Nastro; its
`telaio-gallery` binary shows every standard component under every sample palette. Postino's
default tokens are telaio's defaults. The rest is under `crates/postino-app/src/`:

| What | File |
| --- | --- |
| Color tokens (light and dark), `Palette`, gpui-kit theme JSON | telaio, `src/palette.rs` |
| Method colors, status code and environment color mapping | `theme/palette.rs` |
| Shared sizes and radii | telaio, `src/metrics.rs` |
| Postino's own sizes, heights, widths | `theme/metrics.rs` |
| Fonts | telaio, `src/fonts.rs` |
| Theme registration, live settings apply | `theme/mod.rs` (through `telaio::install`, `telaio::apply`) |
| Standard widgets | telaio, `src/components/` |
| Domain widgets (method badge, URL bar, env pill, ...) | `views/components/` (see the gallery: `POSTINO_OPEN=components`) |
| Environment dot color rule | `state/env_color.rs` |

## Rules

- Colors only through `cx.palette()` (a `Palette`), never from `cx.theme()` and never as a literal.
- Sizes only from `theme::metrics`, never as a literal.
- The two themes ("Postino Light", "Postino Dark") are built from the same token list. The hex
  literals are written once: the shared tokens in telaio's `Tokens::DEFAULT_LIGHT` and
  `DEFAULT_DARK`, the method colors in `LIGHT` and `DARK` in `theme/palette.rs`. Add a token
  there, not in a second place.
- Panels are flat (border only). Inputs and cards use `raised` plus a border. Menus, popovers and
  modals use `overlay` plus the shadow token. No shadows on fixed panels.
- A single-line input's text sits 9 px from its border (`INPUT_PADDING_X`, 1 px less than
  gpui-component's default), and so does the text of a `VariableField`'s row of chips, so
  focusing a field does not move its text. The URL bar keeps its own 8 px.
- Every text input hovers like the search trigger: a `border` border at rest and `border_strong`
  while the pointer is over it, then its own focus look (`accent`) once focused. An `Input` that
  draws its own border goes through telaio's `HoverInput`, since
  gpui-component's `Input` has no hover style; one whose border is drawn by a container (URL bar,
  sidebar filter, load test fields) sets `hover` on that container, which needs an `id` to
  render again when the pointer enters or leaves. Multi-line editors (body, scripts, docs) are
  not included.
- Code editors (body, scripts, docs, response) sit in a `raised` box, and their line number
  column shares that background (`editor.gutter.background`): no tinted strip, whose edge read as
  a divider under a cursor at the start of a line.
- Every clickable thing has a hover and a pressed state. `hover` and `pressed` are translucent,
  so the same token shows on `bg`, `surface`, `raised` and `overlay` (the opaque dark `hover`
  used before was invisible on `raised` and `overlay`). A gpui `.hover()` style replaces the
  background instead of layering over it: an element with an opaque resting fill (a `raised`
  control, the selected segment, the active tab, a tinted chip) hovers with
  `Palette::hover_over(fill)` and presses with `Palette::pressed_over(fill)`; one with a
  transparent resting fill uses `hover` and `pressed` as they are.
- A gpui-kit `Button` built by hand never uses `.ghost()`: its hover is an accent tint that also
  changes the text color. telaio's `NeutralButtonExt` gives the neutral look:
  `neutral_ghost` for a borderless button (icon buttons, the workspace switcher, the method
  picker), `neutral_bordered` for a picker box (environment pill, `select_trigger`), which keeps
  its border and takes `border_strong` on hover, like a text input.
- A select (the Settings selects, `select_trigger`) hovers like a secondary button and shows an
  `accent` border while it has the focus or its list is open.
- A checkbox shows a halo on hover: 3 px around its box (`CHECKBOX_HALO_PADDING`, 22 px in all),
  radius `RADIUS_SM`, `pressed` on hover and `border_strong` while held, one step above the usual
  `hover` and `pressed`, which barely show on an area that small. A switch shows the same halo,
  pill shaped (`switch_halo`), and so does the radio of a theme card, whose ring brightens to
  `fg_subtle` with it. Hovering a theme card also brings its frame to `fg_subtle`
  (`fg_muted` while pressed); a card never dims. A checkbox drawn as a filter pill in a toolbar
  (the load test's "Only errors" and "Include errors", `filter_checkbox`) has no halo: the whole
  pill is the click target and hovers like the `neutral_bordered` filter buttons next to it.
- The accent (indigo) is for the primary action, the selected row or tab, links and focus.
  Do not use it as decoration.
- State colors are semantic: `success` green, `warning` amber, `danger` red, `info` blue. Each has
  a `*-subtle` tint used as the badge background.
- No em dash anywhere (code, comments, docs, strings).

## Palette

Hex values copied from telaio's `src/palette.rs` and, for the methods, `theme/palette.rs`.

### Surfaces, text and borders

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `bg` | `#fcfcfd` | `#15161a` | Editor and content panels |
| `surface` | `#f4f5f7` | `#1b1c21` | Sidebar, title bar, tab bars, status bar |
| `raised` | `#ffffff` | `#23252c` | Inputs, cards, code editor |
| `overlay` | `#ffffff` | `#262830` | Menus, popovers, modals (fill) |
| `scrim` | `#0a0b1073` | `#0a0b1073` | Dimmed backdrop behind a modal (same in both) |
| `border` | `#e3e5ea` | `#2c2e36` | Default separator and border |
| `border_strong` | `#cdd0d8` | `#3b3e48` | Control borders (buttons, hovered inputs) |
| `fg` | `#1b1c22` | `#e9eaee` | Primary text |
| `fg_muted` | `#5c606e` | `#a0a3ae` | Secondary text, inactive tabs |
| `fg_subtle` | `#8b8e9a` | `#6f7280` | Placeholders, icons, line numbers, counts |
| `hover` | `#1b1c2213` | `#ffffff0d` | Hover background, a translucent layer (`fg` at 7.5%, white at 5%) |

Derived at runtime (not hex literals): `pressed` is the `hover` layer at 2.2 times its alpha. `list_active` is `accent_subtle` at alpha 0.2, the selected row background gpui-component's `ListItem` draws, for rows that paint their selection themselves.
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

The code editors' colors, Postino's `SyntaxTheme` per mode in `theme/syntax.rs`, passed to
telaio with `ThemeFamily::syntax`. Each key is a tree-sitter capture; a dotted capture with no
entry of its own (`function.method`, `constant.builtin`) takes its first part's color, and
anything left out (variables, operators, punctuation) is `fg`. Every color is a palette color.
Views that draw code by hand read them with `PostinoColors::syntax_color`.

| Capture | Light | Dark | Palette color | Examples |
| --- | --- | --- | --- | --- |
| `comment` | `#5c606e` | `#a0a3ae` | `fg_muted`, italic | `// ...`, `<!-- -->` |
| `keyword` | `#7a4fd6` | `#b597f7` | PATCH purple | `const`, `return`, `=>` |
| `function` | `#1c6fb0` | `#62b3f0` | `info` | `json()`, `toBe()` |
| `property`, `attribute` | `#3f47c4` | `#a3a9ff` | `accent_text` | JSON keys, `.status`, `id=` |
| `string` | `#1d7a4f` | `#86d7a5` | green | `"text"` |
| `number`, `type` | `#9a6500` | `#e8b75a` | `warning` | `200`, class names |
| `boolean`, `constant` | `#b03a78` | `#f3a2c6` | pink | `true`, `null`, `undefined` |
| `tag` | `#cf3535` | `#f27171` | `danger` | XML tags |

The bundled Geist Mono has no italic face, so comments show upright.

### Shadow (menus, popovers, modals)

Two layers, horizontal offset always 0.

| Layer | Light | Dark |
| --- | --- | --- |
| 1 | y 12, blur 32, spread 0, `rgba(20,22,40,0.14)` | y 12, blur 32, spread 0, `rgba(0,0,0,0.45)` |
| 2 | y 0, blur 0, spread 1, `rgba(20,22,40,0.06)` | y 0, blur 0, spread 1, `rgba(255,255,255,0.04)` |

The values are CSS. gpui's `blur_radius` is the gaussian's standard deviation, half a CSS blur, so
`Palette::shadow` passes half the blur to gpui.

### Semantic mappings

- Response status (`Palette::status_colors`): 2xx `success`, 3xx `info`, 4xx `warning`, 5xx
  `danger`. Background is the matching `*-subtle`. "Not sent" or a send failure is `fg_muted` on
  `hover`.
- Environment dot (`state::env_color`), from the name, case-insensitive: contains `prod` is
  `danger`; contains `stag`, `test`, `qa` or `uat` is `warning`; anything else is `success`. "No
  environment" is a hollow ring in `fg_subtle`.
- Tests tab count: `success` when all pass, `danger` otherwise.

### Load test chart

The "Performance over time" chart of the load test results (`Palette::series_color`, in
`theme/palette.rs`) uses palette tokens where one fits and two colors of Postino's own for the
latency lines. The PDF report draws the same series with the light column. It is at least 320 px
tall (`CHART_HEIGHT`, from the design) and grows into the room the window leaves under the results;
a canvas measures that room and asks for one more frame only when it changes.

| Series | Light | Dark | Token |
| --- | --- | --- | --- |
| Requests/s (line, left axis titled `req/s`) | `#3f47c4` | `#a3a9ff` | `accent_text` |
| p50 | `#1f8f80` | `#5fc9b8` | chart token |
| p95 | `#9a6500` | `#e8b75a` | `warning` |
| p99 | `#cf5b3a` | `#f08060` | chart token |
| Baseline (compared run's p95, dashed, 60% opacity) | `#9a6500` | `#e8b75a` | same as p95 |
| Virtual users (area peaking at half the plot, no axis, labeled plateaus) | `#8b8e9a` | `#6f7280` | `fg_subtle` |
| Latency goal lines | `#cf3535` | `#f27171` | `danger` |

The Latency tab tells requests apart by color (`PostinoColors::request_color`, cycling through
six); its p50, p95 and p99 markers use the chart's series colors.

| Request | Light | Dark |
| --- | --- | --- |
| 1 | `#4f57d8` (`accent`) | `#a3a9ff` (`accent_text`) |
| 2 | `#1c78b8` | `#62b3f0` (`info`) |
| 3 | `#b0479a` | `#e58fd0` |
| 4 | `#3f8a3a` | `#8fd18a` |
| 5 | `#a8641c` | `#e0a96d` |
| 6 | `#7a4fd6` | `#b9a2ff` |

The verdict strip uses `success` / `success_subtle` for "Thresholds passed" and `danger` /
`danger_subtle` for "Not passed". KPI deltas are `success` when better and `danger` when worse. A KPI cell is at least 136 px wide
(room for a nine figure total); a delta that does not fit next to its value drops under it, and
the cells of a row stretch to the tallest one.
Status colors in the per request table's status bars and in the Requests table follow
`Palette::status_colors`; a timeout or a failure is `danger` (at 60% opacity in a status bar, so
it stands apart from a 5xx next to it).

## Typography

- UI font **Geist**, base 13 px (user setting 11 to 16). Mono font **Geist Mono**, 12.5 px in
  editors (user setting 10 to 18, 0.5 steps). Both are bundled by telaio (OFL).
- Scale: 20/600 run title, 15/600 dialog title, 13/500 buttons and active tabs, 13/400 body,
  12/400 secondary (timings, sizes, hints), 11/600 uppercase with 0.06em tracking for section
  labels ("COLLECTIONS"), mono 12.5/400 for code, URL and key-value cells, mono 10/600 for method
  labels in the tree and tabs. Status bar text is 11.5 px in `fg_subtle`.

## Metrics (`theme/metrics.rs`)

Heights (px): title bar 40, open-tabs bar 36, response tab bar 36, request inner tabs 32, URL bar
32, Send button 32, controls 30, tree row 26, menu row 28, env panel row 30, status bar 24,
segmented control inner item 22, status badge 22, env pill 26, sidebar filter 28.

Widths (px): sidebar 248 (resizable 180 to 480), activity rail 48 (buttons 34), search trigger
360, window control 40, Send button min 100, method label 34, key-value side columns 30, autocompletion
menu 340 (8 menu rows tall before it scrolls; the script editors use gpui-kit's own completion
menu, as wide as its rows up to 340).

Icon buttons (px): 24 small, 28 large, 22 in sidebar section headers. Status bar shortcuts button
20.

Response Headers tab (px): a card like telaio's data table, title row 32 (column labels in the UI
font at 11.5/500 `fg_subtle`, the copy all button at its right), then one row per header with 7
vertical and 16 horizontal padding, name column 220, column gap 16, mono 12.5 on a 20 line
height, name in `fg_muted` and value in `fg`. Values wrap.

The welcome tab has its own `WELCOME_*` constants, listed in its section below.

Radii (px): 4 chips and checkboxes (`RADIUS_XS`), 6 segmented items, tree rows and badges
(`RADIUS_SM`), 8 inputs, buttons and editor cards (`RADIUS_MD`), 10 menus, popovers and dashboard
cards (`RADIUS_LG`), 14 modals (`RADIUS_MODAL`). gpui-kit's own widgets get 8 general and 14 large.

## Layout, top to bottom

1. **Title bar** (`surface`, bottom border): "P" logo square (18 px, radius 5, `accent`),
   "Postino" (600), 1x16 separator, workspace switcher (folder icon, folder name, chevron),
   centered 360 px search trigger ("Search requests and actions", `Ctrl K` hint; `raised` with a
   `border` border and `fg_subtle` text, `border_strong` border, `fg_muted` text and a
   `border_strong` hint on hover, and the focused input look while pressed: `accent` border and a
   3 px `accent_subtle` ring), environment
   pill (dot, name, chevron, on `raised` with border), "Import" ghost button, settings gear,
   window controls. On Linux the app draws its own chrome (client decorations).
2. **Activity rail** (48 px) left of the **sidebar** (`surface`, right border). Both are hidden
   while no workspace is open and none is being opened, so the main area takes the whole width
   (the welcome tab, below). The sidebar has: header
   "COLLECTIONS" with new request and new folder icon buttons, a filter input with the
   `list-filter` icon 8 px before its text, like the search trigger, the tree (chevron for folders, 34 px method label for requests; selected
   row `accent_subtle` background with `accent_text` label, hover `hover`), and a footer with a
   top border: `hard-drive` icon and the `~`-shortened workspace path, then `git-branch` and the
   branch name (omitted outside a git repo).
3. **Open-tabs bar** (`surface`): icon (optional: `gauge` for load tests and `house` for the
   welcome tab, `fg_subtle` unless said otherwise, `accent_text` for the house), method label
   (requests only), name, then a filled 8 px dot when dirty or an `x` when clean (hovering a dirty
   tab shows the `x`). Active tab: `bg` background, 2 px `accent` line on top, `fg` text.
   Inactive: transparent, `fg_muted`. A "+" after the last tab, only with a workspace open (a new
   request needs one). It is telaio's `DocumentTabs`: when the tabs do not fit, they scroll
   horizontally (the wheel too), the "+" stays at the edge of the visible tabs, and on the right
   a left arrow, a right arrow and a list of every open tab appear. A welcome tab, an environment
   editor or a load test fills the area below the bar in place of items 4 and 5.
4. **Request pane**: URL bar (method selector and URL joined in one bordered `raised` box, method
   in its color, variables shown as chips) plus the primary Send button with its `Ctrl ↵` hint
   (`Cmd ↵` on macOS). Underline tabs: Params, Headers, Auth, Body, Pre-request, Post-response, Docs,
   with counts in `fg_subtle`. Body tab: segmented control (JSON, Text, XML, Form, None) on the
   left, a "Format" ghost button on the right, then the code editor in a bordered `raised` box
   with line numbers. The Params, Headers and Form tables have 34 px rows with 26 px fields
   (input or chips), 4 px clear of the row lines above and below and 4 px apart: a focused cell
   gets the 1 px `accent` border without the 3 px ring, which would spill over the rows around it.
   Each row ends with a 24 px `IconButton` (`delete` icon, "Delete" tooltip) centered in its
   30 px column.
   Their Key and Value headers sort the rows on click (`fg` on hover), with an `arrow-up` or
   `arrow-down` icon after the sorted one. Auth tab: one column up to 560 px wide, centered in
   the tab, with the segmented control (None, Basic, Bearer, API key) centered at its top and
   the same 8 px vertical padding as the Body tab's, then rows, gap 8:
   a 96 px label in `fg_muted` and the field (the same chips and focus border as a table cell,
   the password, token and API key value masked, with a 20 px `eye` / `eye-off` icon button at
   their end), a Header/Query param segmented control for an API key, and a warning
   `InlineMessage` when a header or param written by hand is sent instead.
5. **Response pane**: tab bar Body, Headers (count), Tests (`passed/total`), Console (count); on
   the right the status badge, time and size. Below, a Pretty/Raw segmented control on the left
   and `copy` and `search` icon buttons on the right, then the body with line numbers in a
   bordered `raised` box. The body tab has the same 8 px inset as the request pane's content, so
   both boxes line up.
6. **Status bar** (24 px, `surface`, top border): `hard-drive` icon and the workspace path (a
   click opens the folder in the file manager), the git branch, a spacer, "Unsaved changes" when
   the active tab is dirty, "UTF-8".

Every dialog (Settings, Keyboard shortcuts, Code, New workspace, the name and confirmation
dialogs, the command palette) is a modal card drawn by `components::modal`: `overlay` fill, radius
14, a 1 px `border` border and the shadow token, `scrim` behind. gpui-component's `Dialog` box
around it is transparent and larger than the card, so the shadow has room to show
(`docs/gpui-notes.md`, "Dialogs"). A dialog has a 52 px header (title 15/600 and a close icon
button, bottom border), a body with 20 px padding and, when it has buttons, a 52 px footer with
a top border and the buttons at the end. A child that paints its own background into the card's
corners rounds them with `MODAL_INNER_RADIUS` (13, inside the border).

Settings, Appearance, Theme (and the welcome tab): three `ThemeCard`s, each a 92 px frame (radius 10, `surface`, 1 px
`border_strong`; selected: `accent_subtle`, 2 px `accent`) with 8 px of padding around a mini
window (radius 6, 1 px `border_strong`) whose left half is a sidebar and right half an editor, in
the colors of the theme the card stands for.

## Welcome tab

`views/welcome.rs` renders it and `state/welcome.rs` decides when it opens and reads the recent
workspaces (once, when the tab opens, never in `render`). It is a tab of its own kind
(`TabKind::Welcome`), never dirty, with a `house` icon in `accent_text` and the label "Welcome".
Why it exists and when it opens: GitHub #90.

**Page.** `bg` fill, scrolls vertically, and sideways when the tab is narrower than the column's
least width, 640 (`WELCOME_MIN_WIDTH`), so nothing is cut; both scrollbars show while scrolling.
One column, centered horizontally, `100%` wide up to a maximum and stretched to the tab's height,
so the footer sits at the bottom edge when the content is short. Line height 1.25 (gpui's default is taller and spreads the blocks apart). Side padding 40
and bottom padding 32 in both cases. Which case applies depends on whether the sidebar is shown (a
workspace is open or being opened), not on the window width.

| | No workspace (no rail, no sidebar) | Next to the sidebar |
| --- | --- | --- |
| Column max width | 820 (`WELCOME_COLUMN_WIDTH`) | 760 (`WELCOME_COLUMN_WIDTH_NARROW`) |
| Top padding | 72 (`WELCOME_TOP_PADDING`) | 56 (`WELCOME_TOP_PADDING_NARROW`) |

Top to bottom (px unless said otherwise):

1. **Header.** Row, items centered, gap 16. A logo square of 48 (`WELCOME_LOGO_SIZE`), radius 12
   (`WELCOME_LOGO_RADIUS`), `accent` fill, with a "P" in 24/700 `accent_fg`. Beside it, a column
   with gap 4: "Postino" in 26/600 `fg`, and the tagline in 14 `fg_muted`.
2. **Files card.** Margin top 28. `surface` fill, 1 `border`, radius 10 (`RADIUS_LG`), padding 14
   vertical and 16 horizontal, children spaced 12 apart. First an intro line in the base size
   (13) `fg`. Then a wrapping row, items centered, gap 10, text 12.5 `fg_muted`, with three items
   separated by a `·` in `fg_subtle` at 60% opacity. Each item (never wraps inside) is gap 6: a
   13 Lucide icon in `fg_subtle`, a word in the mono font, and "= meaning". The words are `fg`,
   except `.postino`, which is `accent_text`. The items are `folder` icon "folder = workspace",
   `folder-tree` "subfolder = collection" and `file-text` ".postino = request".
3. **Two columns.** Margin top 36, items aligned to the top, gap 40, widths 1 to 1.3 (flex grow
   1 and 1.3 from a zero basis, both allowed to shrink to nothing). Each column is a stack with gap
   6, headed by a `SectionLabel` ("Start", "Recent") padded 0 10 6.
   - **Start rows.** Three rows. A row is 34 high (`WELCOME_ROW_HEIGHT`), padding 0 10, radius 7
     (`RADIUS_MD` minus 1), `hover` background on hover and `pressed` while held, pointer cursor.
     Gap 10: a 15 icon in `accent_text`, the label in `fg` (grows, truncates), and an optional key
     hint in the mono font, 11, `fg_subtle`. The rows are `plus` "New workspace...", `folder-open`
     "Open folder..." with the hint "Ctrl+O" ("Cmd+O" on macOS) and `download` "Import from
     Postman...".
   - **Recent rows.** Up to four (`RECENT_LIMIT`), same row box, gap 12: the folder name in `fg`, a
     fixed 80 wide (`WELCOME_RECENT_NAME_WIDTH`) and truncated, then the path in the mono font, 12,
     `fg_subtle`, which takes the rest and truncates with an ellipsis. The path is `~`-shortened
     and elided in the middle to at most 44 characters. With no recent workspace, the column is
     left empty (no heading) and Start keeps its width.
4. **Theme.** Margin top 36, a stack with gap 6: a `SectionLabel` ("Theme") padded 0 10 6 like
   the column headings, then a row with gap 12 of the three `ThemeCard`s (System, Light, Dark),
   each taking a third of the column, the same component and size as Settings, Appearance. A
   click applies and saves the theme. Below Start and Recent on purpose: creating or opening a
   workspace stays the first thing on the page, and the theme fills the space above the footer.
   On a 760 high window without a workspace, the footer then needs a scroll.
5. **Spacer.** Grows to push the footer down, at least 24 high.
6. **Footer.** Top border 1 `border`, padding top 16, text 12.5, wrapping row, items centered,
   gap 16.
   - Links, gap 10, separated by the same faint `·`: "Quick start", "Shortcuts" with "(F1)" and
     "Search" with "(Ctrl+K)". Text `accent_text`, `fg` on hover, no underline, pointer cursor. The
     hint in parentheses is mono, 11, in the link's color, and follows the label with a gap of 4.
     On macOS the hints read "(Cmd+Shift+/)" and "(Cmd+K)".
   - A flexible gap, then telaio's `labelled_checkbox` "Show the welcome tab when Postino
     starts", with the standard checkbox halo on hover over the box or the label. Clicking either
     flips `show_welcome`, the same setting as Settings, Appearance, Interface.

**Empty main area.** With no workspace and no tab (the welcome tab was closed), the area is `bg`
with a centered column, gap 16: "Create a workspace or open a folder to get started." in
`fg_muted`, then a row with gap 8 of a primary button with a `plus` icon ("New workspace...") and
a secondary button with a `folder-open` icon ("Open folder..."). The mockup did not cover it.

Where the code differs from `Welcome.dc.html`:

- The mockup sets the "Postino" title at -0.01em letter spacing; gpui's text style has no letter
  spacing, so it renders without it (as the 0.06em of `SectionLabel`).
- The mockup's column headings are 11/500 with 0.04em tracking; the shared `SectionLabel` (11/600,
  `fg_subtle`) is reused instead of a second style.
- The mockup only styles `hover` on the rows; the code adds `pressed`.
- The mockup draws the `·` separators and the unchecked box border in a fixed `#4a4d57`; the
  code uses `fg_subtle` at 60% and `border_strong`, so both themes follow the palette.
- The mockup is static: it shows a hover state only, a checkbox that toggles locally, and Spanish
  text. The strings are in `locales/welcome.yml` in four languages.

## Components (`views/components/`)

`buttons` (primary, secondary, danger, ghost), `card`, `document_tabs`, `edit_menu`, `env_pill`,
`icon_button`, `inline_message`, `key_value_table`, `method_badge`, `section_label`,
`segmented_control`, `select_trigger`, `status_badge`, `switch_halo`, `text_field`, `theme_card`,
`underline_tabs`, `url_bar`, `variable_chip`, plus `modal` (the frame every dialog shares).
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

The welcome tab mockup (`Welcome.dc.html`, 2026-10-08) is implemented and not kept, like the
others: the Welcome tab section above is its record.

Decisions taken in the redesign: the main screen is the "flat panes" direction (request above,
response below, panels separated by borders, no shadows); the other direction explored (a
boxed layout with its own error panel) was discarded. Errors and warnings are `InlineMessage`
strips (warning, danger, success, with a "Define" action on unknown variables), never a
full-width red banner. OAuth 2.0 had no screen designed and is tracked as issue #15. The Auth
tab (#14) was designed in code after the Body tab.
