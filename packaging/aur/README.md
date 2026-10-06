# AUR package

`PKGBUILD` is the template of `postino-bin`: it installs the release tarball
(`postino-<version>-linux-x86_64.tar.gz`). Set `pkgver` to a stable version and run `updpkgsums`
before publishing. Pre-releases do not go to the AUR.

Not published yet: AUR registration was closed when this was written. Until then it can be built
from a checkout with `cd packaging/aur && makepkg -si`. Tested with the rc.5 tarball
(`pkgver` and the source name adjusted by hand): the package contains the binary, the desktop
entry, the icons, the MIME definition and the licenses.

Once there is an account: add the SSH key as a secret and push `PKGBUILD` plus `.SRCINFO`
(`makepkg --printsrcinfo`) to `ssh://aur@aur.archlinux.org/postino-bin.git` from the `distribute`
job of `release.yml` (`docs/releasing.md`).
