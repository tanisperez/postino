#!/bin/bash
# Records the website demo video into site/video/ (WebM AV1, MP4 H.264 and a poster).
# See site/screenshots/README.md for the requirements.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BIN="$(command -v postino)"
PORT=8099
# Larger than the screenshots' window, at a larger scale, so the video is sharp at 1600 pixels
# wide with a layout of 1280x800 logical pixels.
SCALE=1.8
WIDTH=2304
HEIGHT=1440
WORK="$ROOT/target/video"
OUT="$ROOT/site/video"
# The app runs with HOME pointing here, so the paths it shows read "~/projects/bookshelf-api".
HOME_DIR="$WORK/home"
WORKSPACE="$HOME_DIR/projects/bookshelf-api"
MOCK_PID=""
REC_PID=""

cleanup() {
    [ -n "$REC_PID" ] && kill -INT "$REC_PID" 2>/dev/null
    pkill -x postino 2>/dev/null
    [ -n "$MOCK_PID" ] && kill "$MOCK_PID" 2>/dev/null
}
trap cleanup EXIT INT TERM

die() { echo "record-video.sh: $*" >&2; exit 1; }

[ -n "$BIN" ] || die "postino is not in the PATH (make install or make install-latest)"
for tool in gpu-screen-recorder spectacle xdotool magick ffmpeg python3 git; do
    command -v "$tool" >/dev/null || die "missing required tool: $tool"
done
if [ "$(busctl --user call org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver GetActive 2>/dev/null)" = "b true" ]; then
    die "the screen is locked, unlock it first"
fi
pgrep -x postino >/dev/null && die "a postino process is already running, close it first"

# XWayland runs at the largest display scale: X11 coordinates are physical pixels times it, and
# xdotool pointer moves are divided by it again. The screenshots and the recording are physical.
X_SCALE=$(kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '/Scale:/ {print $2}' | sort -g | tail -1)
X_SCALE=${X_SCALE:-1}

rm -rf "${WORK:?}"
mkdir -p "$HOME_DIR/.config/postino" "$HOME_DIR/.local/state" "$(dirname "$WORKSPACE")" "$OUT"
cp -r "$HERE/workspace" "$WORKSPACE"
(cd "$WORKSPACE" && git init -q -b main && git add -A \
    && git -c user.name=Demo -c user.email=demo@example.com commit -qm "Demo workspace")
printf 'theme = "light"\nlanguage = "en"\n' > "$HOME_DIR/.config/postino/settings.toml"

python3 "$HERE/mock_server.py" "$PORT" >/dev/null 2>&1 &
MOCK_PID=$!
sleep 1
kill -0 "$MOCK_PID" 2>/dev/null || die "the mock server could not start (port $PORT busy?)"

env WAYLAND_DISPLAY= HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/.config" \
    XDG_STATE_HOME="$HOME_DIR/.local/state" GPUI_X11_SCALE_FACTOR=$SCALE POSTINO_ENV=local \
    "$BIN" "$WORKSPACE" >"$WORK/app.log" 2>&1 &
WID=""
for _ in $(seq 1 40); do
    WID=$(xdotool search --pid "$(pgrep -x postino | head -1)" 2>/dev/null | tail -1)
    [ -n "$WID" ] && break
    sleep 0.5
done
[ -n "$WID" ] || die "the postino window never appeared"
sleep 2
# At (20, 20) physical pixels of the top left display.
POS=$(python3 -c "print(round(20*$X_SCALE))")
xdotool windowsize "$WID" "$WIDTH" "$HEIGHT" windowmove "$WID" "$POS" "$POS"
sleep 4

# Keystrokes and clicks must never reach another application.
ensure_active() {
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] && return 0
    xdotool windowactivate --sync "$WID"; sleep 0.4
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] || die "Postino lost the focus, aborting"
}

# glide PX PY: moves the pointer smoothly to the screenshot pixel (PX, PY) of the window.
glide() {
    ensure_active
    python3 -I - "$1" "$2" "$POS" "$X_SCALE" "$(xdotool getmouselocation --shell)" <<'EOF'
import math, subprocess, sys, time
px, py, pos, scale = float(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
loc = dict(l.split("=") for l in sys.argv[5].split())
x0, y0 = float(loc["X"]) / scale, float(loc["Y"]) / scale
x1, y1 = pos / scale + px, pos / scale + py
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

# type_into PX PY TEXT: clicks a field and replaces its text, typed key by key.
type_into() { click "$1" "$2" 0.2; ensure_active; xdotool key ctrl+a; xdotool type --delay 90 "$3"; sleep 0.3; }

# The window's opaque area in the screenshot (the window without its shadow) is the area recorded,
# with an even width and height for the encoder.
ensure_active
spectacle -b -n -a -o "$WORK/window.png" || die "spectacle failed"
read -r CW CH CX CY < <(magick "$WORK/window.png" -alpha extract -threshold 99% -format '%@' info: \
    | sed 's/[x+]/ /g')
CW=$((CW / 2 * 2))
CH=$((CH / 2 * 2))
REGION="${CW}x${CH}+$((20 + CX))+$((20 + CY))"

glide 1200 900
gpu-screen-recorder -w "$REGION" -f 60 -k h264 -q ultra -fm cfr -cursor yes \
    -o "$WORK/raw.mp4" >"$WORK/recorder.log" 2>&1 &
REC_PID=$!
sleep 1.5

click 1787 63 1.0      # Settings
click 1449 520 1.2     # Dark
click 1569 102 0.8     # closes Settings
click 178 272 0.8      # the books folder
click 228 435 0.8      # books/list
click 1866 196 2.2     # Send: 200 OK, 5 of 5 tests
click 1000 258 1.8     # its Post-response script
click 717 717 2.0      # the Tests of the response
ensure_active; xdotool key ctrl+k; sleep 0.6
xdotool type --delay 110 "create"; sleep 0.8
ensure_active; xdotool key Return; sleep 1.0
click 1866 196 2.6     # Send books/create: 201 Created, 2 of 2 tests
click 68 252 1.0       # the Load tests panel
click 228 574 1.0      # books/list as the target
type_into 640 406 25   # virtual users
type_into 640 518 8    # duration
type_into 640 630 2    # ramp-up
click 666 1044 9.6     # Start run, until it finishes
click 1400 1150 1.2    # blurs the ramp-up input, so no text cursor is left

kill -INT "$REC_PID"
wait "$REC_PID"
REC_PID=""

# The poster: the finished load test, without the pointer.
ensure_active
spectacle -b -n -a -o "$WORK/poster.png" || die "spectacle failed"

ffmpeg -v error -y -i "$WORK/raw.mp4" -vf "scale=1600:-2:flags=lanczos" -an -c:v libx264 \
    -preset slow -crf 22 -pix_fmt yuv420p -movflags +faststart "$OUT/demo.mp4" \
    || die "the MP4 could not be encoded"
ffmpeg -v error -y -i "$WORK/raw.mp4" -vf "scale=1600:-2:flags=lanczos" -an -c:v libsvtav1 \
    -preset 4 -crf 38 -pix_fmt yuv420p "$OUT/demo.webm" || die "the WebM could not be encoded"
# The poster is the same area as the video, shown until it plays (or instead of it).
magick "$WORK/poster.png" -crop "${CW}x${CH}+${CX}+${CY}" +repage -alpha off \
    -filter Lanczos -resize 1600x -quality 85 "$OUT/demo-poster.webp" || die "the poster failed"
ls -l "$OUT"
