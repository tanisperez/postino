# Windows packaging

`postino.iss` is an Inno Setup 6 script. Defines, passed to `ISCC`:

- `AppVersion`: full version, may be `0.1.0-rc.1` (the numeric file version is derived).
- `SourceExe`: path to the release `postino.exe`.
- `OutputDir`: output folder (default `Output`).

```bat
ISCC.exe /DAppVersion=0.1.0 /DSourceExe=..\..\target\release\postino.exe /DOutputDir=..\..\dist postino.iss
```

Produces `Postino-<version>-windows-x86_64-setup.exe`. Per-user install in
`%LOCALAPPDATA%\Programs\Postino`, no admin. Registers `.postino` under HKCU, Start menu shortcut
(and optional desktop one) with AppUserModelID `codes.tanis.postino`. Interactive installs offer to
launch Postino at the end; silent installs (winget) do not, unless `/relaunch=1` is passed, which
the in-app updater does. The `AppId` GUID must never change. Languages:
English, Spanish, Italian (Inno bundles no Galician).

Tested with Inno Setup 6.7.3 under wine: compile, silent install, registry keys, shortcuts,
silent uninstall cleanup. Not tested on real Windows.

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
