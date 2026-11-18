#!/usr/bin/env python3
"""Points the download links of the published site at the files of one release. Standard
library only. Run by the website workflow on the assembled site, never on site/ itself:

    python3 site/docs-src/release_links.py SITE_DIR TAG

The pages in git hold `…` where the version goes, and link to the latest release page, so they
work as they are in a local preview or when no release exists yet. For TAG (`v1.0.1`), in
index.html and install.html:

- An element with `href` or `value` followed by `data-asset="Postino-…-macos-arm64.dmg"` gets
  the direct download address of that file, and `data-asset` the real name.
- `<code data-file>Postino-…-macos-arm64.dmg</code>` shows the real file name.
- An element with `data-version hidden` is shown, with `…` replaced by the version.

It fails when a page has none of these, so a renamed attribute never goes unnoticed.
"""
import re
import sys
from pathlib import Path

PAGES = ["index.html", "install.html"]
DOWNLOAD = "https://github.com/tanisperez/postino/releases/download/{tag}/{name}"

ASSET = re.compile(r'(href|value)="[^"]*" data-asset="([^"]*)"')
FILE = re.compile(r"(<code data-file>)([^<]*)(</code>)")
VERSION = re.compile(r"<(\w+)([^>]*?) data-version hidden>(.*?)</\1>", re.DOTALL)


def fill(text, tag):
    version = tag.removeprefix("v")

    def asset(m):
        name = m.group(2).replace("…", version)
        url = DOWNLOAD.format(tag=tag, name=name)
        return f'{m.group(1)}="{url}" data-asset="{name}"'

    text, assets = ASSET.subn(asset, text)
    text = FILE.sub(lambda m: m.group(1) + m.group(2).replace("…", version) + m.group(3), text)
    text, versions = VERSION.subn(
        lambda m: f"<{m.group(1)}{m.group(2)} data-version>"
        + m.group(3).replace("…", version)
        + f"</{m.group(1)}>",
        text,
    )
    return text, assets + versions


def main():
    if len(sys.argv) != 3 or not re.fullmatch(r"v\d+\.\d+\.\d+", sys.argv[2]):
        sys.exit("usage: release_links.py SITE_DIR vX.Y.Z")
    site, tag = Path(sys.argv[1]), sys.argv[2]
    for page in PAGES:
        path = site / page
        text, count = fill(path.read_text(encoding="utf-8"), tag)
        if count == 0:
            sys.exit(f"{path}: no data-asset or data-version to fill in")
        path.write_text(text, encoding="utf-8")
        print(f"{page}: {count} links and versions for {tag}")


if __name__ == "__main__":
    main()
