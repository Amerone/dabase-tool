# Phase-1 Multi-DB QA Checklist (Connection + Browsing)

## Goal

Validate phase-1 behavior:

1. Connection test works by `db_type`
2. Table list works
3. Table details (columns + primary keys) works
4. Non-DM8 export is blocked with clear message
5. Existing DM8 flow is not regressed

## Test Matrix

| DB Type | Driver Type | Phase-1 Scope |
| --- | --- | --- |
| `dm8` | ODBC (existing) | connection + browsing + export |
| `mysql` | native `sqlx/mysql` | connection + browsing |
| `kingbase` | ODBC generic | connection + browsing |
| `shentong` | ODBC generic | connection + browsing |

## Environment Preconditions

1. Backend started on `127.0.0.1:3000`
2. Frontend started on `127.0.0.1:5173` (or 5174)
3. Databases prepared with at least:
   - 2 tables
   - 1 table with primary key
4. For ODBC DBs:
   - Driver installed and registered
   - Driver visible in ODBC manager

## Common UI Verification

1. Open connection page.
2. Verify new `DB TYPE` selector exists.
3. Switch DB type and verify default port changes:
   - DM8: `5236`
   - MySQL: `3306`
   - Kingbase: `54321`
   - Shentong: `2003`

Expected:

1. Selector renders correctly
2. Port auto-fills by type
3. Existing fields remain editable

## DM8 Regression Case

1. Select `DM8`.
2. Fill valid connection and click test.
3. Go to table browsing page.
4. Verify table list loads.
5. Open a table detail entry.
6. Run DDL export and data export.

Expected:

1. Connection success
2. Table list shown
3. Detail returns columns/PK
4. Export still succeeds for DM8

## MySQL Case

1. Select `MySQL`.
2. Fill host/port/user/password/schema(database).
3. Test connection.
4. Browse tables.
5. Open table details for one table.
6. Try export.

Expected:

1. Connection success
2. Table list from `information_schema.TABLES`
3. Details include columns and PK (indexes/fk/check/triggers may be empty in phase-1)
4. Export blocked with phase-1 message

## Kingbase Case (ODBC)

1. Select `Kingbase`.
2. Fill connection values.
3. Test connection.
4. Browse tables.
5. Open table details.
6. Try export.

Expected:

1. Connection success
2. Table list loaded
3. Details include columns and PK if catalog supports standard `information_schema`
4. Export blocked with phase-1 message

## Shentong Case (ODBC)

1. Select `Shentong`.
2. Fill connection values.
3. Test connection.
4. Browse tables.
5. Open table details.
6. Try export.

Expected:

1. Connection success
2. Table list loaded
3. Details include columns and PK if catalog supports standard `information_schema`
4. Export blocked with phase-1 message

## Config Persistence Checks

1. Save a MySQL config.
2. Reload page and load saved config.
3. Verify `db_type` is restored as `mysql`.
4. Save Kingbase config, reload and verify it is restored.

Expected:

1. Saved config includes `db_type`
2. Most recently updated config is returned by default API

## API Smoke Checks (Optional)

Use backend API directly:

1. `POST /api/connection/test` with each `db_type`
2. `GET /api/tables?...&db_type=<type>`
3. `GET /api/tables/<table>/details?...&db_type=<type>`
4. `POST /api/export/ddl` and `/api/export/data` with non-DM8 config

Expected:

1. Connection and browsing endpoints route by `db_type`
2. Non-DM8 export returns clear "not supported yet" message

## Failure Log Template

Record for each failed case:

1. DB type
2. Request payload (hide password)
3. Error message from UI/API
4. Whether issue is driver setup, auth, network, or SQL compatibility
