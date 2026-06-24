# DM8 Identity Cleanup SQL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and run a read-only DM8 schema comparison script that finds tables whose source schema uses identity primary keys while the target schema has removed identity behavior, then writes the cleanup SQL for `DROP IDENTITY`, table triggers, and related sequences.

**Architecture:** Keep this as a standalone operational script under `scripts/` so it does not change the Rust backend or React UI. The script reads metadata from both DM8 schemas, compares source objects against target objects, and writes a reviewed SQL artifact; it never executes generated `ALTER TABLE`, `DROP TRIGGER`, or `DROP SEQUENCE` statements.

**Tech Stack:** Python 3 standard library, official `dmPython` driver for live DM8 metadata reads, `unittest` for pure comparison tests, PowerShell for Windows execution commands.

---

## File Structure

- Create: `scripts/generate_dm8_identity_cleanup_sql.py`
  - Responsibility: connect to the two DM8 instances, query table/column/trigger/sequence metadata, compare objects, and write SQL.
  - Safety boundary: only runs `SELECT` statements against DM8; generated DDL is written to a file.
- Create: `scripts/tests/test_generate_dm8_identity_cleanup_sql.py`
  - Responsibility: lock the SQL generation and comparison rules without requiring a live database.
- Create during execution: `docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql`
  - Responsibility: final SQL artifact for manual review and later execution by a DBA or controlled migration step.
- Reference: `backend/src/db/schema.rs:240`
  - Existing DM8 identity detection uses `SYS.SYSCOLUMNS.INFO2 & 1`.
- Reference: `backend/src/db/schema.rs:954`
  - Existing DM8 sequence query reads from `ALL_SEQUENCES`.
- Reference: `backend/src/db/schema.rs:996`
  - Existing DM8 trigger query reads from `ALL_TRIGGERS` and already handles missing trigger metadata columns.

## Assumptions

- Source database is the schema that still has identity primary keys:
  - host `192.168.0.122`
  - port `5237`
  - schema `PLATFORM`
  - user `PLATFORM`
- Target database is the schema where identity behavior and table-related triggers have already been removed:
  - host `192.168.3.181`
  - port `5236`
  - schema `PLATFORM`
  - user `PLATFORM`
- Database passwords must be supplied through process environment variables at runtime and must not be committed into the repository.
- Generated SQL targets the source schema so it can be brought into alignment with the target schema.

## Comparison Rules

1. Generate `ALTER TABLE "SCHEMA"."TABLE" MODIFY "COLUMN" DROP IDENTITY;` when:
   - source has an identity column according to `SYS.SYSCOLUMNS.INFO2 & 1`;
   - target has the same table and column;
   - target does not mark that column as identity.
2. Preserve the primary key constraint by not generating any `DROP CONSTRAINT` statement.
3. Generate `DROP TRIGGER "SCHEMA"."TRIGGER";` when:
   - the trigger belongs to a table that needs `DROP IDENTITY`;
   - the trigger exists in source;
   - the same trigger name does not exist in target for the same table.
4. Generate `DROP SEQUENCE "SCHEMA"."SEQUENCE";` only when:
   - the sequence exists in source and not in target;
   - the sequence is referenced by a dropped trigger body through `.NEXTVAL`, or the sequence name clearly matches the affected table and identity column naming patterns.
5. Emit comments before every generated statement explaining why it was selected.

---

### Task 1: Add Pure Comparison Tests

**Files:**
- Create: `scripts/tests/test_generate_dm8_identity_cleanup_sql.py`
- Later implementation under test: `scripts/generate_dm8_identity_cleanup_sql.py`

- [ ] **Step 1: Create the test file**

Use `apply_patch` to create `scripts/tests/test_generate_dm8_identity_cleanup_sql.py` with this content:

