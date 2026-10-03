# Postino website

The landing page and install page served at <https://postino.tanis.codes>. Plain HTML and CSS,
no JavaScript and no build step. Plan in `plans/releases.md`, section 5.

- `index.html`, `install.html`: the pages.
- `css/style.css`: every color, radius and font comes from the Postino design system
  (`postino_design_system/`, `crates/postino-app/src/theme/palette.rs`). Light and dark follow
  `prefers-color-scheme`. Screenshots switch with it through `<picture>` and a `media` source.
- `fonts/`: Geist and Geist Mono (SIL OFL 1.1, `fonts/OFL.txt`), subset to Latin and converted to
  WOFF2 from the TTFs in `crates/postino-app/assets/fonts/` with `pyftsubset`.
- `img/`: the logo (`packaging/icons/`) and the app screenshots, WebP only.
- `screenshots/`: the tooling that regenerates `img/*-light.webp` and `img/*-dark.webp`
  (`make screenshots`). It is not published.

Preview locally with `python3 -m http.server -d site` and open <http://localhost:8000>.

Deployed by `.github/workflows/website.yml` on every push to `main` that touches `site/`.
