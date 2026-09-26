repo: tanisperez/postino
branch: main

## Last sync
date: 2026-09-26T07:55:06Z

### Updated in this project
- Recreated current UI (empty state, request with failed send)
- New design system: light/dark tokens, type, spacing, components
- Main screen in two directions, Settings (Appearance), Performance tab

## Screen map
| Screen | Repo files |
|---|---|
| Current UI.dc.html | crates/postino-app/src/views/root.rs, sidebar.rs, request_editor.rs, response_view.rs, env_picker.rs, state/ui_tabs.rs, main.rs |
| Main A.dc.html / Main B.dc.html | crates/postino-app/src/views/root.rs, sidebar.rs, response_view.rs, env_picker.rs, state/ui_tabs.rs |
| Settings.dc.html | crates/postino-app/src/main.rs (Theme::sync_system_appearance) |
| Performance.dc.html | crates/postino-app/src/views/root.rs (new tab type) |
| Components.dc.html | gpui-kit components used across views/ |
