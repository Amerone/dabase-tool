# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Full-stack app for exporting DM8 (达梦) database schemas and data. Generates SQL files containing DDL (CREATE TABLE, CREATE SEQUENCE) and INSERT statements with DM8-specific syntax.

**Tech stack:** Rust (Axum + ODBC) backend, React + TypeScript + Vite frontend, Tauri v2 desktop packaging.

## Running the App

### HTTP Mode (development)

Start backend and frontend separately:

```bash
# Backend (port 3000)
cd backend
./run_with_dm8_driver.sh          # Linux: auto-sets driver paths
# Windows: use run_backend.ps1 or run_backend.bat

# Frontend (port 5173, proxies /api to backend)
cd frontend
npm run dev
```

### Desktop Mode (Tauri)

```powershell
# Windows NSIS installer
powershell -ExecutionPolicy Bypass -File build_windows.ps1

# Linux AppImage/deb
./build_linux.sh
```

Tauri starts the backend on a random port, discovers the DM8 ODBC driver automatically, and exposes `backend_base_url()` and `driver_info()` commands to the frontend.

### Backend Configuration

Backend needs `backend/.env` (copy from `.env.example`):

```env
DATABASE_HOST=localhost
DATABASE_PORT=5236
DATABASE_USERNAME=SYSDBA
DATABASE_PASSWORD=SYSDBA
DATABASE_SCHEMA=SYSDBA
SERVER_PORT=3000
```

`.env` is only the initial fallback. Runtime config is persisted in `~/.amarone/config.db` (SQLite) with AES-GCM encrypted passwords. Users save/load connections from the frontend.

## Common Commands

### Backend

```bash
cd backend
cargo run                         # start dev server
cargo test                        # all tests
cargo test config_store           # specific module tests
cargo test test_name              # single test by name
cargo check                       # type-check without building
cargo clippy                      # lint
cargo fmt                         # format
cargo build --release             # production build
```

### Frontend

```bash
cd frontend
npm run dev                       # dev server (5173)
npm run build                     # production build
npm run lint                      # ESLint check
npm run lint:fix                  # auto-fix lint issues
npm run format                    # Prettier format
```

## Architecture

### Data Flow

1. Frontend (`services/api.ts`) sends requests to `/api/*`
2. Vite dev server proxies to backend `localhost:3000` (or Tauri provides the URL)
3. Axum routes dispatch to API handlers in `api/`
4. Handlers call `db/dm8_adapter.rs` which executes ODBC queries
5. `export/` modules generate SQL files written to `backend/exports/`
6. Results return to frontend, state managed by Zustand store

### Backend Modules (`backend/src/`)

| Module | Purpose |
|--------|---------|
| `api/mod.rs` | Axum router, CORS config, `AppState` (holds `ConfigStore`) |
| `api/connection.rs` | `POST /api/connection/test` |
| `api/schema.rs` | Schema/table metadata endpoints |
| `api/export.rs` | DDL and data export endpoints |
| `api/config.rs` | Connection config persistence (GET/POST `/api/config/connection`) |
| `db/connection.rs` | ODBC connection management |
| `db/dm8_adapter.rs` | **Core**: all DM8 database operations |
| `db/schema.rs` | Metadata queries: tables, columns, indexes, constraints, triggers, sequences |
| `db/odbc_register.rs` | Windows-only: registers ODBC driver in Windows registry at startup |
| `export/ddl.rs` | DDL generation: CREATE TABLE, CREATE SEQUENCE, triggers, indexes, comments |
| `export/data.rs` | INSERT statement generation with batching and row counting |
| `config_store/mod.rs` | SQLite config store at `~/.amarone/config.db` |
| `models/mod.rs` | Data models: `ConnectionConfig`, `Table`, `Column`, `Sequence`, `ExportRequest` |

### Frontend Key Files (`frontend/src/`)

| File | Purpose |
|------|---------|
| `store/useExportStore.ts` | Zustand store — single source of truth for all app state |
| `pages/ExportWizard.tsx` | Main business page: multi-step export wizard |
| `components/ConnectionForm.tsx` | DB connection form with save/load config |
| `components/ExportConfig.tsx` | Export options: DDL/data, compatibility mode, batch size |
| `components/TechBackground.tsx` | Canvas particle animation (perf-sensitive) |
| `services/api.ts` | Axios-based API client |
| `types/index.ts` | All TypeScript type definitions |

