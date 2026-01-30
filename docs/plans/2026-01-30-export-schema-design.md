# Export Target Schema Input

Date: 2026-01-30

## Summary
Add a user-configurable "export target schema" that is saved with the connection configuration and used only for generated SQL (DDL and data scripts). The source schema remains the connection schema used for metadata and data reads.

## Goals
- Allow users to specify a target schema for generated SQL without changing the source schema used to read data/metadata.
- Persist the target schema with saved connection config and auto-fill it on load.
- Use the target schema in DDL/data SQL generation and in output filenames.
- Keep backward compatibility with older clients that do not send the new field.

## Non-Goals
- Switching the source schema for querying tables or exporting data.
- Automatic schema mapping beyond a single target schema.
- SQL post-processing text replacement.

## UX Changes
- Add an input field in the export page for "Export Schema" with default value:
  - `connectionConfig.export_schema` if present, otherwise `connectionConfig.schema`.
- Add an input field in the connection form so the value is saved/loaded with the connection config.
- Allow empty input; if empty, the backend defaults to the source schema.

## Data Model Changes
Frontend (`ConnectionConfig`, `ExportRequest`):
- Add `export_schema?: string` to connection config.
- Add `export_schema?: string` to export request.

Backend (`ConnectionConfig`, `ExportRequest`):
- Add `export_schema: Option<String>` to export request (or to config if stored server-side).
- Ensure serde defaults allow missing fields.

## API and Flow
- Export request includes `export_schema`.
- Backend determines:
  - `source_schema = req.config.schema`
  - `target_schema = req.export_schema.unwrap_or(req.config.schema.clone())`
- Export generation uses `target_schema` for SQL schema-qualified identifiers.
- Reads remain against `source_schema`.

## Output File Naming
Use both schemas:
- DDL: `exports/<source>_to_<target>_ddl_<timestamp>.sql`
- Data: `exports/<source>_to_<target>_data_<timestamp>.sql`
If target is empty, treat as source.

## Backend Changes
- `ExportRequest` struct extended to include `export_schema`.
- `export_ddl`/`export_data` compute `target_schema` and pass it to:
  - `export_schema_ddl` (new parameter)
  - `export_schema_data` (new parameter)
- `export_schema_ddl`/`export_schema_data` use `target_schema` in SQL generation:
  - Table names, sequence statements, trigger statements, TRUNCATE/INSERT targets.
- Keep metadata fetch and data reads using `source_schema`.

## Frontend Changes
- Update types to include `export_schema`.
- `ConnectionForm` adds "Export Schema" input and persists value on save/load.
- `ExportConfig` adds "Export Schema" input and includes value in export request.
- Optional: update store to keep `export_schema` in `connectionConfig` for consistent defaults.

## Error Handling
- If `export_schema` is empty or missing, fallback to `config.schema`.
- No change to existing error messaging; keep ApiResponse error format.

## Testing
- Unit tests (backend):
  - Verify target schema used for DDL/data SQL generation.
  - Verify default fallback when `export_schema` is missing.
- Manual test:
  - Load connection, set export schema different from source, export DDL and data, verify SQL uses target schema and file name includes `source_to_target`.

## Backward Compatibility
- Older clients omit `export_schema`; backend defaults to source schema.
- Existing saved configs without `export_schema` remain valid.
