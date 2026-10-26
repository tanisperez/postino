# Website screenshots

`capture.sh` regenerates every screenshot of the website into `site/img/` as WebP, one light and
one dark version of each: `hero`, `scripts`, `palette`, `snippet`, `settings`, `environments`,
`loadtest`, `loadtest-latency` and `loadtest-export` (a crop around the Export menu), plus
`report.webp`, the top of the first page of a PDF report (light only, like the report). Run it
from anywhere, or with `make screenshots`. Give step names to run only those, for example
`capture.sh loadtest` (steps: `hero`, which also takes `scripts`, `palette`, `snippet`,
`settings`, `environments`, `loadtest`), and `THEMES=dark` to take one theme.

## What it does

- Starts `mock_server.py` (a "Bookshelf API" on `127.0.0.1:8099`, Python standard library only).
- Copies `workspace/` (the demo workspace) to a temporary folder, as a git repository on `main`.
  The app runs with `HOME` set to that folder, so the paths it shows read `~/projects/...`.
- Launches `target/debug/postino` (built with `cargo build` if missing; `POSTINO_BIN` picks
  another binary) under XWayland at 1.5x scale, with a temporary, isolated config and state
  directory, so your real settings are never touched. The UI is forced to English.
- Drives the app with the `POSTINO_*` debug hooks where they exist, and with `xdotool` clicks for
  the rest (the Post-response and Tests tabs, the load test form and results), then captures the
  window with `spectacle` and converts it with ImageMagick.
- The load test step runs the books folder twice (10, then 25 virtual users with a p95 goal) with
  the sidebar collapsed, for a wider view of the results. `POSTINO_EXPORT_DIR`, a hidden debug
  hook, makes the Export menu save there under the suggested name instead of opening the native
  save dialog, which scripts cannot drive; the PDF goes through `pdftoppm`.
- The mock server never removes a book on `DELETE`, so a load test of the whole folder keeps
  finding the book it just deleted.
- Stops the app and the mock server on exit, also on Ctrl+C or failure.

## Requirements

- Linux, KDE Plasma on Wayland with XWayland. The click coordinates are screenshot pixels, which
  are X11 pixels. XWayland runs at the largest display scale, read from `kscreen-doctor -o`, and
  xdotool moves are divided by it (1.25 and 1.1 have been seen).
- `spectacle`, `xdotool`, `python3`, `git`, `pdftoppm` (poppler) and ImageMagick (`magick`) built
  with WebP support.
- An unlocked screen. Do not use the keyboard or mouse while it runs (about 3 minutes): the
  script aborts if Postino loses the focus, so keystrokes never reach another application.

## Files

- `workspace/`: the demo workspace (`.postino` requests and `environments/*.env`).
- `mock_server.py`: the API behind it. Run it by hand with `python3 mock_server.py [port]`.
- `capture.sh`: the pipeline.

# Website videos

`record-video.sh` records the two videos of the home page into `site/video/`, each as `NAME.webm`
(AV1), `NAME.mp4` (H.264, for browsers without AV1) and `NAME-poster.webp`. Run it from anywhere,
or with `make video`. It takes about 3 minutes. Give a scene name to record only that one:

- `demo`: Settings (Dark), `books/list` sent, its Post-response script and Tests, then
  `books/create` opened from the command palette and sent.
- `loadtest`: in the dark theme with the sidebar collapsed, a first run of the books folder off
  camera, then on camera 25 virtual users with a ramp-up and a p95 goal, the live run, the passed
  thresholds, a window dragged on the chart, the Latency tab compared with the first run, and a
  PDF exported (saved to `~/Documents` of the fake home through `POSTINO_EXPORT_DIR`).

- Uses the `postino` of the `PATH` (`make install` or `make install-latest`), or the binary in
  `POSTINO_BIN`, such as `target/release/postino`. Not a debug build: the load test would lag.
- Starts `mock_server.py` and copies `workspace/` to `target/video/` as a git repository, with
  `HOME` pointing there, like `capture.sh`. Light theme, English.
- The window is 2176x1360 X11 pixels at 1.5x scale (about 1450x906 logical, like the
  screenshots), so the video is sharp at 1600 pixels wide and the window fits a 1440 pixels high
  display.
- Records the window with `gpu-screen-recorder` (a screen region in logical pixels, KMS capture,
  no portal dialog) while `xdotool` drives it with smooth pointer moves.
- Encodes the recording with `ffmpeg` (SVT-AV1 and x264, no audio). The poster is the last state
  of the scene, captured with `spectacle` without the pointer.

Requirements: the same as `capture.sh`, plus `postino` in the `PATH`, `gpu-screen-recorder` and
`ffmpeg` built with `libsvtav1` and `libx264`. XWayland runs at the largest display scale, read
from `kscreen-doctor -o`; the window is placed on the display at the top left corner.

# Open Graph card

`og-image.sh` renders `og.html` with headless Chrome into `site/img/og.png` (1200x630): the logo,
the tagline and `img/hero-dark.webp`, with the website's fonts. Run it again after the
screenshots change.
