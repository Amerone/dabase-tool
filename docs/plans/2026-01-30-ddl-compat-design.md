# DDL Export Compatibility Design

## Goal
Add a required export-compatibility selector so each export can generate DDL that runs cleanly in the chosen SQL client (DataGrip or DBeaver/SQLark). Ensure trigger output is compatible (END + optional slash), and avoid DM8 invalid index name errors by renaming INDEX+digits to a safe pattern. The selection applies to DDL and initial data export and is not persisted in connection config.

## Scope
- Frontend: add a required selector on the export form; no default value; block export until chosen; send value in export request.
- Backend: accept the new field, default to DataGrip behavior if missing; pass mode into DDL generation; update trigger and index output to be DM8-compatible.

## Data Flow
1. User opens Export page and must select export compatibility (DataGrip or Script).
2. Frontend sends `export_compat` with the export request (no persistence).
3. Backend resolves target schema as today, and forwards `export_compat` to DDL generation.
4. DDL generator formats triggers based on the mode:
   - DataGrip: end with `END;` only.
   - Script: end with `END;` plus a new line `/`.
5. DDL generator normalizes trigger references and index names.

## Key Behaviors
- Triggers:
  - Add `REFERENCING OLD AS OLD NEW AS NEW` for row-level triggers.
  - Normalize `NEW.` / `OLD.` to `:NEW.` / `:OLD.` in WHEN and body.
  - Normalize trigger body statements to include semicolons.
  - Terminator behavior depends on mode (DataGrip vs Script).
- Indexes:
  - If index name matches `INDEX` + digits, rename to `IDX_<TABLE>_<COLS>`.
  - Keep column order; cap identifier length to a safe limit (128).

## Error Handling
- If `export_compat` is missing (older clients), default to DataGrip mode.
- Export still fails fast if connection or metadata queries fail; errors returned as before.

## Testing
- Backend unit tests for:
  - Trigger output in DataGrip mode (no `/`).
  - Trigger output in Script mode (has `/`).
  - Trigger normalization for WHEN and body references.
  - Index renaming for `INDEX` + digits.
- Manual UI verification for required select (no frontend test infra).
