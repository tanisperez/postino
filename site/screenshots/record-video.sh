#!/bin/bash
# Records the website videos into site/video/ (WebM AV1, MP4 H.264 and a poster each).
# Usage: record-video.sh [SCENE...], both when none is given. Scenes: demo (requests, scripts and
# the command palette) and loadtest (a load test from the form to the PDF report).
# POSTINO_BIN picks the binary, the postino of the PATH by default.
# See site/screenshots/README.md for the requirements.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BIN="${POSTINO_BIN:-$(command -v postino)}"
PORT=8099
# About the screenshots' window and scale (a layout of 1450x906 logical pixels, where the load
# test's KPIs fit one row), sharp at 1600 pixels wide, and fitting a 1440 pixels high display.
SCALE=1.5
WIDTH=2176
HEIGHT=1360
WORK="$ROOT/target/video"
OUT="$ROOT/site/video"
# The app runs with HOME pointing here, so the paths it shows read "~/projects/bookshelf-api".
HOME_DIR="$WORK/home"
WORKSPACE="$HOME_DIR/projects/bookshelf-api"
MOCK_PID=""
REC_PID=""
WID=""

cleanup() {
    [ -n "$REC_PID" ] && kill -INT "$REC_PID" 2>/dev/null
    pkill -x postino 2>/dev/null
    [ -n "$MOCK_PID" ] && kill "$MOCK_PID" 2>/dev/null
}
trap cleanup EXIT INT TERM

die() { echo "record-video.sh: $*" >&2; exit 1; }

[ -n "$BIN" ] || die "postino is not in the PATH (make install or make install-latest, or set POSTINO_BIN)"
for tool in gpu-screen-recorder spectacle xdotool magick ffmpeg python3 git; do
    command -v "$tool" >/dev/null || die "missing required tool: $tool"
done
if [ "$(busctl --user call org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver GetActive 2>/dev/null)" = "b true" ]; then
    die "the screen is locked, unlock it first"
fi
pgrep -x postino >/dev/null && die "a postino process is already running, close it first"

# XWayland runs at the largest display scale: X11 coordinates are physical pixels times it, and
# xdotool pointer moves are divided by it again. The recording region is in physical pixels.
X_SCALE=$(kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '/Scale:/ {print $2}' | sort -g | tail -1)
X_SCALE=${X_SCALE:-1}
# The window's X11 position on the top left display, (20, 20) logical pixels. X11 pixels are the
# physical pixels the screenshots and the recording use.
POS=$(python3 -c "print(round(20*$X_SCALE))")

# start_app THEME [VAR=value ...]: a fresh demo workspace and a fresh Postino, placed and sized.
# Exports are saved to ~/Documents of that HOME without asking (POSTINO_EXPORT_DIR), so the
# notification reads "Exported to ~/Documents/...".
start_app() {
    local theme="$1"; shift
    pkill -x postino 2>/dev/null; sleep 1
    rm -rf "$HOME_DIR"
    mkdir -p "$HOME_DIR/.config/postino" "$HOME_DIR/.local/state" "$(dirname "$WORKSPACE")" \
        "$OUT" "$HOME_DIR/Documents"
    cp -r "$HERE/workspace" "$WORKSPACE"
    (cd "$WORKSPACE" && git init -q -b main && git add -A \
        && git -c user.name=Demo -c user.email=demo@example.com commit -qm "Demo workspace")
    printf 'theme = "%s"\nlanguage = "en"\n' "$theme" > "$HOME_DIR/.config/postino/settings.toml"
    env WAYLAND_DISPLAY= HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/.config" \
        XDG_STATE_HOME="$HOME_DIR/.local/state" GPUI_X11_SCALE_FACTOR=$SCALE POSTINO_ENV=local \
        POSTINO_EXPORT_DIR="$HOME_DIR/Documents" "$@" "$BIN" "$WORKSPACE" >"$WORK/app.log" 2>&1 &
    WID=""
    for _ in $(seq 1 40); do
        WID=$(xdotool search --pid "$(pgrep -x postino | head -1)" 2>/dev/null | tail -1)
        [ -n "$WID" ] && break
        sleep 0.5
    done
    [ -n "$WID" ] || die "the postino window never appeared"
    sleep 2
    xdotool windowsize "$WID" "$WIDTH" "$HEIGHT" windowmove "$WID" "$POS" "$POS"
    sleep 4
}

