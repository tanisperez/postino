# Releasing Postino

Publishing a version means pushing a tag. The `Release` workflow
(`.github/workflows/release.yml`) builds Postino for every platform, attaches the binaries to the
GitHub release of that tag and publishes it. The full plan, including packages, distribution
channels and the in-app updater, is in `plans/releases.md`.

## Versions and tags

- The version lives in one place: `version` under `[workspace.package]` in `Cargo.toml`.
  Versions follow SemVer.
- The tag is `v` plus that version: `v0.1.0`. The workflow fails if they differ.
- A tag with a pre-release suffix (`v0.1.0-rc.1`, `v0.2.0-beta.2`) is accepted when the part
  before the suffix matches the version (or when the version itself has the same suffix). It is
  published as a GitHub pre-release: built and attached like any release, but never marked as
  the latest one.

## Steps

The work of a version is tracked by its GitHub milestone (`gh issue list --milestone X.Y.Z`).

1. Check that every issue of the milestone is closed and `make format`, `make lint` and
   `make test` pass on `main`.
2. Bump `version` in `Cargo.toml`, run `cargo check` so `Cargo.lock` follows, and commit.
3. Tag and push:

   ```
   git tag v0.1.0
   git push origin main v0.1.0
   ```

4. Create the release as a **draft**, with the notes in English Markdown (what changes and which
   milestone issues went in):

   ```
   gh release create v0.1.0 --draft --verify-tag --title v0.1.0 --notes-file notes.md
   ```

5. Wait for the `Release` workflow (`gh run watch`). It uploads the assets to the draft and then
   publishes it. If the draft does not exist yet when the builds finish, the workflow creates one
   with placeholder notes; edit them afterwards with `gh release edit v0.1.0 --notes-file notes.md`.

The release stays a draft until every asset is uploaded, so nobody sees a half-built release.

## Assets

| Asset | Content |
|---|---|
| `postino-X.Y.Z-linux-x86_64.tar.gz` | Linux binary plus desktop entry, MIME type and icons in a `/usr`-like layout |
| `postino_X.Y.Z_amd64.deb` | Debian and Ubuntu package |
| `postino-X.Y.Z-1.x86_64.rpm` | Fedora and openSUSE package |
| `Postino-X.Y.Z-macos-arm64.dmg` | macOS app, Apple silicon, ad-hoc signed |
| `Postino-X.Y.Z-macos-arm64.app.tar.gz` | The same app, for the in-app updater |
| `Postino-X.Y.Z-windows-x86_64-setup.exe` | Per-user Windows installer |
| `SHA256SUMS` | SHA-256 of every asset above |

Linux binaries are built on Ubuntu 22.04 (glibc 2.35). In the deb and rpm a pre-release version
uses a tilde (`0.1.0~rc.1`) so it sorts before the final release; the other names keep the dash.

The packaging scripts live in `packaging/` (one README per platform) and run in the workflow.
`make dist` builds the packages for the current OS locally into `target/dist` (Linux needs
`nfpm` on `PATH`, macOS the Xcode command line tools). The Windows installer is compiled with
Inno Setup, see `packaging/windows/README.md`.

## When something fails

- **Version mismatch**: delete the tag (`git push origin :refs/tags/v0.1.0`, `git tag -d v0.1.0`),
  fix `Cargo.toml`, commit and tag again.
- **A build fails**: fix it on `main`, then move the tag to the fixed commit (delete and push it
  again). The draft keeps its notes; re-running the workflow replaces the assets (`--clobber`).
- **Only the publish job failed**: re-run the failed jobs from the Actions page.
