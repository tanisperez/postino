#!/bin/bash
# Regenerates the website screenshots into site/img/ (WebP, light and dark).
# Usage: capture.sh [STEP...], every step when none is given. Steps: hero (also scripts), palette,
# snippet, settings, environments, loadtest (also loadtest-latency, loadtest-export and report).
# THEMES="dark" limits the themes. See site/screenshots/README.md for the requirements.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# POSTINO_BIN picks the binary, target/debug/postino by default.
BIN="${POSTINO_BIN:-$ROOT/target/debug/postino}"
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

for tool in spectacle xdotool magick python3 git pdftoppm; do
    command -v "$tool" >/dev/null || die "missing required tool: $tool"
done
if [ "$(busctl --user call org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver GetActive 2>/dev/null)" = "b true" ]; then
    die "the screen is locked, unlock it first"
fi
pgrep -x postino >/dev/null && die "a postino process is already running, close it first"

if [ ! -x "$BIN" ]; then
    (cd "$ROOT" && cargo build) || die "cargo build failed"
fi

mkdir -p "$OUT" "$WORK/export"
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

# XWayland runs at the largest display scale: X11 coordinates are physical pixels times it, and
# xdotool pointer moves are divided by it again, while the spectacle image has the size of the window
# geometry. So a screenshot pixel (PX, PY) is requested at (window origin + P) / factor. The factor
# is read from kscreen-doctor, like record-video.sh: measuring it by moving the pointer to a fixed
# point and reading it back gave wrong values when that point was off every display.
PFACTOR=$(kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '/Scale:/ {print $2}' \
    | sort -g | tail -1)
PFACTOR=${PFACTOR:-1}

pointer() {
    eval "$(xdotool getwindowgeometry --shell "$WID")"
    local rx ry
    rx=$(python3 -c "print(round(($X+$1)/$PFACTOR))")
    ry=$(python3 -c "print(round(($Y+$2)/$PFACTOR))")
    ensure_active
    xdotool mousemove $((rx-2)) $((ry-2)); sleep 0.2
    xdotool mousemove "$rx" "$ry"; sleep 0.3
    ensure_active
}

click() { pointer "$1" "$2"; xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.8; }

type_into() { click "$1" "$2"; ensure_active; xdotool key ctrl+a; sleep 0.2; xdotool type --delay 80 "$3"; sleep 0.3; }

# Parks the pointer on an empty part of the sidebar so no hover state is captured.
park() { pointer 200 900; }

# shot NAME THEME [keep [CROP]]: captures the active window into site/img/NAME-THEME.webp, or only
# its CROP area (WxH+X+Y in screenshot pixels). The pointer is parked first unless "keep" is given
# (an open menu would close).
shot() {
    local name="$1" theme="$2"
    [ "${3:-}" = keep ] || park
    sleep 0.6
    ensure_active
    spectacle -b -n -a -o "$WORK/$name.png" || die "spectacle failed"
    if [ -n "${4:-}" ]; then
        magick "$WORK/$name.png" -crop "$4" +repage "$WORK/$name.png" || die "magick crop failed"
    fi
    magick "$WORK/$name.png" -quality 85 -define webp:method=6 -define webp:alpha-quality=90 \
        "$OUT/$name-$theme.webp" || die "magick failed (no WebP support?)"
    echo "wrote $OUT/$name-$theme.webp"
}

step_hero() {
    local theme="$1"
    # hero and scripts share one session
    launch "$theme" POSTINO_AUTOSEND=books/list.postino
    shot hero "$theme"
    click 1086 246      # Post-response tab of the request editor
    click 730 770    # Tests tab of the response pane
    shot scripts "$theme"
    quit_app
}

step_palette() {
    launch "$1" POSTINO_AUTOSEND=books/list.postino POSTINO_OPEN=palette
    ensure_active; xdotool type --delay 120 "book"; sleep 0.8
    shot palette "$1"
    quit_app
}

step_snippet() {
    launch "$1" POSTINO_AUTOSEND=books/list.postino POSTINO_OPEN=snippet
    shot snippet "$1"
    quit_app
}

step_settings() {
    launch "$1" POSTINO_AUTOSEND=books/list.postino POSTINO_OPEN=settings
    shot settings "$1"
    quit_app
}

step_environments() {
    # environment editor, opened from the Environments panel of the activity rail
    launch "$1" POSTINO_OPEN=env:staging
    shot environments "$1"
    quit_app
}

# run_load_test VUS DURATION RAMP_UP: fills the form of the open load test tab, starts it and
# waits for it to end.
run_load_test() {
    type_into 190 384 "$1"; type_into 400 384 "$2"
    type_into 190 482 "$3"; type_into 400 482 20
    click 312 1160
    sleep $(( $2 + 5 ))
}

step_loadtest() {
    local theme="$1"
    # Two runs of the books folder (its five requests): a first one of 10 virtual users, then 25
    # with a p95 < 250 ms threshold (the default value of the new row), compared with the first.
    # The results open on the Summary tab.
    rm -f "$WORK"/export/*
    launch "$theme" POSTINO_OPEN=loadtest POSTINO_EXPORT_DIR="$WORK/export"
    click 670 226; click 694 278; click 550 370       # Collection, the folder field, books
    click 66 126      # collapses the sidebar, for a wider view of the results
    run_load_test 10 8 2
    click 210 590; click 161 678                       # Add threshold, p95
    run_load_test 25 10 3
    click 1520 190    # blank area of the results header, blurs the input (no text cursor)
    shot loadtest "$theme"
    click 704 314     # Latency tab
    shot loadtest-latency "$theme"
    click 596 314     # back to the Summary
    click 2088 190    # Export menu
    shot loadtest-export "$theme" keep 1630x620+545+150    # the results' header and the menu
    if [ "$theme" = light ]; then
        # The PDF report, saved to POSTINO_EXPORT_DIR: the top of its first page, down to the per
        # request table (the footer would show the build). The report is always light, so there is
        # one image for both themes.
        click 1741 363    # PDF
        sleep 3
        local pdf
        pdf=$(ls "$WORK"/export/*.pdf 2>/dev/null | head -1)
        [ -n "$pdf" ] || die "the PDF report was not exported"
        pdftoppm -png -r 160 -f 1 -l 1 "$pdf" "$WORK/report" || die "pdftoppm failed"
        magick "$WORK"/report-*1.png -crop 100%x70%+0+0 +repage -quality 88 \
            -define webp:method=6 "$OUT/report.webp" \
            || die "magick failed"
        echo "wrote $OUT/report.webp"
    fi
    quit_app
}

STEPS=("$@")
[ ${#STEPS[@]} -gt 0 ] || STEPS=(hero palette snippet settings environments loadtest)
for step in "${STEPS[@]}"; do
    declare -F "step_$step" >/dev/null || die "unknown step: $step"
done
for theme in ${THEMES:-light dark}; do
    for step in "${STEPS[@]}"; do
        "step_$step" "$theme"
    done
done

echo "done"