```python
import unittest

from scripts.generate_dm8_identity_cleanup_sql import (
    ColumnMeta,
    SequenceMeta,
    TriggerMeta,
    build_cleanup_plan,
    extract_nextval_sequences,
    quote_qualified,
)


class Dm8IdentityCleanupSqlTests(unittest.TestCase):
    def test_quote_qualified_escapes_embedded_quotes(self):
        self.assertEqual(
            quote_qualified("PLAT\"FORM", "USER"),
            '"PLAT""FORM"."USER"',
        )

    def test_extract_nextval_sequences_from_unquoted_trigger_body(self):
        body = "SELECT SEQ_USER.NEXTVAL INTO :NEW.ID FROM DUAL;"
        self.assertEqual(extract_nextval_sequences(body), {"SEQ_USER"})

    def test_extract_nextval_sequences_from_schema_qualified_trigger_body(self):
        body = 'SELECT "PLATFORM"."SEQ_ORDER".NEXTVAL INTO :NEW.ID FROM DUAL;'
        self.assertEqual(extract_nextval_sequences(body), {"SEQ_ORDER"})

    def test_generates_drop_identity_for_source_identity_absent_in_target(self):
        plan = build_cleanup_plan(
            source_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", True, True),
            ],
            target_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", False, True),
            ],
            source_triggers=[],
            target_triggers=[],
            source_sequences=[],
            target_sequences=[],
        )

        self.assertEqual(
            plan.identity_sql,
            [
                '-- APP_USER.ID is identity in source and non-identity in target; primary key is preserved.',
                'ALTER TABLE "PLATFORM"."APP_USER" MODIFY "ID" DROP IDENTITY;',
            ],
        )

    def test_skips_identity_when_target_is_also_identity(self):
        plan = build_cleanup_plan(
            source_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", True, True),
            ],
            target_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", True, True),
            ],
            source_triggers=[],
            target_triggers=[],
            source_sequences=[],
            target_sequences=[],
        )

        self.assertEqual(plan.identity_sql, [])
        self.assertEqual(plan.trigger_sql, [])
        self.assertEqual(plan.sequence_sql, [])

    def test_drops_source_trigger_absent_from_target_on_affected_table(self):
        plan = build_cleanup_plan(
            source_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", True, True),
            ],
            target_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", False, True),
            ],
            source_triggers=[
                TriggerMeta("PLATFORM", "TRG_APP_USER_BI", "APP_USER", ""),
            ],
            target_triggers=[],
            source_sequences=[],
            target_sequences=[],
        )

        self.assertEqual(
            plan.trigger_sql,
            [
                "-- TRG_APP_USER_BI exists on source table APP_USER and is absent from target.",
                'DROP TRIGGER "PLATFORM"."TRG_APP_USER_BI";',
            ],
        )

    def test_drops_sequence_referenced_by_removed_trigger(self):
        plan = build_cleanup_plan(
            source_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", True, True),
            ],
            target_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", False, True),
            ],
            source_triggers=[
                TriggerMeta(
                    "PLATFORM",
                    "TRG_APP_USER_BI",
                    "APP_USER",
                    "SELECT SEQ_APP_USER.NEXTVAL INTO :NEW.ID FROM DUAL;",
                ),
            ],
            target_triggers=[],
            source_sequences=[
                SequenceMeta("PLATFORM", "SEQ_APP_USER"),
                SequenceMeta("PLATFORM", "SEQ_UNRELATED"),
            ],
            target_sequences=[],
        )

        self.assertEqual(
            plan.sequence_sql,
            [
                "-- SEQ_APP_USER is referenced by removed trigger TRG_APP_USER_BI and is absent from target.",
                'DROP SEQUENCE "PLATFORM"."SEQ_APP_USER";',
            ],
        )

    def test_does_not_drop_sequence_present_in_target(self):
        plan = build_cleanup_plan(
            source_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", True, True),
            ],
            target_columns=[
                ColumnMeta("PLATFORM", "APP_USER", "ID", False, True),
            ],
            source_triggers=[
                TriggerMeta(
                    "PLATFORM",
                    "TRG_APP_USER_BI",
                    "APP_USER",
                    "SELECT SEQ_APP_USER.NEXTVAL INTO :NEW.ID FROM DUAL;",
                ),
            ],
            target_triggers=[],
            source_sequences=[SequenceMeta("PLATFORM", "SEQ_APP_USER")],
            target_sequences=[SequenceMeta("PLATFORM", "SEQ_APP_USER")],
        )

        self.assertEqual(plan.sequence_sql, [])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run the test to verify it fails for the expected reason**

Run:

```powershell
python -m unittest discover -s scripts/tests -p "test_generate_dm8_identity_cleanup_sql.py" -v
```

Expected result:

```text
ModuleNotFoundError: No module named 'scripts.generate_dm8_identity_cleanup_sql'
```

- [ ] **Step 3: Commit the failing test**

Run only if the worktree can stage just the new test file:

```powershell
git add scripts/tests/test_generate_dm8_identity_cleanup_sql.py
git commit -m "Require deterministic DM8 identity cleanup planning" -m "The cleanup generator needs pure tests before touching live metadata so SQL generation can be reviewed without database access.

