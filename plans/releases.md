# Postino releases, distribution and updates plan

Status: written on 2026-10-02, open questions settled the same day (section 8). Nothing
implemented yet.

Goal: publishing a version is "push a tag", and every user gets it through the channel that is
natural on their OS (package manager on Linux, self update or Homebrew on macOS, self update or
winget on Windows). No backend: only free GitHub features (Actions on a public repo, Releases,
Pages) plus the free community registries (AUR, Homebrew tap, winget-pkgs).

## 1. Decisions

| Topic | Decision |
|---|---|
| Version source | `[workspace.package] version` in `Cargo.toml`. SemVer, tags `vX.Y.Z`. First public release: `0.1.0`. CI fails if the tag and the version differ; a pre-release tag (`v0.1.0-rc.1`) only needs the part before the suffix to match. |
| Website | GitHub Pages with the custom domain `postino.tanis.codes` (a CNAME record, free HTTPS). It hosts the landing page, the install page and the APT/DNF repos. |
| Pre-releases | Tags with a suffix (`v0.3.0-beta.1`) become GitHub pre-releases: built and attached, but not distributed and invisible to the updater (`/releases/latest` skips pre-releases). |
| Trigger | `release.yml` on `push: tags: ["v*"]`. The existing `build.yml` stays as the per-push CI. |
| Release notes | Written by hand (milestone flow, `release` skill), created as a **draft** right after pushing the tag. The workflow uploads the assets to that draft and publishes it only when every asset is there, so nobody (updater, package repos) sees a half-built release. |
| Targets, first round | Linux x86_64, Windows x86_64, macOS arm64. ARM Linux and ARM Windows later (free `ubuntu-22.04-arm` and `windows-11-arm` runners for public repos). macOS Intel only if someone asks (universal binary via `lipo`). |
| App id | `codes.tanis.postino`, the same `codes.tanis.*` scheme as the author's mobile apps (the domain `tanis.codes` is owned, which Flathub requires). Used for the `.desktop` file, Wayland `app_id`, macOS bundle id and the Windows AppUserModelID. |
| Update manifest | A `latest.json` asset on every release. The app reads `https://github.com/tanisperez/postino/releases/latest/download/latest.json`: GitHub redirects it to the newest stable release, no API, no token, no rate limit. |
| Who updates | Build time switch `POSTINO_UPDATER=github`, set only on the macOS `.app` and the Windows installer. Everything else (deb, rpm, AUR, local builds) ships without an updater: the package manager owns updates. |
| Signing | None in the first round. The author already has an Apple Developer account, so macOS signing costs nothing extra and comes later as a small step (section 7). |

## 2. Release assets

One release, these files (about 40 MB each, the binary is 42 MB stripped):

