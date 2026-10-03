#!/bin/bash
# Regenerates every website screenshot into site/img/ (WebP, light and dark).
# See site/screenshots/README.md for the requirements.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BIN="$ROOT/target/debug/postino"
OUT="$ROOT/site/img"
PORT=8099
SCALE=1.5
WORK="$(mktemp -d)"
# The app runs with HOME pointing here, so the paths it shows (sidebar footer, Settings) read
# "~/projects/bookshelf-api" and "~/.config/postino" instead of a temporary directory.
HOME_DIR="$WORK/home"
WORKSPACE="$HOME_DIR/projects/bookshelf-api"
MOCK_PID=""

cleanup() {
    pkill -x postino 2>/dev/null
    [ -n "$MOCK_PID" ] && kill "$MOCK_PID" 2>/dev/null
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

die() { echo "capture.sh: $*" >&2; exit 1; }

for tool in spectacle xdotool magick python3 git; do
    command -v "$tool" >/dev/null || die "missing required tool: $tool"
done
if [ "$(busctl --user call org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver GetActive 2>/dev/null)" = "b true" ]; then
    die "the screen is locked, unlock it first"
fi
pgrep -x postino >/dev/null && die "a postino process is already running, close it first"

if [ ! -x "$BIN" ]; then
    (cd "$ROOT" && cargo build) || die "cargo build failed"
fi

mkdir -p "$OUT"
python3 "$HERE/mock_server.py" "$PORT" &
MOCK_PID=$!
sleep 1
kill -0 "$MOCK_PID" 2>/dev/null || die "the mock server could not start (port $PORT busy?)"

WID=""

# Copies the demo workspace to a fresh git repository on branch main.
fresh_workspace() {
    rm -rf "$WORKSPACE"
    mkdir -p "$(dirname "$WORKSPACE")"
    cp -r "$HERE/workspace" "$WORKSPACE"
    (cd "$WORKSPACE" && git init -q -b main && git add -A \
        && git -c user.name=Demo -c user.email=demo@example.com commit -qm "Demo workspace")
}

# launch THEME [VAR=value ...]: starts Postino with an isolated config and waits for it.
launch() {
    local theme="$1"; shift
    mkdir -p "$HOME_DIR/.config/postino" "$HOME_DIR/.local/state"
    printf 'theme = "%s"\nlanguage = "en"\n' "$theme" > "$HOME_DIR/.config/postino/settings.toml"
    fresh_workspace
    env WAYLAND_DISPLAY= HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/.config" \
        XDG_STATE_HOME="$HOME_DIR/.local/state" \
        GPUI_X11_SCALE_FACTOR=$SCALE POSTINO_ENV=local "$@" \
        "$BIN" "$WORKSPACE" >"$WORK/app.log" 2>&1 &
    local i
    for i in $(seq 1 40); do
        WID=$(xdotool search --pid "$(pgrep -x postino | head -1)" 2>/dev/null | tail -1)
        [ -n "$WID" ] && break
        sleep 0.5
    done
    [ -n "$WID" ] || die "the postino window never appeared"
    sleep 7
}

quit_app() {
    pkill -x postino
    local i
    for i in $(seq 1 20); do pgrep -x postino >/dev/null || break; sleep 0.25; done
    WID=""
}

# Keystrokes and clicks must never reach another application.
ensure_active() {
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] && return 0
    xdotool windowactivate --sync "$WID"; sleep 0.4
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] || die "Postino lost the focus, aborting"
}

# X11 pointer requests are scaled by 1.1 by the compositor here, and the spectacle image is
# the window geometry divided by 1.1, so a screenshot pixel (PX, PY) is requested at
# (window origin + P * 1.1) / 1.1.
pointer() {
    eval "$(xdotool getwindowgeometry --shell "$WID")"
    local rx ry
    rx=$(python3 -c "print(round(($X+$1*1.1)/1.1))")
    ry=$(python3 -c "print(round(($Y+$2*1.1)/1.1))")
    ensure_active
    xdotool mousemove $((rx-2)) $((ry-2)); sleep 0.2
    xdotool mousemove "$rx" "$ry"; sleep 0.3
    ensure_active
}

click() { pointer "$1" "$2"; xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.8; }

type_into() { click "$1" "$2"; ensure_active; xdotool key ctrl+a; sleep 0.2; xdotool type --delay 80 "$3"; sleep 0.3; }

# Parks the pointer on an empty part of the sidebar so no hover state is captured.
park() { pointer 200 900; }

# shot NAME THEME: captures the active window into site/img/NAME-THEME.webp
shot() {
    local name="$1" theme="$2"
    park
    sleep 0.6
    ensure_active
    spectacle -b -n -a -o "$WORK/$name.png" || die "spectacle failed"
    magick "$WORK/$name.png" -quality 85 -define webp:method=6 -define webp:alpha-quality=90 \
        "$OUT/$name-$theme.webp" || die "magick failed (no WebP support?)"
    echo "wrote $OUT/$name-$theme.webp"
}

for theme in light dark; do
    # hero and scripts share one session
    launch "$theme" POSTINO_AUTOSEND=books/list.postino
    shot hero "$theme"
    click 904 225      # Post-response tab of the request editor
    click 638 729      # Tests tab of the response pane
    shot scripts "$theme"
    quit_app

    launch "$theme" POSTINO_AUTOSEND=books/list.postino POSTINO_OPEN=palette
    ensure_active; xdotool type --delay 120 "book"; sleep 0.8
    shot palette "$theme"
    quit_app

    launch "$theme" POSTINO_AUTOSEND=books/list.postino POSTINO_OPEN=snippet
    shot snippet "$theme"
    quit_app

    launch "$theme" POSTINO_AUTOSEND=books/list.postino POSTINO_OPEN=settings
    shot settings "$theme"
    quit_app

    # load test: pick books/list, 25 VUs for 10 s, then wait for the run to finish
    launch "$theme" POSTINO_OPEN=loadtest
    click 611 254; click 494 488
    type_into 586 352 25; type_into 586 449 10; type_into 586 547 3; type_into 586 644 20
    click 611 1106
    sleep 15
    click 1400 1150    # blur the focused input so no text cursor is captured
    shot loadtest "$theme"
    quit_app
done

echo "done"
