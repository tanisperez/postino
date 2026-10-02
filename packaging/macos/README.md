# macOS packaging

- `Info.plist`: template for `Postino.app/Contents/Info.plist`. `bundle.sh` replaces
  `@SHORT_VERSION@` and `@BUILD_VERSION@` (both the numeric `X.Y.Z`, pre-release suffix dropped).
  Declares the `codes.tanis.postino.request` UTI (extension `postino`, conforms to
  `public.plain-text`) and makes Postino its Owner/Editor. Minimum macOS is 11.0 (the arm64
  floor; gpui targets 10.15).
- `bundle.sh`: builds the app, the `.dmg` and the `.app.tar.gz`.

```sh
packaging/macos/bundle.sh 0.1.0 target/aarch64-apple-darwin/release/postino dist/
```

Output in `dist/`: `Postino-<version>-macos-arm64.dmg` (UDZO, with an `/Applications` link) and
`Postino-<version>-macos-arm64.app.tar.gz` (used by the in-app updater). The bundle is signed
ad hoc (`codesign --sign -`), not with a Developer ID.
The `.icns` is built from `packaging/icons/png/` (16 to 1024 px).

Needs macOS tools: `iconutil`, `codesign`, `hdiutil`.

Tested on Linux only, with stub `iconutil`, `codesign` and `hdiutil`: bundle layout, plist
substitution (parsed with Python `plistlib`) and the tarball. Not run on real macOS yet.
