# Export Target Schema Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a saved, user-configurable export target schema that is used only in generated SQL and output filenames.

**Architecture:** Persist `export_schema` with the connection config (SQLite). Use `config.schema` as the source schema for reads, and `export_schema` (fallback to source) for SQL generation and file names. Frontend surfaces input in connection form and export page.

**Tech Stack:** Rust (Axum, rusqlite), React + Vite (Ant Design), Zustand store, TypeScript.

---

### Task 1: Persist export_schema in backend config

**Files:**
- Modify: `backend/src/models/mod.rs`
- Modify: `backend/src/config_store/mod.rs`
- Modify: `backend/src/api/config.rs`

**Step 1: Write the failing test**

Update config store tests to round-trip `export_schema` and expect it to persist.

```rust
#[test]
fn upsert_and_get_default_round_trip() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("config.db");
    let store = ConfigStore::new_with_path(db_path).unwrap();

    let config = sample_config();
    let saved = store.upsert_default(&config).unwrap();
    assert_eq!(saved.config.export_schema.as_deref(), Some("APP"));

    let fetched = store.get_default().unwrap().unwrap();
    assert_eq!(fetched.config.export_schema.as_deref(), Some("APP"));
}
```

**Step 2: Run test to verify it fails**

Run: `cd backend && cargo test config_store::tests::upsert_and_get_default_round_trip -v`
Expected: FAIL (missing field / compilation errors).

**Step 3: Write minimal implementation**

- Add `export_schema: Option<String>` to `ConnectionConfig`.
- Update `ConfigStore` SQL schema to include `export_schema` column.
- Add migration logic in `init_db` to `ALTER TABLE` when column missing.
- Update select/insert/update to read/write `export_schema`.
- Update `env_connection_config()` to set `export_schema: None`.
- Update test helper `sample_config()` to include `export_schema: Some("APP".into())`.

**Step 4: Run test to verify it passes**

Run: `cd backend && cargo test config_store::tests::upsert_and_get_default_round_trip -v`
Expected: PASS.

**Step 5: Commit**

```bash
git add backend/src/models/mod.rs backend/src/config_store/mod.rs backend/src/api/config.rs

git commit -m "feat: persist export_schema in config store"
```

---

### Task 2: Use export_schema in export APIs and SQL generation

**Files:**
- Modify: `backend/src/models/mod.rs`
- Modify: `backend/src/api/export.rs`
- Modify: `backend/src/export/ddl.rs`
- Modify: `backend/src/export/data.rs`

**Step 1: Write the failing test**

Add unit tests for target schema resolution and file naming in `backend/src/api/export.rs`.

```rust
#[test]
fn resolve_target_schema_falls_back_to_source() {
    let target = resolve_target_schema("SYSDBA", None);
    assert_eq!(target, "SYSDBA");
}

#[test]
fn resolve_target_schema_uses_trimmed_value() {
    let target = resolve_target_schema("SYSDBA", Some("  APP  "));
    assert_eq!(target, "APP");
}

#[test]
fn format_export_filename_includes_source_and_target() {
    let name = format_export_filename("SRC", "TGT", "ddl", "20260130_120000_000");
    assert_eq!(name, "exports/SRC_to_TGT_ddl_20260130_120000_000.sql");
}
```

**Step 2: Run test to verify it fails**

Run: `cd backend && cargo test api::export::tests::resolve_target_schema_falls_back_to_source -v`
Expected: FAIL (missing helpers).

**Step 3: Write minimal implementation**

- Extend `ExportRequest` with `export_schema: Option<String>`.
- Add helpers in `api/export.rs`:
  - `resolve_target_schema(source: &str, export_schema: Option<&str>) -> String`
  - `format_export_filename(source: &str, target: &str, kind: &str, stamp: &str) -> String`
- In `export_ddl` / `export_data`:
  - `source = req.config.schema.clone()`
  - `target = resolve_target_schema(&source, req.export_schema.as_deref())`
  - output path uses `format_export_filename(source, target, ...)`
  - call `export_schema_ddl(connection, &source, &target, ...)`
  - call `export_schema_data(connection, &source, &target, ...)`
- Update `export_schema_ddl` to accept `source_schema` and `target_schema` and use target schema for SQL output while querying with source schema.
- Update `export_schema_data`/`export_table_data` similarly: read from source schema, write SQL using target schema.

**Step 4: Run test to verify it passes**

Run: `cd backend && cargo test api::export::tests::resolve_target_schema_falls_back_to_source -v`
Expected: PASS.

**Step 5: Commit**

```bash
git add backend/src/models/mod.rs backend/src/api/export.rs backend/src/export/ddl.rs backend/src/export/data.rs

git commit -m "feat: apply export schema to ddl/data generation"
```

---

### Task 3: Frontend types and connection form

**Files:**
- Modify: `frontend/src/types/index.ts`
- Modify: `frontend/src/components/ConnectionForm.tsx`

**Step 1: Write the failing test**

No frontend test harness exists here. Skip automated tests and rely on manual verification.

**Step 2: Implement**

- Add `export_schema?: string` to `ConnectionConfig` and `ExportRequest` types.
- In `ConnectionForm`, add an input for export schema:
  - Label: "导出模式 (EXPORT SCHEMA)"
  - Not required
  - Placeholder can use current schema
- Update `handleTest` and `handleSave` to send `export_schema` (trimmed, empty -> undefined).
- Ensure `loadSaved` populates the field from saved config.

**Step 3: Manual check**

- Load saved config and confirm export schema appears.
- Save config and confirm value persists on reload.

**Step 4: Commit**

```bash
git add frontend/src/types/index.ts frontend/src/components/ConnectionForm.tsx

git commit -m "feat: add export_schema to connection config UI"
```

---

### Task 4: Export page input and request wiring

**Files:**
- Modify: `frontend/src/components/ExportConfig.tsx`

**Step 1: Write the failing test**

No frontend test harness exists here. Skip automated tests and rely on manual verification.

**Step 2: Implement**

- Add `export_schema` input to the ExportConfig form.
- Default value: `config.export_schema ?? config.schema`.
- On export, include `export_schema` in request (trimmed, empty -> undefined).

**Step 3: Manual check**

- Enter a target schema and export DDL/data.
- Confirm output SQL uses target schema and file name includes `source_to_target`.

**Step 4: Commit**

```bash
git add frontend/src/components/ExportConfig.tsx

git commit -m "feat: allow export schema override in export page"
```

---

### Task 5: Full test pass

**Step 1: Run backend tests**

Run: `cd backend && cargo test`
Expected: PASS (warnings ok).

**Step 2: (Optional) Frontend build**

Run: `cd frontend && npm run build`
Expected: PASS.

**Step 3: Final commit (if needed)**

If any fixes were made during testing:

```bash
git add -A

git commit -m "chore: fix tests after export schema changes"
```
