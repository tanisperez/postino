# Website screenshots

`capture.sh` regenerates every screenshot of the website into `site/img/` as WebP, one light and
one dark version of each: `hero`, `scripts`, `palette`, `snippet`, `settings`,
`environments` and `loadtest`. Run it from anywhere, or with `make screenshots`.

## What it does

- Starts `mock_server.py` (a "Bookshelf API" on `127.0.0.1:8099`, Python standard library only).
- Copies `workspace/` (the demo workspace) to a temporary folder, as a git repository on `main`.
  The app runs with `HOME` set to that folder, so the paths it shows read `~/projects/...`.
- Launches `target/debug/postino` (built with `cargo build` if missing) under XWayland at 1.5x
  scale, with a temporary, isolated config and state directory, so your real settings are never
  touched. The UI is forced to English.
- Drives the app with the `POSTINO_*` debug hooks where they exist, and with `xdotool` clicks for
  the rest (the Post-response and Tests tabs, the load test form), then captures the window with
  `spectacle` and converts it with ImageMagick.
- Stops the app and the mock server on exit, also on Ctrl+C or failure.

## Requirements

- Linux, KDE Plasma on Wayland with XWayland. The click coordinates are calibrated for a Plasma
  setup where X11 pointer coordinates are 1.1 times the screenshot pixels (see `pointer()` in the
  script); on another scale factor, adjust that function.
- `spectacle`, `xdotool`, `python3`, `git` and ImageMagick (`magick`) built with WebP support.
- An unlocked screen. Do not use the keyboard or mouse while it runs (about 3 minutes): the
  script aborts if Postino loses the focus, so keystrokes never reach another application.

## Files

- `workspace/`: the demo workspace (`.postino` requests and `environments/*.env`).
- `mock_server.py`: the API behind it. Run it by hand with `python3 mock_server.py [port]`.
- `capture.sh`: the pipeline.
