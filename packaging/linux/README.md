# Linux packaging

| File | Purpose |
|---|---|
| `codes.tanis.postino.desktop` | Desktop entry (`Exec=postino %f`, `MimeType=application/x-postino;`) |
| `codes.tanis.postino.xml` | shared-mime-info definition of `*.postino` (`application/x-postino`) |
| `nfpm.yaml` | nfpm config shared by the deb and the rpm |
| `package.sh` | Builds the tarball, the deb and the rpm |

## Usage

```sh
packaging/linux/package.sh <version> <path-to-postino-binary> <out-dir> [x86_64|aarch64]
# example
packaging/linux/package.sh 0.1.0-rc.1 target/release/postino dist
```

`<version>` has no leading `v`. A pre-release such as `0.1.0-rc.1` becomes `0.1.0~rc.1` in the
deb and rpm (nfpm semver handling). The optional arch defaults to `x86_64`; `aarch64` maps to
`arm64` for the deb (untested, there is no ARM build yet). Output in `<out-dir>`:

- `postino-<version>-linux-<arch>.tar.gz`: a `/usr`-like prefix (`bin/`, `share/`) plus `LICENSE`
  and `README.md`, meant to be copied into `/usr` (AUR `PKGBUILD`) or `~/.local`.
- `postino_<version>_amd64.deb` and `postino-<version>-1.x86_64.rpm` (nfpm default names).

`nfpm` must be on `PATH`. Tested with nfpm 2.47.0.

## Notes

- No maintainer scripts: dpkg triggers (shared-mime-info, desktop-file-utils, icon caches) and
  rpm file triggers refresh the MIME, desktop and icon caches on modern distributions.
- Dependencies are the runtime libraries the binary links (xcb, xkbcommon, xkbcommon-x11) or
  loads with dlopen (wayland-client, wayland-egl, EGL), checked with `ldd` and the library names
  embedded in the binary. The Vulkan loader is only recommended. Package names for deb and rpm
  are set in `nfpm.yaml`.
- nfpm only expands environment variables in content entries that set `expand: true`.
