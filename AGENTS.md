# Repository Guidelines

## Project Structure & Modules
- `backend/`: Rust Axum API, ODBC/DM8 logic (`src/db`, `src/api`, `src/export`, `src/models`), entrypoints `src/main.rs`, `src/lib.rs`.
- `frontend/`: React + Vite UI (`src/components`, `src/pages/ExportWizard.tsx`, `src/services/api.ts`, `src/store/useExportStore.ts`), assets in `src/assets`.
- `drivers/dm8/`: Bundled DM8 ODBC driver binaries (Linux `.so`; `windows/` for DLLs).
- `docs/`: Plans, packaging notes, QA checklists.

## Build, Test, Run
- Backend build/test: `cd backend && cargo build` / `cargo test` / `cargo check`.
- Backend dev (HTTP): `export LD_LIBRARY_PATH=$(pwd)/../drivers/dm8:$LD_LIBRARY_PATH && export DM8_DRIVER_PATH=$(pwd)/../drivers/dm8/libdodbc.so && cargo run` (binds :3000).
- Frontend dev: `cd frontend && npm install && npm run dev -- --host 127.0.0.1 --port 5173` (or 5174 if taken). Prod build: `npm run build`.
- Tauri shell (work-in-progress): `cd src-tauri && cargo tauri dev` after installing WebKit deps.

## Coding Style & Naming
- Rust: `cargo fmt`, idiomatic module naming (`snake_case` files/modules).
- TypeScript/React: ESLint + Prettier (`npm run lint`, `npm run format`), `@/` alias to `src/`. Components `PascalCase`, hooks `useX`, stores in `store/`.
- Paths: API base `/api`, driver vars `LD_LIBRARY_PATH`, `DM8_DRIVER_PATH`.

## Testing Guidelines
- Backend: `cargo test` (unit/integration). Add focused tests per module; prefer deterministic ODBC fakes or clear env setup.
- Frontend: use existing test setup if added; colocate tests near components or in `__tests__`, name `*.test.ts(x)`.
- Run relevant tests before pushing; include manual steps for DM8 connectivity when applicable.

## Commit & PR Guidelines
- Commits: concise, imperative (e.g., `feat: add tauri driver discovery`, `fix: adjust api base url`). Avoid bundling unrelated changes.
- PRs: describe intent, list key changes, include testing done (`cargo test`, `npm run build/dev`), note driver/env assumptions and ports (3000/5173).
- Attach screenshots/GIFs for UI changes when possible; link issues/tickets if available.

## Security & Configuration Tips
- Keep secrets out of repo; use `.env` (backend) for DB credentials. Do not commit `.env`.
- Prefer bundled drivers; only set `DM8_DRIVER_PATH`/`LD_LIBRARY_PATH` when overriding. Document custom paths in PRs.
- Ports: backend 3000, frontend dev 5173/5174. Update proxy settings if changed.