### Tauri Desktop (`src-tauri/`)

| File | Purpose |
|------|---------|
| `src/main.rs` | Driver discovery, backend server start (random port), Tauri commands |
| `src/driver.rs` | Platform-specific driver resolution (bundled → env var → system) |
| `tauri.conf.json` | Bundle config: NSIS (Windows), AppImage/deb (Linux), resource mapping |

## ODBC Driver Strategy

**Priority order:** bundled `drivers/dm8/` → `DM8_DRIVER_PATH` env var → system ODBC config.

- **Linux:** `libdodbc.so` + supporting `.so` files in `drivers/dm8/`. Set via `LD_LIBRARY_PATH`.
- **Windows:** `dodbc.dll` + DM8 DLLs in `drivers/dm8/windows/`. `odbc_register.rs` writes to Windows registry. DLLs may have ReadOnly attributes that cause build failures — `build_windows.ps1` clears them.

## Export Features

### DDL Export

Generates `CREATE TABLE` with columns, PKs, indexes, constraints, triggers, and `COMMENT ON` statements. Supports `DROP TABLE IF EXISTS`, `IDENTITY` columns, `DEFAULT` values.

### Sequence Export

Generates `CREATE SEQUENCE` with DM8-specific options: `START WITH`, `INCREMENT BY`, `MINVALUE/MAXVALUE`, `CACHE/NOCACHE`, `CYCLE/NOCYCLE`, `ORDER/NOORDER`.

### Data Export

Generates batched `INSERT` statements with optional `TRUNCATE TABLE` or `DELETE FROM` cleanup and row count statistics.

### Export Compatibility Modes

Three modes control trigger/statement terminator syntax:

| Mode | Key | Behavior |
|------|-----|----------|
| DataGrip 逐语句 | `datagrip` | `END;` without `/` delimiter |
| DataGrip 脚本模式 | `datagrip-script` | Triggers written to separate `.triggers.sql` file |
| DBeaver/SQLArk/DIsql | `script` | `END;` followed by `/` delimiter |

### Output Files

Written to `backend/exports/` with format: `<schema>_ddl_YYYYMMDD_HHMMSS.sql` (or `_data_`). Headers include generation time, table list, and row counts.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| POST | `/api/connection/test` | Test database connection |
| GET | `/api/config/connection` | Get saved connection (SQLite → `.env` fallback) |
| POST | `/api/config/connection` | Save connection to SQLite |
| GET | `/api/schemas` | List all schemas |
| GET | `/api/tables` | List tables in schema (with row counts) |
| GET | `/api/tables/:table/details` | Table details (columns, indexes, constraints, triggers) |
| POST | `/api/export/ddl` | Export DDL. Body: `tables`, `schema`, `drop_existing`, `export_compat` |
| POST | `/api/export/data` | Export data. Body: `tables`, `schema`, `batch_size`, `include_row_counts` |

## UI Design

Dark cyberpunk theme: primary color `#00b96b`, dark blue-black gradient backgrounds, glass morphism, sharp 2px corners. Ant Design dark mode with custom tokens. Canvas particle background in `TechBackground.tsx` uses `requestAnimationFrame`.

## Key Dependencies

**Backend:** `odbc-api` (ODBC), `axum` + `tower-http` (HTTP/CORS), `rusqlite` (config), `aes-gcm` (password encryption), `chrono` (timestamps), `encoding_rs` (Chinese encoding), `winreg` (Windows ODBC registration)

**Frontend:** React 18, Ant Design, Zustand (state), React Query (data fetching), React Router v7, anime.js (animation), Axios, Vite (build + dev proxy)

## Gotchas

- Backend creates a **new ODBC connection per request** (no connection pool). Fine for single-user export tool.
- `TechBackground.tsx` is performance-sensitive — runs `requestAnimationFrame` loop.
- Windows DM8 DLLs in `drivers/dm8/windows/` have ReadOnly file attributes. Build scripts must clear them or Tauri resource copying fails with OS error 5.
- Tauri v2 config paths: `bundle.windows.nsis` (not `bundle.nsis`), `bundle.linux.deb` (not `bundle.deb`).
- Frontend path alias: `@/` → `src/`.
- Vite proxy config in `vite.config.ts` forwards `/api` to `http://localhost:3000`.
- Log level: `RUST_LOG=dm8_export_backend=debug,tower_http=debug`.
