# Windows packaging

## Manifest

`postino.exe` is a console subsystem binary (#282), so shells wait for its commands. Right after
the release build, `merge-manifest.ps1` merges `console.manifest` (`consoleAllocationPolicy`
`detached`, so a double click opens no console window on Windows 11 24H2 or later) into the
manifest gpui embeds, with `mt.exe` from the Windows SDK. It runs before the installer and the
zip, which package the merged exe:

```powershell
powershell -File packaging\windows\merge-manifest.ps1 target\x86_64-pc-windows-msvc\release\postino.exe
```

## Installer

`postino.iss` is an Inno Setup 6 script. Defines, passed to `ISCC`:

- `AppVersion`: full version, may be `0.1.0-rc.1` (the numeric file version is derived).
- `SourceExe`: path to the release `postino.exe`.
- `OutputDir`: output folder (default `Output`).

```bat
ISCC.exe /DAppVersion=0.1.0 /DSourceExe=..\..\target\release\postino.exe /DOutputDir=..\..\dist postino.iss
```

Produces `Postino-<version>-windows-x86_64-setup.exe`. Per-user install in
`%LOCALAPPDATA%\Programs\Postino`, no admin. Adds the install folder to the user `Path` (HKCU, removed on uninstall, #284). Registers `.postino` under HKCU, Start menu shortcut
(and optional desktop one) with AppUserModelID `codes.tanis.postino`. Interactive installs offer to
launch Postino at the end; silent installs (winget) do not, unless `/relaunch=1` is passed, which
the in-app updater does. The `AppId` GUID must never change. Languages:
English, Spanish, Italian (Inno bundles no Galician).

Tested with Inno Setup 6.7.3 under wine: compile, silent install, registry keys, shortcuts,
silent uninstall cleanup. Tested on Windows 11 25H2 (a VM, 2026-11-18, #284): silent install twice,
the Path entry added once, `postino` found by a new terminal, and removed by the uninstaller. Run the
uninstaller from the desktop, not over SSH: OpenSSH kills it with the session, halfway.

## Portable zip

There is no script for it: the `Package the portable zip for Windows` step of
`.github/workflows/release.yml` builds `Postino-<version>-windows-x86_64-portable.zip` with
`Compress-Archive`, right after the installer, into the same `dist` folder. It holds `postino.exe`,
`LICENSE` and `NOTICE` at the top level and is meant for machines where running an installer is
not allowed. Settings and logs stay where the installed app keeps them. To make one by hand:

```powershell
Compress-Archive -DestinationPath dist\Postino-0.1.0-windows-x86_64-portable.zip `
  -Path target\release\postino.exe, LICENSE, NOTICE
```

The zip is not in `latest.json` (its name does not match the `-setup.exe` glob). The in-app updater
tells the two copies apart by `unins000.exe`, which the installer always leaves next to
`postino.exe`: a copy without it is portable, never runs the installer (that would install a second
copy) and sends the user to the release page instead. Keep that file name and location if the
installer script changes.
