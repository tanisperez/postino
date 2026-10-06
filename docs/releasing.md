# Releasing Postino

Publishing a version means pushing a tag. The `Release` workflow
(`.github/workflows/release.yml`) builds Postino for every platform, attaches the binaries to the
GitHub release of that tag and publishes it. Packages, distribution
channels, desktop integration and the in-app updater are described further down.

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
| `latest.json` | Manifest read by the in-app updater: version, release page, and URL plus SHA-256 of the macOS and Windows assets |
| `SHA256SUMS` | SHA-256 of every asset above |

Linux binaries are built on Ubuntu 22.04 (glibc 2.35). Inside the deb and rpm a pre-release
version uses a tilde (`0.1.0~rc.1`) so it sorts before the final release. Every file name keeps
the dash, since GitHub renames a `~` in an asset name to `.`.

The updater reads `latest.json` from `releases/latest/download/`, which GitHub points at the newest
stable release and never at a pre-release. To test an update between pre-releases, run the older
pre-release with `POSTINO_UPDATE_URL` set to the manifest of the newer one before launching it:

```sh
POSTINO_UPDATE_URL=https://github.com/tanisperez/postino/releases/download/vX.Y.Z-rc.N/latest.json postino
```

The app takes its own version from the workspace version in `Cargo.toml`, not from the tag. For a
test pre-release, set the full version there (`version = "0.1.0-rc.4"`) before tagging, or the
build reports `0.1.0`, which is newer than any `0.1.0-rc.N`, and the updater finds nothing. Set it
back to `0.1.0` for the final release.

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

## Why releases work this way

- **No backend.** Only free GitHub features (Actions on a public repo, Releases, Pages) plus the
  free community registries (AUR, a Homebrew tap, winget-pkgs).
- **Pre-releases** (`v0.3.0-beta.1`) are built and attached but not distributed, and are invisible
  to the updater, because `/releases/latest` skips pre-releases.
- **Trigger:** `release.yml` runs on `push: tags: ["v*"]`. `build.yml` stays as the per-push CI.
- **Draft first.** The release is created as a draft and the workflow publishes it only when
  every asset is there, so neither the updater nor the package repos see a half-built release.
- **Targets:** Linux x86_64, Windows x86_64 and macOS arm64. ARM Linux and ARM Windows can come
  later (free `ubuntu-22.04-arm` and `windows-11-arm` runners for public repos). A macOS Intel
  build only if someone asks (a universal binary via `lipo`).
- **App id:** `codes.tanis.postino`, used for the `.desktop` file, the Wayland `app_id`, the macOS
  bundle id and the Windows AppUserModelID. The domain `tanis.codes` is owned, which Flathub
  requires.
- **nfpm** builds the deb and the rpm from one YAML file, so there is no per-distro packaging
  toolchain to learn.
- Linux packages declare these dependencies: `libxkbcommon`, `libxkbcommon-x11`,
  `libwayland-client`, `libxcb`, `libvulkan` (loader), `fontconfig`, `freetype`.

The workflow, in short:

```
tag v0.3.0 pushed
  └─ check: tag == Cargo version, prerelease = tag contains "-"
      ├─ build-linux   (ubuntu-22.04)   cargo build --release, tar.gz, nfpm deb + rpm
      ├─ build-macos   (macos-latest)   cargo build --release, .app bundle, ad-hoc codesign, dmg, app.tar.gz
      └─ build-windows (windows-latest) cargo build --release, Inno Setup installer
          └─ publish: SHA256SUMS + latest.json, upload to the draft, mark it published
              └─ distribute (stable only, each job independent): website + APT + DNF repos on Pages, AUR
```

Packaging files live in `packaging/`: `linux/` (nfpm, desktop file, MIME XML), `macos/`
(`Info.plist`, bundle script), `windows/` (`postino.iss`), `aur/` (`PKGBUILD`) and `icons/`
(an SVG master and the generated PNG sizes).

## Desktop integration

Installing registers the app and the `.postino` file type:

| OS | What the package installs |
| --- | --- |
| Linux (deb, rpm, AUR) | `/usr/bin/postino`, `/usr/share/applications/codes.tanis.postino.desktop` (with `MimeType=application/x-postino;`), `/usr/share/mime/packages/codes.tanis.postino.xml` (glob `*.postino`), icons in `/usr/share/icons/hicolor/*/apps/`. The distro triggers refresh the MIME and desktop databases, nothing to script. |
| macOS | `Postino.app` with `Info.plist`: `CFBundleIdentifier`, `CFBundleDocumentTypes` and `UTExportedTypeDeclarations` for the `postino` extension, and an `.icns` icon. LaunchServices registers it when the app lands in `/Applications`. |
| Windows | Inno Setup, **per user** install (`%LOCALAPPDATA%\Programs\Postino`, no admin, so a self update never needs UAC). Start menu shortcut, optional desktop shortcut, `.postino` association under `HKCU\Software\Classes`, uninstaller in "Apps", and `CloseApplications=yes` so it can update a running Postino. |

In the app:

- A `.postino` path as the first argument opens its workspace and that request in a tab. A folder
  opens as a workspace. A file opens the remembered workspace if it contains the file, otherwise
  the file's parent folder, and then opens the file in a tab.
- macOS delivers "open with" through Apple Events, not argv, so the app hooks `cx.on_open_urls`
  and routes the `file://` URLs to the same code.
- The Wayland `app_id` is the app id, so the window matches the `.desktop` file (right icon and
  name in the dock and alt-tab).
- There is no single instance: opening a file while Postino runs starts a second process.
  Forwarding to the running instance can come later.

## Distribution channels

### Linux

| Channel | How |
| --- | --- |
| Arch (AUR) `postino-bin` | A `PKGBUILD` that downloads the release tarball and checks its sha256. `yay -Syu` updates. |
| Debian and Ubuntu | A signed APT repo on GitHub Pages (`apt-ftparchive` and GPG). Users add the key and one `.sources` file once, then `apt upgrade` updates. |
| Fedora and openSUSE | A signed DNF repo on the same site (`createrepo_c` and GPG, `rpm --addsign`). One `.repo` file, then `dnf upgrade`. |
| Everyone else | The tarball, plus the deb and rpm as direct downloads. |

The APT and DNF repos share one GPG key (private key and passphrase as secrets, public key
published on the site). There is no `gh-pages` branch: a branch holding the deb and rpm files
would make every `git clone` of Postino download them too. The site is deployed from Actions
(`actions/deploy-pages`) and rebuilt from scratch each time: the job builds the website,
downloads the deb and rpm of the last 3 stable releases from GitHub Releases, regenerates and
signs the APT and DNF metadata, and uploads the whole site. It runs after each stable release and
on pushes to `main` that touch `site/`. GitHub Pages limits: 1 GB per site, 100 MB per file,
100 GB of bandwidth per month (soft). Older versions stay downloadable from Releases. The repo
URLs use the `postino.tanis.codes` domain, so moving the hosting later (Cloudflare Pages is ruled
out for now by its 25 MB per file limit) never breaks the users' apt and dnf configuration.

Considered and left for later:

- **Flathub:** one package for all distros with updates, but it builds from source offline (every
  crate vendored in the manifest) and reviews the submission. A good second step now that the app
  id and name are final, because renaming a Flathub app is painful.
- **Ubuntu PPA, Fedora COPR, official distro repos:** they build from source on their servers
  without network, with their own review. Too much work for now.
- **AppImage:** easy to build, but no updates without extra machinery. The tarball covers it.

### macOS