Constraint: Live DM8 credentials must not be committed
Confidence: high
Scope-risk: narrow
Directive: Keep live database access out of unit tests
Tested: unittest fails because generator module does not exist yet
Not-tested: live DM8 metadata queries"
```

Expected result:

```text
git exits with status 0 and prints a one-line commit summary for "Require deterministic DM8 identity cleanup planning".
```

---

### Task 2: Implement the Standalone Metadata Comparison Script

**Files:**
- Create: `scripts/generate_dm8_identity_cleanup_sql.py`
- Test: `scripts/tests/test_generate_dm8_identity_cleanup_sql.py`

- [ ] **Step 1: Create the script**

Use `apply_patch` to create `scripts/generate_dm8_identity_cleanup_sql.py` with this content:

```python
#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


NEXTVAL_RE = re.compile(
    r'(?:(?:"?[A-Z][A-Z0-9_$#]*"?\s*\.\s*)?"?([A-Z][A-Z0-9_$#]*)"?\s*\.\s*NEXTVAL)',
    re.IGNORECASE,
)


@dataclass(frozen=True)
class ColumnMeta:
    schema: str
    table: str
    column: str
    identity: bool
    primary_key: bool


@dataclass(frozen=True)
class TriggerMeta:
    schema: str
    name: str
    table: str
    body: str


@dataclass(frozen=True)
class SequenceMeta:
    schema: str
    name: str


@dataclass(frozen=True)
class CleanupPlan:
    identity_sql: list[str]
    trigger_sql: list[str]
    sequence_sql: list[str]

    def all_sql(self) -> list[str]:
        lines: list[str] = [
            "-- Generated by scripts/generate_dm8_identity_cleanup_sql.py",
            "-- Review before execution. This file preserves primary key constraints.",
            "",
        ]
        if self.identity_sql:
            lines.extend(["-- Identity columns", *self.identity_sql, ""])
        if self.trigger_sql:
            lines.extend(["-- Table triggers", *self.trigger_sql, ""])
        if self.sequence_sql:
            lines.extend(["-- Related sequences", *self.sequence_sql, ""])
        if not self.identity_sql and not self.trigger_sql and not self.sequence_sql:
            lines.append("-- No cleanup SQL generated.")
        return lines


def normalize_name(value: str) -> str:
    return value.strip().strip('"').upper()