# Keystrokes and clicks must never reach another application.
ensure_active() {
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] && return 0
    xdotool windowactivate --sync "$WID"; sleep 0.4
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] || die "Postino lost the focus, aborting"
}

# glide PX PY: moves the pointer smoothly to the screenshot pixel (PX, PY) of the window. The
# spectacle image has the size of the window's X11 geometry, so that pixel is at X11 (POS + P),
# which xdotool reaches at (POS + P) / X_SCALE.
glide() {
    ensure_active
    python3 -I - "$1" "$2" "$POS" "$X_SCALE" "$(xdotool getmouselocation --shell)" <<'EOF'
import math, subprocess, sys, time
px, py, pos, scale = float(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
loc = dict(l.split("=") for l in sys.argv[5].split())
x0, y0 = float(loc["X"]) / scale, float(loc["Y"]) / scale
x1, y1 = (pos + px) / scale, (pos + py) / scale
steps = 36
for i in range(1, steps + 1):
    t = i / steps
    e = 0.5 - math.cos(math.pi * t) / 2
    subprocess.run(["xdotool", "mousemove", str(round(x0 + (x1 - x0) * e)), str(round(y0 + (y1 - y0) * e))])
    time.sleep(0.016)
EOF
    sleep 0.15
}

press() { ensure_active; xdotool mousedown 1; sleep 0.1; xdotool mouseup 1; }

# click PX PY WAIT: glides to the pixel, clicks and waits.
click() { glide "$1" "$2"; press; sleep "$3"; }

# drag X1 Y1 X2 Y2 WAIT: presses at the first pixel, glides to the second and releases there.
drag() {
    glide "$1" "$2"; ensure_active; xdotool mousedown 1; sleep 0.2
    glide "$3" "$4"; sleep 0.1; xdotool mouseup 1; sleep "$5"
}

# type_into PX PY TEXT: clicks a field and replaces its text, typed key by key.
type_into() { click "$1" "$2" 0.2; ensure_active; xdotool key ctrl+a; xdotool type --delay 90 "$3"; sleep 0.3; }

# start_recording: records the window's opaque area (the window without its shadow). The area is
# measured on a screenshot, in X11 pixels, and gpu-screen-recorder takes logical pixels, so it is
# divided by X_SCALE, with an even width and height for the encoder.
start_recording() {
    ensure_active
    spectacle -b -n -a -o "$WORK/window.png" || die "spectacle failed"
    read -r CW CH CX CY < <(magick "$WORK/window.png" -alpha extract -threshold 99% -format '%@' info: \
        | sed 's/[x+]/ /g')
    local region
    region=$(python3 -c "s=$X_SCALE; even=lambda v: int(v / s) // 2 * 2
print(f'{even($CW)}x{even($CH)}+{round(($POS + $CX) / s)}+{round(($POS + $CY) / s)}')")
    gpu-screen-recorder -w "$region" -f 60 -k h264 -q ultra -fm cfr -cursor yes \
        -o "$WORK/raw.mp4" >"$WORK/recorder.log" 2>&1 &
    REC_PID=$!
    sleep 1.5
}

# finish NAME: stops the recording, captures the poster (the last state, without the pointer) and
# encodes site/video/NAME.mp4, NAME.webm and NAME-poster.webp.
finish() {
    local name="$1"
    kill -INT "$REC_PID"
    wait "$REC_PID"
    REC_PID=""
    glide "$PARK_X" "$PARK_Y"
    ensure_active
    spectacle -b -n -a -o "$WORK/poster.png" || die "spectacle failed"
    pkill -x postino
    ffmpeg -v error -y -i "$WORK/raw.mp4" -vf "scale=1600:-2:flags=lanczos" -an -c:v libx264 \
        -preset slow -crf 22 -pix_fmt yuv420p -movflags +faststart "$OUT/$name.mp4" \
        || die "the MP4 could not be encoded"
    ffmpeg -v error -y -i "$WORK/raw.mp4" -vf "scale=1600:-2:flags=lanczos" -an -c:v libsvtav1 \
        -preset 4 -crf 38 -pix_fmt yuv420p "$OUT/$name.webm" || die "the WebM could not be encoded"
    # The poster is the same area as the video, shown until it plays (or instead of it).
    magick "$WORK/poster.png" -crop "${CW}x${CH}+${CX}+${CY}" +repage -alpha off \
        -filter Lanczos -resize 1600x -quality 85 "$OUT/$name-poster.webp" || die "the poster failed"
}

# Where the pointer rests for the posters: the empty bottom of the sidebar.
PARK_X=200
PARK_Y=1300

# Requests, scripts and the command palette, in the light theme switched to dark.
scene_demo() {
    start_app light
    glide 1200 900
    start_recording
    click 1972 60 1.0      # Settings
    click 1520 580 1.2     # Dark
    click 1634 178 0.8     # closes Settings
    click 170 260 0.8      # the books folder
    click 220 416 0.8      # books/list
    click 2048 186 2.2     # Send: 200 OK, 5 of 5 tests
    click 1078 246 1.8     # its Post-response script
    click 720 772 2.0      # the Tests of the response
    ensure_active; xdotool key ctrl+k; sleep 0.6
    xdotool type --delay 110 "create"; sleep 0.8
    ensure_active; xdotool key Return; sleep 1.0
    click 2048 186 3.0     # Send books/create: 201 Created, 2 of 2 tests
    finish demo
}

# A load test of books/list in the dark theme. Off camera, a first run of 10 virtual users; on
# camera, 25 virtual users with a ramp-up and a p95 goal, the live run, the passed thresholds, a
# window of the chart, the latency compared with the first run and the PDF report.
scene_loadtest() {
    start_app dark POSTINO_OPEN=loadtest
    click 664 226 0.6; click 686 278 0.8; click 544 370 0.6    # Collection, the folder, books
    click 66 126 1.0                                           # collapses the sidebar
    type_into 190 384 10; type_into 400 384 5; type_into 190 482 1; type_into 400 482 20
    click 312 1164 9                                           # the first run
    glide 1300 600
    start_recording
    type_into 190 384 25; type_into 400 384 10; type_into 190 482 3
    click 210 590 0.8; click 161 678 1.0                       # Add threshold, p95 < 250 ms
    click 312 1164 12.5                                        # Start run, the live run
    click 1300 190 1.5                                         # the passed thresholds
    drag 1200 900 1600 900 2.5                                 # a window of the chart
    click 704 314 1.5                                          # Latency tab
    glide 1200 760; sleep 2.0                                  # a bar of the distribution
    click 2044 190 1.5                                         # Export menu
    click 1697 363 2.5                                         # PDF, saved
    click 596 314 2.0                                          # back to the Summary
    finish loadtest
}

SCENES=("$@")
[ ${#SCENES[@]} -gt 0 ] || SCENES=(demo loadtest)
for scene in "${SCENES[@]}"; do
    declare -F "scene_$scene" >/dev/null || die "unknown scene: $scene"
done

rm -rf "${WORK:?}"
mkdir -p "$WORK"
python3 "$HERE/mock_server.py" "$PORT" >/dev/null 2>&1 &
MOCK_PID=$!
sleep 1
kill -0 "$MOCK_PID" 2>/dev/null || die "the mock server could not start (port $PORT busy?)"

for scene in "${SCENES[@]}"; do
    "scene_$scene"
done
ls -l "$OUT"
