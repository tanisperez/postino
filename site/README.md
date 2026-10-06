# Postino website

The landing page, install page and user documentation served at <https://postino.tanis.codes>.
Plain HTML, CSS and JavaScript, no dependencies and no build step. How it is deployed:
`docs/releasing.md`.

- `index.html`, `install.html`: the pages.
- `docs/`: the user documentation, one page per topic. Every page repeats the same header,
  sidebar, footer and search dialog, so adding, renaming or reordering a page means updating the
  sidebar of every page and the Previous/Next links of its neighbours. Headings that should be
  linkable (`h2`, `h3`) need an `id`. Code blocks are `<div class="code"><pre data-lang="…">`
  with `postino`, `js`, `json`, `env`, `toml` or `shell`, highlighted by `js/docs.js`.
- `css/docs.css`: the documentation layout (navigation, article, "On this page"), callouts,
  tables and the search dialog, on top of `style.css`.
- `js/docs.js`: search, the "On this page" list, heading anchors, copy buttons, syntax
  highlighting and the mobile menu. The search index is built in the browser the first time the
  search opens, by fetching every page linked from the sidebar, so it never goes stale. Without
  JavaScript the pages still read fine.
- `css/style.css`: every color, radius and font comes from the Postino design system
  (`docs/design-system.md`, `crates/postino-app/src/theme/palette.rs`). Each color token is a
  `light-dark()` pair, picked by `color-scheme`. Every screenshot is in the page twice, `.light`
  and `.dark`, and CSS shows the one for the active theme.
- `js/theme.js`: follows the system theme until the visitor picks one with the
  header button, keeps that choice in `localStorage` and sets `data-theme` on `<html>`. Without
  JavaScript the page still follows the system and the button is hidden.
- `fonts/`: Geist and Geist Mono (SIL OFL 1.1, `fonts/OFL.txt`), subset to Latin and converted to
  WOFF2 from the TTFs in `crates/postino-app/assets/fonts/` with `pyftsubset`.
- `img/`: the logo (`packaging/icons/`) and the app screenshots, WebP only.
- `screenshots/`: the tooling that regenerates `img/*-light.webp` and `img/*-dark.webp`
  (`make screenshots`). It is not published.

Preview locally with `python3 -m http.server -d site` and open <http://localhost:8000>.

Deployed by `.github/workflows/website.yml` on every push to `main` that touches `site/`.