def quote_ident(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def quote_qualified(schema: str, name: str) -> str:
    return f"{quote_ident(schema)}.{quote_ident(name)}"


def sql_literal(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def extract_nextval_sequences(trigger_body: str) -> set[str]:
    return {normalize_name(match.group(1)) for match in NEXTVAL_RE.finditer(trigger_body or "")}


def sequence_name_matches_table(sequence_name: str, table_name: str, column_name: str) -> bool:
    seq = normalize_name(sequence_name)
    table = normalize_name(table_name)
    column = normalize_name(column_name)
    exact_names = {
        f"SEQ_{table}",
        f"{table}_SEQ",
        f"SEQ_{table}_{column}",
        f"{table}_{column}_SEQ",
        f"{table}_{column}_ID_SEQ",
        f"SEQ_{table}_ID",
        f"S_{table}",
    }
    return seq in exact_names or (table in seq and seq.startswith(("SEQ_", "S_")))


def key(schema: str, name: str) -> tuple[str, str]:
    return normalize_name(schema), normalize_name(name)


def table_key(schema: str, table: str) -> tuple[str, str]:
    return key(schema, table)


def column_key(column: ColumnMeta) -> tuple[str, str, str]:
    return normalize_name(column.schema), normalize_name(column.table), normalize_name(column.column)


def trigger_key(trigger: TriggerMeta) -> tuple[str, str, str]:
    return (
        normalize_name(trigger.schema),
        normalize_name(trigger.table),
        normalize_name(trigger.name),
    )


def sequence_key(sequence: SequenceMeta) -> tuple[str, str]:
    return key(sequence.schema, sequence.name)


def build_cleanup_plan(
    source_columns: Iterable[ColumnMeta],
    target_columns: Iterable[ColumnMeta],
    source_triggers: Iterable[TriggerMeta],
    target_triggers: Iterable[TriggerMeta],
    source_sequences: Iterable[SequenceMeta],
    target_sequences: Iterable[SequenceMeta],
) -> CleanupPlan:
    source_columns = list(source_columns)
    target_by_column = {column_key(col): col for col in target_columns}
    target_trigger_keys = {trigger_key(trigger) for trigger in target_triggers}
    target_sequence_keys = {sequence_key(sequence) for sequence in target_sequences}

    affected_columns: list[ColumnMeta] = []
    identity_sql: list[str] = []
    for source_col in sorted(
        source_columns,
        key=lambda item: (normalize_name(item.schema), normalize_name(item.table), normalize_name(item.column)),
    ):
        if not source_col.identity:
            continue
        target_col = target_by_column.get(column_key(source_col))
        if target_col is None or target_col.identity:
            continue
        affected_columns.append(source_col)
        identity_sql.extend(
            [
                f"-- {source_col.table}.{source_col.column} is identity in source and non-identity in target; primary key is preserved.",
                f"ALTER TABLE {quote_qualified(source_col.schema, source_col.table)} MODIFY {quote_ident(source_col.column)} DROP IDENTITY;",
            ]
        )

    affected_table_keys = {table_key(col.schema, col.table) for col in affected_columns}
    identity_column_by_table = {table_key(col.schema, col.table): col for col in affected_columns}

    removed_triggers: list[TriggerMeta] = []
    trigger_sql: list[str] = []
    for trigger in sorted(
        source_triggers,
        key=lambda item: (normalize_name(item.schema), normalize_name(item.table), normalize_name(item.name)),
    ):
        if table_key(trigger.schema, trigger.table) not in affected_table_keys:
            continue
        if trigger_key(trigger) in target_trigger_keys:
            continue
        removed_triggers.append(trigger)
        trigger_sql.extend(
            [
                f"-- {trigger.name} exists on source table {trigger.table} and is absent from target.",
                f"DROP TRIGGER {quote_qualified(trigger.schema, trigger.name)};",
            ]
        )

    referenced_sequences_by_trigger: dict[str, str] = {}
    for trigger in removed_triggers:
        for sequence_name in extract_nextval_sequences(trigger.body):
            referenced_sequences_by_trigger[sequence_name] = trigger.name

    sequence_sql: list[str] = []
    for sequence in sorted(
        source_sequences,
        key=lambda item: (normalize_name(item.schema), normalize_name(item.name)),
    ):
        normalized_sequence = normalize_name(sequence.name)
        if sequence_key(sequence) in target_sequence_keys:
            continue
        referenced_by = referenced_sequences_by_trigger.get(normalized_sequence)
        matched_by_name = False
        matched_table = ""
        for affected_key, affected_column in identity_column_by_table.items():
            if affected_key[0] != normalize_name(sequence.schema):
                continue
            if sequence_name_matches_table(sequence.name, affected_column.table, affected_column.column):
                matched_by_name = True
                matched_table = affected_column.table
                break
        if referenced_by:
            reason = f"{sequence.name} is referenced by removed trigger {referenced_by} and is absent from target."
        elif matched_by_name:
            reason = f"{sequence.name} name matches affected table {matched_table} and is absent from target."
        else:
            continue
        sequence_sql.extend(
            [
                f"-- {reason}",
                f"DROP SEQUENCE {quote_qualified(sequence.schema, sequence.name)};",
            ]
        )

    return CleanupPlan(identity_sql, trigger_sql, sequence_sql)


def import_dm_python():
    try:
        import dmPython  # type: ignore
    except ImportError as exc:
        raise SystemExit("dmPython is required for live DM8 metadata reads. Install or activate the DM8 Python driver.") from exc
    return dmPython


def connect(args: argparse.Namespace, prefix: str):
    dm_python = import_dm_python()
    return dm_python.connect(
        host=getattr(args, f"{prefix}_host"),
        port=getattr(args, f"{prefix}_port"),
        user=getattr(args, f"{prefix}_user"),
        password=getattr(args, f"{prefix}_password"),
        database=getattr(args, f"{prefix}_schema"),
    )


def fetch_dicts(connection, sql: str) -> list[dict[str, object]]:
    cursor = connection.cursor()
    try:
        cursor.execute(sql)
        columns = [desc[0].upper() for desc in cursor.description]
        return [dict(zip(columns, row)) for row in cursor.fetchall()]
    finally:
        cursor.close()


def fetch_columns(connection, schema: str) -> list[ColumnMeta]:
    schema_literal = sql_literal(normalize_name(schema))
    sql = f"""
SELECT
    c.OWNER,
    c.TABLE_NAME,
    c.COLUMN_NAME,
    CASE WHEN sc.INFO2 & 1 = 1 THEN 'Y' ELSE 'N' END AS IDENTITY_COLUMN,
    CASE WHEN pk.CONSTRAINT_NAME IS NULL THEN 'N' ELSE 'Y' END AS PRIMARY_KEY
FROM ALL_TAB_COLUMNS c
LEFT JOIN SYS.SYSOBJECTS sch
    ON sch.NAME = c.OWNER AND sch.TYPE$ = 'SCH'
LEFT JOIN SYS.SYSOBJECTS so
    ON so.NAME = c.TABLE_NAME AND so.SCHID = sch.ID AND so.TYPE$ = 'SCHOBJ'
LEFT JOIN SYS.SYSCOLUMNS sc
    ON sc.ID = so.ID AND sc.NAME = c.COLUMN_NAME
LEFT JOIN ALL_CONS_COLUMNS pkc
    ON pkc.OWNER = c.OWNER
    AND pkc.TABLE_NAME = c.TABLE_NAME
    AND pkc.COLUMN_NAME = c.COLUMN_NAME
LEFT JOIN ALL_CONSTRAINTS pk
    ON pk.OWNER = pkc.OWNER
    AND pk.CONSTRAINT_NAME = pkc.CONSTRAINT_NAME
    AND pk.CONSTRAINT_TYPE = 'P'
WHERE c.OWNER = {schema_literal}
ORDER BY c.TABLE_NAME, c.COLUMN_ID
"""
    rows = fetch_dicts(connection, sql)
    return [
        ColumnMeta(
            normalize_name(str(row["OWNER"])),
            normalize_name(str(row["TABLE_NAME"])),
            normalize_name(str(row["COLUMN_NAME"])),
            normalize_name(str(row["IDENTITY_COLUMN"])) == "Y",
            normalize_name(str(row["PRIMARY_KEY"])) == "Y",
        )
        for row in rows
    ]


def fetch_triggers(connection, schema: str) -> list[TriggerMeta]:
    schema_literal = sql_literal(normalize_name(schema))
    full_sql = f"""
SELECT TRIGGER_NAME, TABLE_NAME, TRIGGER_BODY
FROM ALL_TRIGGERS
WHERE TABLE_OWNER = {schema_literal}
ORDER BY TABLE_NAME, TRIGGER_NAME
"""
    fallback_sql = f"""
SELECT TRIGGER_NAME, TABLE_NAME
FROM ALL_TRIGGERS
WHERE TABLE_OWNER = {schema_literal}
ORDER BY TABLE_NAME, TRIGGER_NAME
"""
    try:
        rows = fetch_dicts(connection, full_sql)
        return [
            TriggerMeta(
                normalize_name(schema),
                normalize_name(str(row["TRIGGER_NAME"])),
                normalize_name(str(row["TABLE_NAME"])),
                "" if row.get("TRIGGER_BODY") is None else str(row.get("TRIGGER_BODY")),
            )
            for row in rows
        ]
    except Exception:
        rows = fetch_dicts(connection, fallback_sql)
        return [
            TriggerMeta(
                normalize_name(schema),
                normalize_name(str(row["TRIGGER_NAME"])),
                normalize_name(str(row["TABLE_NAME"])),
                "",
            )
            for row in rows
        ]


def fetch_sequences(connection, schema: str) -> list[SequenceMeta]:
    schema_literal = sql_literal(normalize_name(schema))
    sql = f"""
SELECT SEQUENCE_OWNER, SEQUENCE_NAME
FROM ALL_SEQUENCES
WHERE SEQUENCE_OWNER = {schema_literal}
ORDER BY SEQUENCE_NAME
"""
    return [
        SequenceMeta(normalize_name(str(row["SEQUENCE_OWNER"])), normalize_name(str(row["SEQUENCE_NAME"])))
        for row in fetch_dicts(connection, sql)
    ]


def read_metadata(args: argparse.Namespace, prefix: str) -> tuple[list[ColumnMeta], list[TriggerMeta], list[SequenceMeta]]:
    connection = connect(args, prefix)
    schema = getattr(args, f"{prefix}_schema")
    try:
        return (
            fetch_columns(connection, schema),
            fetch_triggers(connection, schema),
            fetch_sequences(connection, schema),
        )
    finally:
        connection.close()


def env_default(name: str, default: str | None = None) -> str | None:
    return os.environ.get(name, default)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Generate DM8 SQL to remove identity behavior by comparing two schemas.")
    parser.add_argument("--src-host", default=env_default("DM8_SRC_HOST", "192.168.0.122"))
    parser.add_argument("--src-port", type=int, default=int(env_default("DM8_SRC_PORT", "5237")))
    parser.add_argument("--src-schema", default=env_default("DM8_SRC_SCHEMA", "PLATFORM"))
    parser.add_argument("--src-user", default=env_default("DM8_SRC_USER", "PLATFORM"))
    parser.add_argument("--src-password", default=env_default("DM8_SRC_PASSWORD"))
    parser.add_argument("--target-host", default=env_default("DM8_TARGET_HOST", "192.168.3.181"))
    parser.add_argument("--target-port", type=int, default=int(env_default("DM8_TARGET_PORT", "5236")))
    parser.add_argument("--target-schema", default=env_default("DM8_TARGET_SCHEMA", "PLATFORM"))
    parser.add_argument("--target-user", default=env_default("DM8_TARGET_USER", "PLATFORM"))
    parser.add_argument("--target-password", default=env_default("DM8_TARGET_PASSWORD"))
    parser.add_argument("--output", required=True, help="Path to write generated SQL.")
    return parser


def require_passwords(args: argparse.Namespace) -> None:
    missing = []
    if not args.src_password:
        missing.append("DM8_SRC_PASSWORD or --src-password")
    if not args.target_password:
        missing.append("DM8_TARGET_PASSWORD or --target-password")
    if missing:
        raise SystemExit("Missing required password input: " + ", ".join(missing))


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    require_passwords(args)

    source_columns, source_triggers, source_sequences = read_metadata(args, "src")
    target_columns, target_triggers, target_sequences = read_metadata(args, "target")

    plan = build_cleanup_plan(
        source_columns=source_columns,
        target_columns=target_columns,
        source_triggers=source_triggers,
        target_triggers=target_triggers,
        source_sequences=source_sequences,
        target_sequences=target_sequences,
    )

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text("\n".join(plan.all_sql()) + "\n", encoding="utf-8")

    print(f"Wrote SQL to {output_path}")
    print(f"identity_statement_count={len(plan.identity_sql) // 2}")
    print(f"trigger_statement_count={len(plan.trigger_sql) // 2}")
    print(f"sequence_statement_count={len(plan.sequence_sql) // 2}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Run the unit tests**

Run:

```powershell
python -m unittest discover -s scripts/tests -p "test_generate_dm8_identity_cleanup_sql.py" -v
```

Expected result:

```text
Ran 8 tests

OK
```

- [ ] **Step 3: Run Python syntax verification**

Run:

```powershell
python -m py_compile scripts/generate_dm8_identity_cleanup_sql.py scripts/tests/test_generate_dm8_identity_cleanup_sql.py
```

Expected result: command exits with status `0` and prints no output.

- [ ] **Step 4: Commit the implementation**

Run only after the tests pass and only stage the owned files:

```powershell
git add scripts/generate_dm8_identity_cleanup_sql.py scripts/tests/test_generate_dm8_identity_cleanup_sql.py
git commit -m "Generate DM8 identity cleanup SQL from schema diff" -m "The script compares source and target DM8 metadata using read-only catalog queries, then writes reviewable cleanup SQL instead of executing DDL directly.

Constraint: Cleanup DDL is destructive and must remain a generated artifact until reviewed
Rejected: Execute ALTER and DROP statements directly | too much operational risk for a metadata comparison task
Confidence: high
Scope-risk: narrow
Directive: Do not add DDL execution to this script without a separate approval gate and backup procedure
Tested: python -m unittest discover -s scripts/tests -p test_generate_dm8_identity_cleanup_sql.py -v
Tested: python -m py_compile scripts/generate_dm8_identity_cleanup_sql.py scripts/tests/test_generate_dm8_identity_cleanup_sql.py
Not-tested: live DM8 connectivity"
```

Expected result:

```text
git exits with status 0 and prints a one-line commit summary for "Generate DM8 identity cleanup SQL from schema diff".
```

---

### Task 3: Verify Live DM8 Read Access

**Files:**
- Use: `scripts/generate_dm8_identity_cleanup_sql.py`
- Create during execution: no committed file

- [ ] **Step 1: Set runtime-only connection variables**

Run in PowerShell. The password values are typed into the shell session and are not written to repo files:

```powershell
$env:DM8_SRC_HOST = "192.168.0.122"
$env:DM8_SRC_PORT = "5237"
$env:DM8_SRC_SCHEMA = "PLATFORM"
$env:DM8_SRC_USER = "PLATFORM"
$env:DM8_SRC_PASSWORD = Read-Host "Source DM8 password"

$env:DM8_TARGET_HOST = "192.168.3.181"
$env:DM8_TARGET_PORT = "5236"
$env:DM8_TARGET_SCHEMA = "PLATFORM"
$env:DM8_TARGET_USER = "PLATFORM"
$env:DM8_TARGET_PASSWORD = Read-Host "Target DM8 password"
```

Expected result: PowerShell returns to the prompt with no file changes.

- [ ] **Step 2: Confirm the Python DM8 driver is importable**

Run:

```powershell
python -c "import dmPython; print('dmPython OK')"
```

Expected result:

```text
dmPython OK
```

- [ ] **Step 3: Run a non-writing metadata smoke check against the source**

Run:

```powershell
python -c "import dmPython; c=dmPython.connect(host='$env:DM8_SRC_HOST', port=int('$env:DM8_SRC_PORT'), user='$env:DM8_SRC_USER', password='$env:DM8_SRC_PASSWORD', database='$env:DM8_SRC_SCHEMA'); cur=c.cursor(); cur.execute(\"SELECT COUNT(*) FROM ALL_TABLES WHERE OWNER='PLATFORM'\"); print('source_table_count=' + str(cur.fetchone()[0])); cur.close(); c.close()"
```

Expected result:

```text
source_table_count= followed by a decimal value greater than 0
```

- [ ] **Step 4: Run a non-writing metadata smoke check against the target**

Run:

```powershell
python -c "import dmPython; c=dmPython.connect(host='$env:DM8_TARGET_HOST', port=int('$env:DM8_TARGET_PORT'), user='$env:DM8_TARGET_USER', password='$env:DM8_TARGET_PASSWORD', database='$env:DM8_TARGET_SCHEMA'); cur=c.cursor(); cur.execute(\"SELECT COUNT(*) FROM ALL_TABLES WHERE OWNER='PLATFORM'\"); print('target_table_count=' + str(cur.fetchone()[0])); cur.close(); c.close()"
```

Expected result:

```text
target_table_count= followed by a decimal value greater than 0
```

---

### Task 4: Generate and Review the Cleanup SQL Artifact

**Files:**
- Use: `scripts/generate_dm8_identity_cleanup_sql.py`
- Create: `docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql`

- [ ] **Step 1: Generate SQL**

Run:

```powershell
python scripts/generate_dm8_identity_cleanup_sql.py --output docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql
```

Expected result:

```text
Wrote SQL to docs\dm8-identity-cleanup\2026-04-27-platform-remove-identity.sql
identity_statement_count= followed by a decimal value
trigger_statement_count= followed by a decimal value
sequence_statement_count= followed by a decimal value
```

- [ ] **Step 2: Inspect generated `ALTER TABLE` statements**

Run:

```powershell
Select-String -Path docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql -Pattern "DROP IDENTITY"
```

Expected result: every matched line is in this exact form:

```sql
ALTER TABLE "PLATFORM"."actual_table_name" MODIFY "actual_column_name" DROP IDENTITY;
```

- [ ] **Step 3: Inspect generated trigger drops**

Run:

```powershell
Select-String -Path docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql -Pattern "DROP TRIGGER"
```

Expected result: every matched line is in this exact form:

```sql
DROP TRIGGER "PLATFORM"."actual_trigger_name";
```

- [ ] **Step 4: Inspect generated sequence drops**

Run:

```powershell
Select-String -Path docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql -Pattern "DROP SEQUENCE"
```

Expected result: every matched line is in this exact form:

```sql
DROP SEQUENCE "PLATFORM"."actual_sequence_name";
```

- [ ] **Step 5: Confirm no primary key constraints are dropped**

Run:

```powershell
Select-String -Path docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql -Pattern "DROP CONSTRAINT|PRIMARY KEY" -CaseSensitive:$false
```

Expected result: no `ALTER TABLE ... DROP CONSTRAINT` statements are present; the only `PRIMARY KEY` text may appear in comments.

- [ ] **Step 6: Commit the reviewed SQL artifact if it is intended to be versioned**

Run only after reviewing the generated SQL:

```powershell
git add docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql
git commit -m "Record DM8 identity cleanup SQL for PLATFORM" -m "The generated artifact lists the DDL needed to align the source PLATFORM schema with the target schema that removed identity behavior and table-related automatic key objects.

Constraint: Generated SQL must be reviewed before execution against DM8
Confidence: medium
Scope-risk: moderate
Directive: Execute statements only after confirming backups and maintenance window
Tested: generated SQL reviewed for DROP IDENTITY, DROP TRIGGER, DROP SEQUENCE, and absence of primary key constraint drops
Not-tested: executing generated DDL against production-like source database"
```

Expected result:

```text
git exits with status 0 and prints a one-line commit summary for "Record DM8 identity cleanup SQL for PLATFORM".
```

---

### Task 5: Produce the Operator Handoff

**Files:**
- Read: `docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql`
- Modify: no file modification required

- [ ] **Step 1: Count generated statement types**

Run:

```powershell
$sql = Get-Content docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql
"drop_identity=" + (($sql | Select-String "DROP IDENTITY").Count)
"drop_trigger=" + (($sql | Select-String "DROP TRIGGER").Count)
"drop_sequence=" + (($sql | Select-String "DROP SEQUENCE").Count)
```

Expected result:

```text
drop_identity= followed by a decimal value
drop_trigger= followed by a decimal value
drop_sequence= followed by a decimal value
```

- [ ] **Step 2: List affected tables**

Run:

```powershell
Select-String -Path docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql -Pattern "DROP IDENTITY" | ForEach-Object {
    if ($_.Line -match 'ALTER TABLE "PLATFORM"\."([^"]+)" MODIFY "([^"]+)" DROP IDENTITY;') {
        "$($Matches[1]).$($Matches[2])"
    }
} | Sort-Object
```

Expected result:

```text
One line per affected identity column in the form table_name.column_name.
```

- [ ] **Step 3: Write the final handoff summary**

Report these exact items to the user:

```text
Changed files:
- scripts/generate_dm8_identity_cleanup_sql.py
- scripts/tests/test_generate_dm8_identity_cleanup_sql.py
- docs/dm8-identity-cleanup/2026-04-27-platform-remove-identity.sql

Verification:
- Unit tests passed with unittest.
- Python syntax check passed.
- Live source and target DM8 metadata queries succeeded.
- Generated SQL contains only DROP IDENTITY, DROP TRIGGER, DROP SEQUENCE, and comments.
- Generated SQL does not drop primary key constraints.

Remaining risks:
- Sequence-to-table relation is exact when the sequence is referenced by trigger body; name-pattern matches still require DBA review.
- Generated DDL has not been executed.
- A database backup and maintenance window are required before running the SQL.
```

## Self-Review

- Spec coverage: The plan covers both provided DM8 schemas, traverses schema objects, identifies source identity columns absent from target identity metadata, preserves primary key constraints, and generates SQL for identity, trigger, and sequence cleanup.
- Placeholder scan: No implementation task relies on undefined code or unstated error handling; live passwords are intentionally runtime-only secrets rather than repo content.
- Type consistency: `ColumnMeta`, `TriggerMeta`, `SequenceMeta`, and `CleanupPlan` are defined in Task 2 and used consistently by the Task 1 tests.
- Safety review: The implementation executes only `SELECT` metadata queries and writes SQL to disk; destructive DDL remains manual and review-gated.
