#!/usr/bin/env bash
# Build the Linux AppImage: Postino-<version>-<arch>.AppImage.
#
# Usage: packaging/linux/appimage.sh <version> <path-to-postino-binary> <out-dir> [arch]
#
# <version> is the bare version (0.1.0 or 0.1.0-rc.1, no leading "v").
# [arch] is x86_64 (default) or aarch64.
# Requires appimagetool on PATH (https://github.com/AppImage/appimagetool). APPIMAGE_RUNTIME may
# name a type2 runtime file to embed instead of the one appimagetool downloads.
#
# The AppDir holds the same files as the tarball: the binary, the desktop entry, the MIME type
# and the icons. Mesa, the Vulkan loader and the Wayland and X11 client libraries are not
# bundled, the system ones are used like with the other packages.
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
    echo "usage: $0 <version> <postino-binary> <out-dir> [x86_64|aarch64]" >&2
    exit 2
fi

version=$1
binary=$2
out_dir=$3
arch=${4:-x86_64}

case "$arch" in
x86_64 | aarch64) ;;
*)
    echo "error: unsupported arch '$arch' (use x86_64 or aarch64)" >&2
    exit 2
    ;;
esac

if ! command -v appimagetool >/dev/null 2>&1; then
    echo "error: appimagetool not found on PATH, see https://github.com/AppImage/appimagetool" >&2
    exit 1
fi
if [[ ! -f $binary ]]; then
    echo "error: binary not found: $binary" >&2
    exit 1
fi

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
linux_dir=$root/packaging/linux
icons_dir=$root/packaging/icons
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)

app_id=codes.tanis.postino
sizes=(16 24 32 48 64 128 256 512)

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app_dir=$stage/Postino.AppDir
install -Dm755 "$binary" "$app_dir/usr/bin/postino"
install -Dm644 "$linux_dir/$app_id.desktop" "$app_dir/usr/share/applications/$app_id.desktop"
install -Dm644 "$linux_dir/$app_id.xml" "$app_dir/usr/share/mime/packages/$app_id.xml"
install -Dm644 "$icons_dir/postino.svg" \
    "$app_dir/usr/share/icons/hicolor/scalable/apps/$app_id.svg"
for size in "${sizes[@]}"; do
    install -Dm644 "$icons_dir/png/postino-$size.png" \
        "$app_dir/usr/share/icons/hicolor/${size}x${size}/apps/$app_id.png"
done

# The AppImage spec wants the desktop entry and its icon at the top of the AppDir, plus AppRun.
install -m644 "$linux_dir/$app_id.desktop" "$app_dir/$app_id.desktop"
install -m644 "$icons_dir/png/postino-256.png" "$app_dir/$app_id.png"
ln -s "$app_id.png" "$app_dir/.DirIcon"
cat >"$app_dir/AppRun" <<'EOF'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
exec "$here/usr/bin/postino" "$@"
EOF
chmod 755 "$app_dir/AppRun"

runtime=()
if [[ -n ${APPIMAGE_RUNTIME:-} ]]; then
    runtime=(--runtime-file "$APPIMAGE_RUNTIME")
fi
ARCH=$arch appimagetool --no-appstream "${runtime[@]}" "$app_dir" \
    "$out_dir/Postino-$version-$arch.AppImage"

echo "Built $out_dir/Postino-$version-$arch.AppImage"
