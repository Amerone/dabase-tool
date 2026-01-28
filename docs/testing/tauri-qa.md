# Tauri Desktop QA Checklist

## Environments
- Linux AppImage on a clean VM (no DM8 drivers installed)
- Linux with system DM8 driver installed (optional)
- Windows exe on a clean VM (WebView2 present or installed by NSIS)

## Pre-flight
- Frontend built (`npm run build`)
- Driver assets placed:
  - Linux: `drivers/dm8/libdodbc.so`, `libdmdpi.so`, `libdmfldr.so`
  - Windows: `drivers/dm8/windows/dmodbc.dll` (+ deps)

## Checks
1. Launch app
   - Expect no driver-missing dialog when bundled drivers exist.
   - If drivers missing, dialog appears and app exits.
2. Driver source indicator
   - In connection form header, shows source (Bundled/Env/System) and path.
3. Connection test (bundled drivers)
   - Fill connection info, click “Test Connection”; expect success against DM8 instance.
4. Config persistence
   - Load saved config; save updated config; reload to confirm updated_at reflects change.
5. Flow
   - Test connection → Next → select tables → export DDL/Data; downloads succeed.
6. System driver fallback (optional)
   - Remove bundled drivers; ensure system driver detected (source shows System) and flow still works.
7. Env override (optional)
   - Set `DM8_DRIVER_PATH` to custom driver; app shows Env source and connects.

## Regression considerations
- App exits gracefully if backend fails to start; clear dialog shown.
- Backend port is randomized; frontend still connects via Tauri command.
- Config DB stored at `~/.amarone/config.db` (Linux) or `%USERPROFILE%\.amarone\config.db` (Windows).
