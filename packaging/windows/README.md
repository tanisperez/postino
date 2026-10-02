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