- **Homebrew:** an own tap `tanisperez/homebrew-postino` with a cask pointing at the release dmg.
  The cask declares `auto_updates true`, so `brew upgrade` leaves updates to the in-app updater.
  The official `homebrew/cask` requires signed and notarized apps, so it is out of reach without
  the Apple Developer Program (see Signing). Not set up yet (#44).
- **Direct:** the dmg from Releases or from the site.
- **Updates:** in-app.

### Windows

- **winget:** a PR to `microsoft/winget-pkgs` per release. The first submission is manual and
  reviewed by Microsoft; later ones are usually merged automatically. winget reads the installed
  version from the uninstall entry the Inno installer writes, so both update paths agree. Not set
  up yet (#44).
- **Direct:** the setup exe from Releases.
- **Updates:** in-app.

## The website

`postino.tanis.codes` is the landing page, the install page and the APT and DNF repos, served by
GitHub Pages.

- Plain static HTML and CSS under `site/`, no JavaScript and no build tool, so a feature and its
  screenshot and text change in the same pull request.
- Styled with the design system (`docs/design-system.md`): its color tokens as CSS custom
  properties, light and dark through `prefers-color-scheme`, Geist and Geist Mono served from the
  site (SIL OFL 1.1).
- Pages: the landing page (features, each with a screenshot) and the install page (one download
  button per OS, then the `apt`, `dnf`, `yay`, `brew` and `winget` commands). The docs link to
  `docs/` on GitHub.
- Screenshots are generated, not taken by hand: a script opens the sample workspace, drives the
  app and saves each screen in light and dark, as WebP only, with fixed width and height to avoid
  layout shifts. They are regenerated when the UI changes visibly, not on every release.
- English only for now.

## In-app updater

The `postino-update` crate (no UI, tested against a local `tiny_http` server) plus the glue in
`state/update.rs` and `views/update.rs`. Package managers own updates on Linux, so the updater
exists only on macOS and Windows.

- **Compiled in only with `POSTINO_UPDATER=github`** at build time (`option_env!`), set only on
  the macOS `.app` and the Windows installer. Otherwise the whole feature, its setting and its
  menu entries are absent (deb, rpm, AUR and local builds).
- **Setting and command:** Settings, Advanced, "Check for updates automatically" (on by
  default), and a command palette entry "Check for updates".
- **Check:** once, 10 s after startup, on the background executor. No timer and no polling, so
  the idle rule stays intact. A window open for days learns of a new version only on the next
  start or the manual check.
- **Manifest:** `GET .../releases/latest/download/latest.json`, compared with the app's own
  version using `semver`. GitHub redirects that URL to the newest stable release: no API, no
  token, no rate limit. Network and parse errors are logged at Warn and otherwise ignored for the
  background check (the manual check does show them).
- **Download:** when a newer version exists, the asset for this platform is downloaded in the
  background and its sha256 verified. Then a discreet indicator appears in the status bar
  ("Update ready, restart") with a link to the release notes. Nothing installs without a click.
- **Install on Windows:** run the downloaded installer with
  `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /relaunch=1` and quit. The installer replaces the
  files (closing Postino through the Restart Manager if needed) and relaunches it only because of
  `/relaunch=1`, so silent installs from winget do not open the app.
- **Install on macOS:** unpack the `.app.tar.gz` into a staging folder next to the installed
  bundle (same volume, so the swap is a rename), spawn a detached `sh` script that waits for our
  PID to exit, swaps the bundles, removes the old one and runs `open` on the new one, then quit.
  Files downloaded by Postino itself get no quarantine attribute, so this works with an unsigned
  app. If the bundle folder is not writable, fall back to opening the release page.
- **Unsaved tabs** reuse the close path, the same prompt as quitting.
- **Privacy:** the only request is the GitHub download, with no identifiers.
- **Logging:** Info for "update available X.Y.Z" and "installing", Warn for failures.
- **Hardening for later:** sign `latest.json` with [minisign](https://jedisct1.github.io/minisign/)
  in CI and verify it with a public key embedded in the binary. It protects users from a tampered
  release asset, not from a stolen signing secret. Worth adding before 1.0.

## Signing

The builds are unsigned, which causes friction on first install only:

| OS | Without signing | Fix and cost |
| --- | --- | --- |
| macOS | Gatekeeper blocks the first launch of a downloaded app. On macOS 15 and later the user goes to System Settings, Privacy & Security, "Open Anyway" (or runs `xattr -dr com.apple.quarantine /Applications/Postino.app`). Self updates are not affected. | Apple Developer Program, 99 USD per year: Developer ID signing plus notarization in CI. Also unlocks the official `homebrew/cask`. Tracked in #46. |
| Windows | SmartScreen shows "Windows protected your PC" until the installer builds reputation: "More info, Run anyway". winget accepts unsigned installers. | [SignPath Foundation](https://signpath.org) gives free code signing to open source projects (application and review needed), or Azure Trusted Signing (about 10 USD per month). |
| Linux | Nothing; the APT and DNF repos are GPG signed for free. | None. |

macOS signing is a small follow-up: `codesign` with a Developer ID certificate plus `notarytool`
in the macOS job, with the certificate as a base64 `.p12`, its password and an App Store Connect
API key as secrets. For Windows, apply to SignPath once the project has some history.

## Secrets

Created as each channel is set up, all free: `AUR_SSH_KEY`, `GPG_PRIVATE_KEY` and
`GPG_PASSPHRASE`, `HOMEBREW_TAP_TOKEN`, `WINGET_TOKEN`.

## Later

macOS signing and notarization (#46), ARM builds, Flathub, minisign for `latest.json`, Windows
signing, a single instance, and the Homebrew and winget channels (#44).
