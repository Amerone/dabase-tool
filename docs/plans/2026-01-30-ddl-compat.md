# DDL Export Compatibility Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a required export compatibility selector (DataGrip vs DBeaver/SQLark) and generate DM8-compatible DDL/trigger output per selection.

**Architecture:** Frontend adds a required `export_compat` field on the export form and sends it with each export request. Backend accepts the field and uses it to control trigger terminators and normalize trigger/index output during DDL generation, defaulting to DataGrip behavior for older clients.

**Tech Stack:** Rust (Axum backend, unit tests), React + Ant Design (frontend), Vite.

---

### Task 1: Add DDL compatibility tests (backend)

**Files:**
- Modify: `backend/src/export/ddl.rs`
- Test: `backend/src/export/ddl.rs`

**Step 1: Write the failing tests**

Add tests that assert:
- DataGrip mode ends triggers with `;` only (no `/`).
- Script mode ends triggers with `;` plus a separate `/` line.
- `NEW.`/`OLD.` are normalized to `:NEW.`/`:OLD.` in WHEN and body.
- `INDEX` + digits is renamed to `IDX_<TABLE>_<COLS>`.

Example test structure:
```rust
#[test]
fn generate_triggers_datagrip_has_no_slash() {
    let triggers = vec![TriggerDefinition { /* ... */ }];
    let out = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
    assert!(out[0].trim_end().ends_with(';'));
    assert!(!out[0].contains("\n/"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test export::ddl::tests::generate_triggers_datagrip_has_no_slash`
Expected: FAIL (function signature missing, or terminator not controlled).

**Step 3: Write minimal implementation**

Add a `TriggerTerminator` enum and thread it into `generate_triggers` and `export_schema_ddl`.
Ensure trigger output formatting and index renaming obey tests.

**Step 4: Run tests to verify they pass**

Run: `cargo test export::ddl::tests::generate_triggers_datagrip_has_no_slash`
Expected: PASS

**Step 5: Commit**

```bash
git add backend/src/export/ddl.rs
git commit -m "fix: add ddl compatibility modes for triggers and indexes"
```

---

### Task 2: Thread compatibility mode through API request

**Files:**
- Modify: `backend/src/models/mod.rs`
- Modify: `backend/src/api/export.rs`
- Modify: `frontend/src/types/index.ts`
- Modify: `frontend/src/services/api.ts`

**Step 1: Write the failing test**

Add a small unit test for a new helper that maps `Option<String>` to a compat enum, defaulting to DataGrip when missing.
```rust
#[test]
fn resolve_compat_defaults_to_datagrip() {
    assert_eq!(resolve_compat(None), TriggerTerminator::DataGrip);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test api::export::tests::resolve_compat_defaults_to_datagrip`
Expected: FAIL (function not found).

**Step 3: Write minimal implementation**

- Add `export_compat: Option<String>` to `ExportRequest`.
- Add `resolve_compat` in `api/export.rs` to map `"datagrip" | "script"`.
- Pass the resolved value into `export_schema_ddl`.

**Step 4: Run tests to verify they pass**

Run: `cargo test api::export::tests::resolve_compat_defaults_to_datagrip`
Expected: PASS

**Step 5: Commit**

```bash
git add backend/src/models/mod.rs backend/src/api/export.rs frontend/src/types/index.ts frontend/src/services/api.ts
git commit -m "feat: add export compatibility to ddl requests"
```

---

### Task 3: Frontend export form required selector

**Files:**
- Modify: `frontend/src/components/ExportConfig.tsx`

**Step 1: Write the failing test**

No frontend test runner exists. If required, add a minimal test harness; otherwise record manual validation steps.

**Step 2: Implement minimal UI change**

- Add a `Select` or `Radio` field named `export_compat` with options:
  - DataGrip (value: `datagrip`)
  - DBeaver/SQLark (value: `script`)
- Add `rules={[{ required: true, message: '请选择导出兼容模式' }]}`
- Ensure request includes `export_compat: values.export_compat`.

**Step 3: Manual verification**

- Leave the selector empty, click export: expect validation error and no request.
- Choose DataGrip: export request succeeds and DDL contains no `/`.
- Choose DBeaver/SQLark: export request succeeds and DDL contains `/` after triggers.

**Step 4: Commit**

```bash
git add frontend/src/components/ExportConfig.tsx
git commit -m "feat: require export compatibility selection"
```
