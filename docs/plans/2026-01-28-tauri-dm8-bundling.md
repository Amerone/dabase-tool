# DM8 Tauri Bundling Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver a zero-config desktop build (Linux AppImage, Windows exe) that bundles DM8 ODBC drivers, auto-injects search paths, and reuses the existing Axum+React app inside Tauri.

**Architecture:** Wrap the existing backend in a Tauri shell that launches the Axum server on a local port (or via commands) and serves the Vite build in a WebView. Bundle DM8 drivers as app resources; on startup, set `LD_LIBRARY_PATH`/`PATH` + `DM8_DRIVER_PATH` and probe system drivers as fallback. Surface driver source in the UI.

**Tech Stack:** Rust (Axum, Tauri), React/Vite, odbc-api, DM8 ODBC drivers; Tauri bundling for AppImage (Linux) and exe (Windows).

---

### Task 1: Create Tauri shell and config

**Files:**
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/main.rs`
- Modify: `.gitignore` (add `src-tauri/target/`, `src-tauri/.cargo/`)

**Step 1:** Scaffold Tauri config referencing dist assets (`../frontend/dist`), set bundle targets to AppImage and Windows exe, define app identifier/name/version.  
**Step 2:** Add Tauri Cargo manifest with dependencies `tauri`, `tauri-build`, `serde`, `anyhow`, `tokio`, `reqwest` (if needed), and workspace settings.  
**Step 3:** Implement minimal `main.rs` that starts Tauri with a command placeholder and loads the front-end dist.  
**Step 4:** Run `cd src-tauri && cargo check` to ensure the shell builds (expected: PASS).  
**Step 5:** Run `cd frontend && npm run build` to confirm static assets exist for Tauri (expected: PASS).

### Task 2: Bundle DM8 drivers as resources

**Files:**
- Move/Copy: `drivers/dm8/libdodbc.so`, `drivers/dm8/libdmdpi.so`, `drivers/dm8/libdmfldr.so`
- Create: `drivers/dm8/windows/dmodbc.dll` (+ required DM deps) [placeholder until copied]
- Modify: `src-tauri/tauri.conf.json` (`bundle.resources` include `../drivers/dm8`)

**Step 1:** Ensure `drivers/dm8/` contains Linux libs; add Windows DLL placeholders or real files.  
**Step 2:** Update `tauri.conf.json` to include the driver directory in `bundle.resources`.  
**Step 3:** Run `cd src-tauri && cargo tauri build --debug` (or `tauri dev` dry-run) to ensure resources are packaged (expected: PASS).  
**Step 4:** Document driver versions and origin in a short README note (optional).

### Task 3: Implement driver discovery & env injection

**Files:**
- Create: `src-tauri/src/driver.rs`
- Modify: `src-tauri/src/main.rs`
- (Optional tests) Create: `src-tauri/tests/driver_discovery.rs`

**Step 1:** Implement discovery logic returning a resolved driver path + search dir: priority (a) bundled resource dir, (b) `DM8_DRIVER_PATH`, (c) system ODBC (parse `/etc/odbcinst.ini`, `~/.odbcinst.ini`, Windows registry), (d) error.  
**Step 2:** Add dependency `ini` (for Linux ini parsing) and `winreg` (Windows) in `Cargo.toml`.  
**Step 3:** Add unit test(s) for ini parsing and priority ordering (mock paths) in `driver_discovery.rs`; run `cargo test driver_discovery` (expected: PASS).  
**Step 4:** In `main.rs`, on startup, call discovery, then set `LD_LIBRARY_PATH` (Linux) or `PATH` (Windows) and `DM8_DRIVER_PATH` accordingly; log selected source.  
**Step 5:** If discovery fails, surface a user-facing error via a Tauri dialog and abort startup (expected: dialog shown in failure scenario).

### Task 4: Launch Axum backend inside Tauri

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `backend/src/main.rs` (make server start callable as a library function)
- Create: `backend/src/lib.rs` (re-export server builder)

**Step 1:** Refactor backend to expose `start_server(port: Option<u16>) -> Result<SocketAddr>` in `lib.rs`, moving reusable setup from `main.rs`; keep binary main calling into it.  
**Step 2:** In Tauri `main.rs`, spawn the server on 127.0.0.1:0, capture the bound port, and share it with the front-end (e.g., via Tauri state/command).  
**Step 3:** Add graceful shutdown hook when Tauri exits.  
**Step 4:** Run `cd backend && cargo test` and `cargo check` (expected: PASS).  
**Step 5:** Run `cd src-tauri && cargo tauri dev` to verify Tauri can start the backend and load the UI (expected: app window loads, API reachable).

### Task 5: Front-end adjustments for bundled app

**Files:**
- Modify: `frontend/src/services/api.ts` (base URL acquisition via Tauri command/env)
- Modify: `frontend/src/components/ConnectionForm.tsx` (show driver source info)
- Modify: `frontend/src/store/useExportStore.ts` (store driver source)

**Step 1:** Add a Tauri invoke wrapper in `api.ts` to fetch the backend base URL from Tauri when running inside desktop; default to existing env for web.  
**Step 2:** Add a “Driver source” badge/text in the connection form showing bundled/system/custom with path (data returned from a new API or Tauri command).  
**Step 3:** Add a lightweight test (unit or component) ensuring driver source renders when provided; run `cd frontend && npm test -- ConnectionForm` (expected: PASS).  
**Step 4:** Build front-end `npm run build` to validate integration (expected: PASS).

### Task 6: Packaging targets and CI steps

**Files:**
- Modify: `src-tauri/tauri.conf.json` (bundle targets: `appimage`, `msi` or `nsis` exe)
- Add: `docs/packaging-notes.md` (commands and signing placeholders)
- (Optional) Add CI workflow file: `.github/workflows/tauri.yml`

**Step 1:** Set `bundle.targets` to AppImage for Linux and exe (NSIS) for Windows; configure icons/app id.  
**Step 2:** Draft packaging commands in `docs/packaging-notes.md` (e.g., `cargo tauri build --target x86_64-unknown-linux-gnu`, `--target x86_64-pc-windows-msvc`).  
**Step 3:** If CI exists, add a workflow to build artifacts (matrix linux/windows) with caching; run `cargo tauri build` locally as smoke test (expected: artifacts produced).  
**Step 4:** Verify produced AppImage runs on a clean-ish Linux container/VM (manual) and exe on Windows VM (manual).

### Task 7: QA checklist & user experience polish

**Files:**
- Modify/Create: `docs/testing/tauri-qa.md`

**Step 1:** Write QA checklist covering: first-run without system drivers (must succeed via bundled), with system drivers (should detect), connection test, export flow, config persistence, error dialogs.  
**Step 2:** Include manual steps for both platforms (AppImage, exe) and expected outcomes.  
**Step 3:** Run through the checklist on Linux host (if available) and note results in the doc.  
**Step 4:** Log any blockers for Windows validation (VM notes, pending driver files).

---

Plan complete and saved to `docs/plans/2026-01-28-tauri-dm8-bundling.md`. Two execution options:

1. Subagent-Driven (this session) — I dispatch tasks sequentially with reviews.  
2. Parallel Session — open a new session with executing-plans to run the plan.

Which approach?***
