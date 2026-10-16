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

# Website demo video

`record-video.sh` records the demo video of the home page into `site/video/`: `demo.webm` (AV1),
`demo.mp4` (H.264, for browsers without AV1) and `demo-poster.webp`. Run it from anywhere, or with
`make video`. It takes about 2 minutes.

- Uses the `postino` of the `PATH` (`make install` or `make install-latest`), not a debug build.
- Starts `mock_server.py` and copies `workspace/` to `target/video/` as a git repository, with
  `HOME` pointing there, like `capture.sh`. Light theme, English.
- The window is 2304x1440 X11 pixels at 1.8x scale (1280x800 logical), so the video is sharp at
  1600 pixels wide.
- Records the window with `gpu-screen-recorder` (a screen region, KMS capture, no portal dialog)
  while `xdotool` drives it with smooth pointer moves: Settings (Dark), `books/list` sent, its
  Post-response script and Tests, `books/create` opened from the command palette and sent, and an
  8 second load test of `books/list` with 25 virtual users.
- Encodes the recording with `ffmpeg` (SVT-AV1 and x264, no audio). The poster is the finished
  load test, captured with `spectacle`.

Requirements: the same as `capture.sh`, plus `postino` in the `PATH`, `gpu-screen-recorder` and
`ffmpeg` built with `libsvtav1` and `libx264`. XWayland runs at the largest display scale, read
from `kscreen-doctor -o`; the window is placed on the display at the top left corner.

# Open Graph card

`og-image.sh` renders `og.html` with headless Chrome into `site/img/og.png` (1200x630): the logo,
the tagline and `img/hero-dark.webp`, with the website's fonts. Run it again after the
screenshots change.
