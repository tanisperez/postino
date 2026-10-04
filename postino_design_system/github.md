repo: tanisperez/postino
branch: main

## Last sync
date: 2026-10-04T19:07:00Z

### Updated in this project
- Current UI rebuilt: app now ships the redesign (Postino Dark tokens, Geist, Main A shell)
- Recreation adds title-bar search, workspace switcher, open-tabs strip, body-mode segmented control, sidebar footer, status bar
- Navigation Options: activity rail / sidebar switcher / contextual entries, Environments tab, Settings › About with updater

## Sync history
- 2026-09-26T07:55:06Z: Recreated current UI; new design system; Main A/B, Settings, Performance

## Screen map
| Screen | Repo files |
|---|---|
| Current UI.dc.html | crates/postino-app/src/views/root.rs, title_bar.rs, sidebar.rs, status_bar.rs, request_editor.rs, response_view.rs, components/{document_tabs,url_bar,underline_tabs,env_pill,method_badge,segmented_control}.rs, theme/palette.rs, theme/metrics.rs |
| Main A.dc.html / Main B.dc.html | crates/postino-app/src/views/root.rs, sidebar.rs, response_view.rs, components/*.rs |
| Settings.dc.html | crates/postino-app/src/views/settings.rs, state/settings.rs |
| Performance.dc.html | crates/postino-app/src/views/load_test/*.rs, state/load_test.rs |
| Components.dc.html | crates/postino-app/src/views/components/*.rs, components/gallery.rs |
| Navigation Options.dc.html | views/env_picker.rs, define_variable.rs, update.rs, state/update.rs, load_test/mod.rs, postino-core/src/interpolate.rs |
| Postino Design System.dc.html | crates/postino-app/src/theme/palette.rs, theme/metrics.rs, theme/mod.rs |
