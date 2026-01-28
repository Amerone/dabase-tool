# Packaging Notes (Tauri)

Targets:
- Linux: AppImage (`tauri.conf.json` bundle targets include `appimage`)
- Windows: NSIS installer (`nsis` -> `.exe`)

Prerequisites (Linux):
- `pkg-config`
- `libwebkit2gtk-4.0-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev` (needed for WebView)

Prerequisites (Windows):
- Build tools (MSVC)
- WebView2 runtime (if not present, NSIS installer can download)

Driver assets:
- Bundled from `drivers/dm8/` (Linux `.so` files and `drivers/dm8/windows` for Windows DLLs).

Build commands (run from repo root):
- Dev (with embedded backend): `cd src-tauri && cargo tauri dev`
- Release Linux AppImage: `cd src-tauri && cargo tauri build --target x86_64-unknown-linux-gnu`
- Release Windows exe: `cd src-tauri && cargo tauri build --target x86_64-pc-windows-msvc`

Notes:
- If Tauri build fails on Linux with `pkg-config`/`webkit2gtk` missing, install the prerequisites above.
- `tauri.conf.json` points to `../frontend/dist`; ensure `npm run build` has been run in `frontend/` before packaging.
