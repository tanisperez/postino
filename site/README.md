# Postino website

The landing page and install page served at <https://postino.tanis.codes>. Plain HTML and CSS,
no build step. How it is deployed: `docs/releasing.md`.

- `index.html`, `install.html`: the pages.
- `css/style.css`: every color, radius and font comes from the Postino design system
  (`docs/design-system.md`, `crates/postino-app/src/theme/palette.rs`). Each color token is a
  `light-dark()` pair, picked by `color-scheme`. Every screenshot is in the page twice, `.light`
  and `.dark`, and CSS shows the one for the active theme.
- `js/theme.js`: the only script. Follows the system theme until the visitor picks one with the
  header button, keeps that choice in `localStorage` and sets `data-theme` on `<html>`. Without
  JavaScript the page still follows the system and the button is hidden.
- `fonts/`: Geist and Geist Mono (SIL OFL 1.1, `fonts/OFL.txt`), subset to Latin and converted to
  WOFF2 from the TTFs in `crates/postino-app/assets/fonts/` with `pyftsubset`.
- `img/`: the logo (`packaging/icons/`) and the app screenshots, WebP only.
- `screenshots/`: the tooling that regenerates `img/*-light.webp` and `img/*-dark.webp`
  (`make screenshots`). It is not published.

Preview locally with `python3 -m http.server -d site` and open <http://localhost:8000>.

Deployed by `.github/workflows/website.yml` on every push to `main` that touches `site/`.