| Asset | Built with | Used by |
|---|---|---|
| `postino-X.Y.Z-linux-x86_64.tar.gz` | `tar` | AUR `postino-bin`, manual installs |
| `postino_X.Y.Z_amd64.deb` | [nfpm](https://nfpm.goreleaser.com) | APT repo, direct download |
| `postino-X.Y.Z-1.x86_64.rpm` | nfpm (same YAML as the deb) | DNF repo, direct download |
| `Postino-X.Y.Z-macos-arm64.dmg` | `hdiutil` | first install, Homebrew cask |
| `Postino-X.Y.Z-macos-arm64.app.tar.gz` | `tar` | macOS self update |
| `Postino-X.Y.Z-windows-x86_64-setup.exe` | Inno Setup | first install, winget, Windows self update |
| `latest.json` | small script in the workflow | in-app updater |
| `SHA256SUMS` | `sha256sum` | everyone |

nfpm is one static binary and one YAML file for deb and rpm (and Arch, if ever needed), so there
is no per-distro packaging toolchain to learn.

Linux binaries are built on `ubuntu-22.04` (glibc 2.35), so they run on Debian 12, Ubuntu 22.04+,
Fedora and Arch. Package dependencies to declare: `libxkbcommon`, `libxkbcommon-x11`,
`libwayland-client`, `libxcb`, `libvulkan` (loader), `fontconfig`, `freetype`.

`latest.json`:

```json
{
  "version": "0.3.0",
  "notes_url": "https://github.com/tanisperez/postino/releases/tag/v0.3.0",
  "assets": {
    "macos-aarch64": { "url": "https://github.com/.../Postino-0.3.0-macos-arm64.app.tar.gz", "sha256": "..." },
    "windows-x86_64": { "url": "https://github.com/.../Postino-0.3.0-windows-x86_64-setup.exe", "sha256": "..." }
  }
}
```

## 3. Release workflow (`.github/workflows/release.yml`)

```
tag v0.3.0 pushed
  └─ check: tag == Cargo version, prerelease = tag contains "-"
      ├─ build-linux   (ubuntu-22.04)  cargo build --release, tar.gz, nfpm deb + rpm
      ├─ build-macos   (macos-latest)  cargo build --release, .app bundle, ad-hoc codesign, dmg, app.tar.gz
      └─ build-windows (windows-latest) cargo build --release, Inno Setup installer
          └─ publish: SHA256SUMS + latest.json, upload all to the draft (create it if missing),
                      mark it published (and prerelease if needed)
              └─ distribute (stable only, each job independent, a failure does not block others):
                   ├─ apt + dnf repos on GitHub Pages
                   ├─ AUR postino-bin
                   ├─ Homebrew tap cask
                   └─ winget PR
```

Packaging files live in a new `packaging/` folder: `nfpm.yaml`, `linux/` (desktop file, MIME
XML, icons), `macos/Info.plist`, `windows/postino.iss`, `aur/PKGBUILD.in`,
`homebrew/postino.rb.in`. A `make dist` target builds the package for the current OS locally, so
packaging can be tested without tagging.

Day to day flow (fits the global milestone flow and the `release` skill):

1. Close the milestone issues, bump `version` in `Cargo.toml`, commit.
2. `git tag v0.3.0 && git push origin main v0.3.0` (asking first, as always).
3. `gh release create v0.3.0 --draft --verify-tag --notes-file notes.md`.
4. Wait for the workflow (about 20 to 30 minutes, the Windows and macOS builds dominate). It
   publishes the release and distributes it.

The only change to the `release` skill is creating the release as `--draft` for this project.

## 4. Desktop integration (install registers the app and `.postino`)

Needed on every OS: a source icon. Today there is only `assets/postino.ico`. Add an SVG master
(`packaging/icons/postino.svg`) and generate the PNG sizes and the `.icns` from it, committed.

App side, small changes in `postino-app`:

- A `.postino` path as the first argument opens its workspace and that request in a tab. Today
  the first argument is a workspace folder (`main.rs`). Rule: a folder opens as today; a file
  opens the remembered workspace if it contains the file, otherwise the file's parent folder,
  then opens the file in a tab.
- macOS delivers "open with" through Apple Events, not argv: hook `cx.on_open_urls`
  (gpui-pre 0.3.7, `app.rs:276`) and route the `file://` URLs to the same code.
- Set the Wayland `app_id` (`WindowOptions::app_id`) to the app id, so the window matches the
  `.desktop` file (right icon and name in the dock and alt-tab).
- No single instance in the first round: opening a file while Postino runs starts a second
  window/process. Forwarding to the running instance can come later.

Per OS:

| OS | What the package installs |
|---|---|
| Linux (deb, rpm, AUR) | `/usr/bin/postino`, `/usr/share/applications/codes.tanis.postino.desktop` (with `MimeType=application/x-postino;`), `/usr/share/mime/packages/codes.tanis.postino.xml` (glob `*.postino`), icons in `/usr/share/icons/hicolor/*/apps/`. The distro triggers (dpkg triggers, rpm file triggers, pacman hooks) refresh the MIME and desktop databases, nothing to script. |
| macOS | `Postino.app` with `Info.plist`: `CFBundleIdentifier`, `CFBundleDocumentTypes` + `UTExportedTypeDeclarations` for the `postino` extension, `.icns` icon. LaunchServices registers it when the app lands in `/Applications`. |
| Windows | Inno Setup, **per user** install (`%LOCALAPPDATA%\Programs\Postino`, no admin, like Zed and VS Code's user installer, so self update never needs UAC). Start menu shortcut, optional desktop shortcut (checkbox), `.postino` association under `HKCU\Software\Classes`, uninstaller in "Apps", `CloseApplications=yes` so it can update a running Postino. |

## 5. Distribution channels

### Linux

| Channel | How | Cost of upkeep |
|---|---|---|
| **Arch (AUR)** `postino-bin` | `PKGBUILD` that downloads the release tarball and checks its sha256. The `distribute` job renders it and pushes to `aur.archlinux.org` over SSH ([`KSXGitHub/github-actions-deploy-aur`](https://github.com/KSXGitHub/github-actions-deploy-aur)). Secret: an AUR SSH key. `yay -Syu` updates. | none after setup |
| **Debian / Ubuntu** | Signed APT repo on GitHub Pages (`apt-ftparchive` + GPG). Users add the key and one `.sources` file once, then `apt upgrade` updates. | none after setup |
| **Fedora / openSUSE** | Signed DNF repo on the same Pages site (`createrepo_c` + GPG, `rpm --addsign`). One `.repo` file, then `dnf upgrade`. | none after setup |
| Everyone else | The tarball, plus the deb/rpm as direct downloads. | none |

APT and DNF repos share one `gh-pages` branch and one GPG key (private key and passphrase as
secrets, public key published on the site). GitHub Pages limits: 1 GB per site, 100 MB per file,
so the repos keep only the last 3 versions; older ones stay downloadable from Releases. The Pages
site, served at `postino.tanis.codes`, is also the project's landing page and install page
with the copy-paste commands. Repo URLs use that domain, so moving the hosting later never
breaks the users' apt/dnf configuration.

Considered and left for later:

- **Flathub**: one package for all distros with updates, but Flathub builds from source offline
  (every crate vendored in the manifest) and reviews the submission. Good second step once the
  app id and name are final, because renaming a Flathub app is painful.
- **Ubuntu PPA / Fedora COPR / official distro repos**: build from source on their servers
  without network, with their own review. Too much work for now.
- **AppImage**: easy to build, but no updates without extra machinery. Not needed with the tarball.

### macOS

- **Homebrew**: own tap `tanisperez/homebrew-postino` with a cask pointing at the release dmg.
  The `distribute` job bumps version and sha256 and commits (secret: a fine-grained token with
  write access to the tap repo only). Users: `brew install --cask tanisperez/postino/postino`.
  The cask declares `auto_updates true`, so `brew upgrade` leaves it to the in-app updater, as
  the Zed cask does. The official `homebrew/cask` requires signed and notarized apps, so it is
  out of reach without the Apple Developer Program (section 7).
- **Direct**: the dmg from Releases or the Pages site.
- **Updates**: in-app (section 6).

### Windows

- **winget**: a PR to `microsoft/winget-pkgs` per release, automated with
  [`vedantmgoyal9/winget-releaser`](https://github.com/vedantmgoyal9/winget-releaser) (secret: a
  classic token with `public_repo`, plus a fork of winget-pkgs). The first submission is manual
  and reviewed by Microsoft; later ones are usually merged automatically. Users:
  `winget install Postino` and `winget upgrade`. winget reads the installed version from the
  uninstall entry the Inno installer writes, so both update paths agree.
- **Direct**: the setup exe from Releases.
- **Updates**: in-app (section 6).

## 6. In-app updater

New standalone crate `postino-update` (`ureq`, `serde_json`, `semver` and `sha2`, all already
in the lock file). No UI, testable with the local `tiny_http`
server like `postino-http`.

```
postino-app ──> postino-update
```

Behaviour:

- Compiled in only when `POSTINO_UPDATER=github` at build time (`option_env!`); otherwise the
  whole feature, its setting and its menu entries are absent.
- Settings, "Advanced": "Check for updates automatically" (on by default). Command palette:
  "Check for updates".
- Check: once, 10 s after startup, on the background executor. No timer, no polling: the idle
  rule stays intact. A window open for days only learns of a new version on the next start or
  the manual check, which is fine.
- `GET .../releases/latest/download/latest.json`, compare with `env!("CARGO_PKG_VERSION")` using
  `semver`. Network or parse errors are logged at Warn and otherwise ignored (no error popups for
  a background check; the manual check does show them).
- Newer version found: download the asset for this platform in the background, verify sha256,
  then show a discreet indicator in the status bar ("Update ready, restart"), plus a link to the
  release notes. Nothing installs without a click.
- On click:
  - **Windows**: run the downloaded installer with
    `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /relaunch=1` and quit. The installer replaces the
    files (closing Postino through the Restart Manager if needed) and relaunches Postino only
    because of `/relaunch=1`, so silent installs from winget do not open the app.
  - **macOS**: unpack the `.app.tar.gz` into a staging folder next to the installed bundle
    (same volume, so the swap is a rename), spawn a detached `sh` script that waits for our PID
    to exit, swaps the bundles, removes the old one and runs `open` on the new one, then quit.
    Files downloaded by Postino itself get no quarantine attribute, so this works with an
    unsigned app. If the bundle folder is not writable (app in `/Applications` owned by another
    admin user), fall back to "Download from GitHub" opening the release page.
- Unsaved tabs: reuse the existing close path, the same prompt as quitting.
- Privacy: the only request is the GitHub download above, no identifiers. Stated in the README.
- Logging: Info for "update available X.Y.Z" and "installing", Warn for failures.

Optional hardening, cheap: sign `latest.json` with [minisign](https://jedisct1.github.io/minisign/)
in CI (private key as a secret) and verify it with the public key embedded in the binary
(`minisign-verify`, no dependencies). It protects users from a tampered release asset, not from
a stolen signing secret. Worth adding before 1.0.

## 7. Signing (the only part that is not free)

Unsigned builds work, with friction on first install only:

| OS | Without signing | Fix and cost |
|---|---|---|
| macOS | Gatekeeper blocks the first launch of a downloaded app. On macOS 15+ the user must go to System Settings, Privacy & Security, "Open Anyway" (or run `xattr -dr com.apple.quarantine /Applications/Postino.app`). Documented on the install page. Self updates are not affected. | Apple Developer Program, 99 USD/year: Developer ID signing plus notarization in CI. Also unlocks the official `homebrew/cask`. |
| Windows | SmartScreen shows "Windows protected your PC" until the installer builds reputation; "More info, Run anyway". winget accepts unsigned installers. | [SignPath Foundation](https://signpath.org) gives free code signing to open source projects (application and review needed), or Azure Trusted Signing (about 10 USD/month). |
| Linux | Nothing; the APT/DNF repos are GPG signed for free. | none |

Decision: ship unsigned. The Apple Developer account already exists, so macOS signing and
notarization (`codesign` with a Developer ID certificate plus `notarytool` in the macOS job,
secrets: the certificate as a base64 `.p12`, its password and an App Store Connect API key) is a
cheap follow-up that removes the Gatekeeper step and opens the official `homebrew/cask`. Windows:
apply to SignPath once the project has some history.

## 8. Settled questions

1. App id `codes.tanis.postino` (section 1). The product name Postino is final (2026-10-03, see
   `AGENTS.md`); package names (AUR, winget, Homebrew, repos) follow it.
2. A `.postino` file opened on its own opens its parent folder as the workspace, unless a
   remembered workspace already contains it (section 4).
3. The first release is `0.1.0`.
4. Unsigned at first; macOS signing is a later step (section 7).
5. The Pages site is the project's landing page, at `postino.tanis.codes`.

## 9. Phases

Each phase is a set of milestone issues, merged independently, and each leaves the project in a
releasable state.

| Phase | Content | Done when |
|---|---|---|
| 1. Release pipeline (#41) | `release.yml`: version check, three builds, raw binaries + `SHA256SUMS`, draft then publish. | A `v0.1.0-rc.1` tag produces a pre-release with the three binaries. |
| 2. Native packages and integration (#42) | SVG icon and derived sizes, `packaging/`, nfpm deb/rpm, tarball, `.app` + dmg, Inno installer, `.postino` association, file argument + `on_open_urls`, Wayland `app_id`, `make dist`. | Installing each package shows Postino in the launcher/Start menu/Launchpad and double clicking a `.postino` file opens it, checked on Arch, a Debian or Ubuntu VM, macOS and Windows. |
| 3. Linux repos (#43) | Pages site, signed APT and DNF repos, AUR `postino-bin`, install page. | `apt upgrade`, `dnf upgrade` and `yay -Syu` pick up a test release. |
| 4. macOS and Windows channels (#44) | Homebrew tap cask, winget first manual submission, then automation. | `brew install --cask` and `winget install` work. |
| 5. In-app updater (#45) | `postino-update` crate with tests, settings toggle, palette command, status bar indicator, Windows and macOS install paths, `latest.json` in the workflow. | Going from `0.x.0` to `0.x.1` through the updater works on macOS and Windows, idle rule re-checked. |
| Later | macOS signing and notarization (#46), ARM builds, Flathub, minisign, Windows signing, single instance. | |

Secrets to create along the way: `AUR_SSH_KEY`, `GPG_PRIVATE_KEY` + `GPG_PASSPHRASE`,
`HOMEBREW_TAP_TOKEN`, `WINGET_TOKEN`. All free.
