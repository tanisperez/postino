#!/usr/bin/env bash
# Tests site/install.sh end to end against fake releases served from file:// URLs, with a
# temporary HOME. No network, no root, nothing outside a temporary folder.
#
# Usage: packaging/linux/test-install.sh
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
script=$root/site/install.sh
app_id=codes.tanis.postino

[[ ! -e /usr/bin/postino ]] || {
    echo "skipped: /usr/bin/postino exists, the script refuses to install next to it" >&2
    exit 0
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
releases=$tmp/releases
failures=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

check() {
    local what=$1
    shift
    "$@" || fail "$what"
}

# Succeeds when the command fails.
refused() {
    ! "$@" >/dev/null 2>&1
}

# A release tarball with the layout of packaging/linux/package.sh, and a stub binary that prints
# its version. Extra files under share/ go in as given.
make_release() {
    local version=$1
    shift
    local name=postino-$version-linux-x86_64
    local top=$tmp/stage/$name dir=$releases/download/v$version
    rm -rf "$tmp/stage"
    mkdir -p "$top/bin" "$dir"
    printf '#!/bin/sh\necho "Postino %s"\n' "$version" >"$top/bin/postino"
    chmod 755 "$top/bin/postino"
    install -Dm644 "$root/packaging/linux/$app_id.desktop" "$top/share/applications/$app_id.desktop"
    install -Dm644 "$root/packaging/linux/$app_id.xml" "$top/share/mime/packages/$app_id.xml"
    install -Dm644 "$root/packaging/icons/postino.svg" \
        "$top/share/icons/hicolor/scalable/apps/$app_id.svg"
    local extra
    for extra in "$@"; do
        install -Dm644 /dev/null "$top/share/$extra"
    done
    tar -C "$tmp/stage" -czf "$dir/$name.tar.gz" "$name"
    echo "unrelated" >"$dir/postino_${version}_amd64.deb"
    (cd "$dir" && sha256sum -- * >SHA256SUMS)
}

publish_latest() {
    rm -rf "$releases/latest"
    mkdir -p "$releases/latest"
    cp -R "$releases/download/v$1" "$releases/latest/download"
}

run() {
    env -i PATH=/usr/bin:/bin HOME="$tmp/home" XDG_CONFIG_HOME="$tmp/home/.config" \
        XDG_STATE_HOME="$tmp/home/.local/state" POSTINO_RELEASES_URL="file://$releases" \
        bash "$script" "$@"
}

prefix=$tmp/home/.local
manifest=$prefix/share/postino/installed-files
old_icon=icons/hicolor/1024x1024/apps/$app_id.png
mkdir -p "$tmp/home"

make_release 0.9.0 "$old_icon"
make_release 0.9.1
publish_latest 0.9.1

# Help and argument errors.
check "--help exits 0" run --help >/dev/null
check "--help mentions --uninstall" grep -q -- --uninstall <<<"$(run --help)"
check "--purge alone is refused" refused run --purge
check "an unknown option is refused" refused run --nope

# Install the latest release.
check "install latest" run >/dev/null 2>&1
check "binary installed" test -x "$prefix/bin/postino"
check "binary is 0.9.1" grep -q 0.9.1 "$prefix/bin/postino"
check "absolute Exec" grep -q "^Exec=$prefix/bin/postino %f" \
    "$prefix/share/applications/$app_id.desktop"
check "MIME type installed" test -f "$prefix/share/mime/packages/$app_id.xml"
check "icon installed" test -f "$prefix/share/icons/hicolor/scalable/apps/$app_id.svg"
check "manifest records 0.9.1" grep -qx "# Postino 0.9.1" "$manifest"
check "manifest lists the binary" grep -qx "$prefix/bin/postino" "$manifest"

# A specific version, then an update that drops a file.
check "install --version 0.9.0" run --version 0.9.0 >/dev/null 2>&1
check "binary is 0.9.0" grep -q 0.9.0 "$prefix/bin/postino"
check "0.9.0 icon installed" test -f "$prefix/share/$old_icon"
out=$(run 2>&1) || fail "update to latest"
check "update message" grep -q "updating Postino 0.9.0 to 0.9.1" <<<"$out"
check "binary is 0.9.1 again" grep -q 0.9.1 "$prefix/bin/postino"
check "file dropped by 0.9.1 removed" test ! -e "$prefix/share/$old_icon"
check "missing version is refused" refused run --version 9.9.9

# A tarball that does not match SHA256SUMS installs nothing.
make_release 0.9.2
echo corrupt >>"$releases/download/v0.9.2/postino-0.9.2-linux-x86_64.tar.gz"
check "bad checksum is refused" refused run --version 0.9.2
check "bad checksum keeps 0.9.1" grep -q 0.9.1 "$prefix/bin/postino"

# Uninstall keeps settings and logs, --purge removes them.
mkdir -p "$tmp/home/.config/postino" "$tmp/home/.local/state/postino"
touch "$tmp/home/.config/postino/settings.toml" "$tmp/home/.local/state/postino/postino.log"
touch "$prefix/share/applications/other.desktop"
check "uninstall" run --uninstall >/dev/null
check "binary removed" test ! -e "$prefix/bin/postino"
check "manifest removed" test ! -e "$manifest"
left=$(find "$prefix" -type f -name "*postino*" ! -path "$tmp/home/.local/state/*")
check "no installed file left ($left)" test -z "$left"
check "other files kept" test -f "$prefix/share/applications/other.desktop"
check "settings kept" test -f "$tmp/home/.config/postino/settings.toml"
check "logs kept" test -f "$tmp/home/.local/state/postino/postino.log"
check "uninstall twice is fine" run --uninstall >/dev/null
check "purge" run --uninstall --purge >/dev/null
check "settings purged" test ! -e "$tmp/home/.config/postino"
check "logs purged" test ! -e "$tmp/home/.local/state/postino"

# Another prefix.
check "install --prefix" run --prefix "$tmp/opt" >/dev/null 2>&1
check "binary in the prefix" test -x "$tmp/opt/bin/postino"
check "uninstall --prefix" run --uninstall --prefix "$tmp/opt" >/dev/null 2>&1
check "prefix binary removed" test ! -e "$tmp/opt/bin/postino"

if [[ $failures -gt 0 ]]; then
    echo "$failures check(s) failed" >&2
    exit 1
fi
echo "install.sh: all checks passed"
