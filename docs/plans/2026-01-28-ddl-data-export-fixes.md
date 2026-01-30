# DDL & Data Export Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix export outputs so generated DDL/data scripts reflect auto-increment and default values, safely drop/truncate tables, include per-table clearing, and emit dated files with metadata headers.

**Architecture:** Rust Axum backend produces SQL files from DM8 metadata; React frontend just triggers exports and shows returned paths. Changes stay backend-centric (schema introspection, generators, API), with a small frontend type sync.

**Tech Stack:** Rust (axum, odbc-api, chrono), TypeScript/React (Vite, Zustand), DM8 dictionary views (ALL_*).

---

### Task 1: Enrich column metadata

**Files:**
- Modify: `backend/src/models/mod.rs`
- Modify: `backend/src/db/schema.rs`
- Modify: `frontend/src/types/index.ts`

**Step 1: Add fields** `default_value: Option<String>` and `identity: bool` to `Column` (Rust + TS) keeping serde compatibility.

**Step 2: Fetch metadata** in `fetch_columns` selecting `DATA_DEFAULT` and `IDENTITY_COLUMN` (or equivalent flag) from `ALL_TAB_COLUMNS`, map to new fields.

**Step 3: Quick compile check** `cd backend && cargo check`.

### Task 2: DDL generation updates

**Files:**
- Modify: `backend/src/export/ddl.rs`

**Step 1: Incorporate identity & defaults** into `format_column_definition` (append `IDENTITY` and `DEFAULT <expr>` before nullability).

**Step 2: Add drop-if-exists** per table before CREATE (`DROP TABLE IF EXISTS <schema.table>;` with comment).

**Step 3: Keep comments/indexes/PK logic unchanged; run `cargo check` for safety.

### Task 3: Data export safety & stats

**Files:**
- Modify: `backend/src/export/data.rs`
- Modify: `backend/src/db/schema.rs` (reuse row_count helper if needed)

**Step 1: Prepend clearing statement** for each table (`TRUNCATE TABLE <schema.table>;` or `DELETE FROM` fallback) before inserts.

**Step 2: Compute row counts** per table using existing helper to accumulate total rows exported; return per-table/total stats from export functions.

**Step 3: Ensure batch insert logic unchanged; `cargo check`.

### Task 4: File naming & metadata headers

**Files:**
- Modify: `backend/src/export/ddl.rs`
- Modify: `backend/src/export/data.rs`
- Modify: `backend/src/api/export.rs`

**Step 1: Build timestamped filenames** like `exports/<schema>_ddl_YYYYMMDD.sql` and `..._data_YYYYMMDD.sql`.

**Step 2: Write header block** at top of each file with table count, total rows (data file), generation time, and warning about DROP/TRUNCATE.

**Step 3: Return new paths in API response; `cargo check`.

### Task 5: Frontend type sync

**Files:**
- Modify: `frontend/src/types/index.ts`

**Step 1: Add `default_value?: string` and `identity?: boolean` to `Column` type; ensure build type safety.

### Task 6: Validation

**Files:**
- Command only

**Step 1: `cd backend && cargo check` to ensure backend builds.

**Step 2: (Optional fast check) `cd frontend && npm run lint -- --max-warnings=0` if available; otherwise note not run.

