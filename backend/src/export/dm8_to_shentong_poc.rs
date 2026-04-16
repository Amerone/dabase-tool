use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::Connection;

use crate::{
    db::schema::{fetch_sequences, get_tables_details_batch},
    dialect::renderer_for,
    export::{
        ddl::{
            filter_sequences_for_tables, generate_check_constraints, generate_foreign_keys,
            generate_indexes, generate_primary_key, generate_triggers, generate_unique_constraints,
            TriggerTerminator,
        },
        orchestrator::LegacyExportPlan,
    },
    models::{Column, ConnectionConfig, DbType, Sequence, TableDetails},
};

use super::common::{apply_identifier_case, apply_sequence_case};

const SHENTONG_MAX_VARCHAR_BYTES: i32 = 8000;
const SHENTONG_INDEXABLE_FALLBACK_BYTES: i32 = 255;

/// Map a DM8 column type to its Shentong (OSCAR) equivalent.
///
/// Key mappings based on official compatibility table:
/// - BIGINT → NUMBER(19) (8-byte integer, most stable mapping)
/// - INT/INTEGER → INTEGER or NUMBER(10)
/// - VARCHAR/VARCHAR2 — when DM8 uses BYTE semantics (CHAR_USED='B'), multiply length
///   by 3 to account for UTF-8 multi-byte characters in Shentong
/// - NCHAR/NVARCHAR → CHAR/VARCHAR with expanded length
/// - TINYINT/BIT pass through (native support)
/// - DECIMAL/NUMERIC pass through, RAW → VARBINARY
/// - DATE passes through unchanged (both include time)
fn dm8_type_to_shentong(column: &Column) -> String {
    let raw = column.data_type.trim().to_uppercase();
    let base = if let Some(pos) = raw.find('(') {
        raw[..pos].trim().to_string()
    } else {
        raw.clone()
    };

    // Helper: determine effective character length for string types.
    // DM8 CHAR_USED='B' means length is in bytes; for UTF-8 targets we need to
    // ensure enough room, so multiply by 3 (worst-case UTF-8 expansion).
    // CHAR_USED='C' means length is already in characters — use as-is.
    let effective_char_len = |len: i32| -> i32 {
        let is_byte_semantics = column
            .char_semantics
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("B"))
            .unwrap_or(false);
        if is_byte_semantics {
            // Byte semantics: length is in bytes, but Shentong VARCHAR(n) is also bytes.
            // DM8 may store multi-byte chars, so keep the byte length as-is.
            // However, DM8 page size may allow larger VARCHAR than Shentong (max 8000).
            len
        } else {
            // Character semantics: multiply by 4 to convert to bytes for Shentong
            // (Shentong VARCHAR(n) counts bytes, not characters)
            len.saturating_mul(4)
        }
    };

    match base.as_str() {
        // LOB / large object types — normalize to Oracle-standard BLOB/CLOB
        "BLOB" | "LONGVARBINARY" => "BLOB".to_string(),
        "CLOB" | "NCLOB" | "LONG" | "TEXT" => "CLOB".to_string(),

        // Float / double
        "DOUBLE" | "FLOAT" => "DOUBLE PRECISION".to_string(),
        "REAL" => "REAL".to_string(),

        // Binary types — Shentong maps RAW(n) to VARBINARY(n) internally
        "RAW" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("VARBINARY({})", len)
            } else {
                "BLOB".to_string()
            }
        }
        "BINARY" | "VARBINARY" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("VARBINARY({})", len)
            } else {
                "BLOB".to_string()
            }
        }

        // TINYINT — Shentong natively supports TINYINT (INT1, range -128..127)
        "TINYINT" => "TINYINT".to_string(),

        // BIT — DM8 uses BIT as boolean (stores 0/1), map to Shentong BOOLEAN
        // (Shentong BIT is a PostgreSQL-style bit string, semantically different)
        "BIT" => "BOOLEAN".to_string(),

        // BIGINT → NUMBER(19) — 8-byte integer, most stable cross-DB mapping
        "BIGINT" => "NUMBER(19)".to_string(),

        // INTEGER types → NUMBER(10) for maximum compatibility
        "INTEGER" | "INT" => "NUMBER(10)".to_string(),

        // SMALLINT → NUMBER(5)
        "SMALLINT" => "NUMBER(5)".to_string(),

        // Oracle NUMBER — direct pass-through with precision/scale
        "NUMBER" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                let scale = column.scale.unwrap_or(0);
                format!("NUMBER({},{})", prec, scale)
            } else {
                "NUMBER".to_string()
            }
        }

        // DECIMAL / NUMERIC — Shentong supports natively (precision up to 1000)
        "DECIMAL" | "NUMERIC" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                let scale = column.scale.unwrap_or(0);
                format!("{}({},{})", base, prec, scale)
            } else if raw.contains('(') {
                raw
            } else {
                base
            }
        }

        // VARCHAR / VARCHAR2 — expand length for character semantics, cap at 8000
        "VARCHAR" | "VARCHAR2" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                let eff = effective_char_len(len);
                if eff > SHENTONG_MAX_VARCHAR_BYTES {
                    "CLOB".to_string()
                } else {
                    format!("VARCHAR({})", eff)
                }
            } else {
                "CLOB".to_string()
            }
        }

        // NVARCHAR / NVARCHAR2 → VARCHAR with expanded length
        // NVARCHAR is always character-semantics, multiply by 4 for UTF-8 bytes
        "NVARCHAR" | "NVARCHAR2" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                let eff = len.saturating_mul(4);
                if eff > SHENTONG_MAX_VARCHAR_BYTES {
                    "CLOB".to_string()
                } else {
                    format!("VARCHAR({})", eff)
                }
            } else {
                "CLOB".to_string()
            }
        }

        // CHAR — expand for character semantics
        "CHAR" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                let eff = effective_char_len(len);
                if eff > SHENTONG_MAX_VARCHAR_BYTES {
                    "CLOB".to_string()
                } else {
                    format!("CHAR({})", eff)
                }
            } else {
                "CHAR(1)".to_string()
            }
        }

        // NCHAR → CHAR with expanded length (always character-semantics)
        "NCHAR" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                let eff = len.saturating_mul(4);
                if eff > SHENTONG_MAX_VARCHAR_BYTES {
                    "CLOB".to_string()
                } else {
                    format!("CHAR({})", eff)
                }
            } else {
                "CHAR(1)".to_string()
            }
        }

        // TIMESTAMP / DATETIME — preserve fractional seconds precision (max 6 in Shentong)
        "TIMESTAMP" | "DATETIME" => {
            if let Some(fsp) = column.scale.filter(|s| *s >= 0 && *s <= 6) {
                if fsp != 6 {
                    return format!("TIMESTAMP({})", fsp);
                }
            }
            "TIMESTAMP".to_string()
        }

        // DATE — both DM8 and Shentong DATE include time component (Oracle-compat)
        "DATE" => "DATE".to_string(),

        // BOOLEAN — Shentong supports BOOLEAN natively
        "BOOLEAN" | "BOOL" => "BOOLEAN".to_string(),

        // Pass through unchanged
        _ => raw,
    }
}

fn indexable_shentong_type(column: &Column, mapped_type: &str) -> String {
    let upper = mapped_type.trim().to_uppercase();
    if upper != "CLOB" && upper != "NCLOB" && upper != "BLOB" {
        return mapped_type.to_string();
    }

    let raw = column.data_type.trim().to_uppercase();
    let base = raw
        .split_once('(')
        .map(|(base, _)| base.trim())
        .unwrap_or(&raw);
    let fallback = match base {
        "BLOB" | "LONGVARBINARY" | "RAW" | "BINARY" | "VARBINARY" => {
            format!("VARBINARY({})", SHENTONG_INDEXABLE_FALLBACK_BYTES)
        }
        _ => format!("VARCHAR({})", SHENTONG_INDEXABLE_FALLBACK_BYTES),
    };

    tracing::warn!(
        "Converting Shentong key/index column '{}' from {} to {} because OSCAR rejects LOB index expressions",
        column.name,
        mapped_type,
        fallback
    );
    fallback
}

fn shentong_indexed_columns(table: &TableDetails) -> HashSet<String> {
    let mut columns = HashSet::new();
    columns.extend(table.primary_keys.iter().map(|name| name.to_uppercase()));
    for unique in &table.unique_constraints {
        columns.extend(unique.columns.iter().map(|name| name.to_uppercase()));
    }
    for index in &table.indexes {
        columns.extend(index.columns.iter().map(|name| name.to_uppercase()));
    }
    columns
}

/// Format a column definition for Shentong DDL output.
fn format_shentong_column_def(column: &Column, force_indexable: bool) -> String {
    let mut parts = Vec::new();
    parts.push(quote_identifier(&column.name));
    let mut st_type = dm8_type_to_shentong(column);
    if force_indexable {
        st_type = indexable_shentong_type(column, &st_type);
    }
    parts.push(st_type.clone());

    if column.identity {
        // Shentong uses AUTO_INCREMENT (MySQL-compatible), not GENERATED AS IDENTITY.
        // IMPORTANT: AUTO_INCREMENT requires the column to have a PRIMARY KEY or UNIQUE
        // constraint. DM8 identity columns are almost always PKs, but if a non-PK identity
        // column is encountered, the generated DDL will fail on Shentong.
        // AUTO_INCREMENT requires an integer type — NUMBER(n) from our mapping won't work,
        // so force an appropriate integer type for identity columns.
        let int_type = match column.precision {
            Some(p) if p <= 4 => "SMALLINT",
            Some(p) if p <= 9 => "INTEGER",
            _ => "BIGINT",
        };
        if st_type.to_uppercase() != int_type {
            tracing::warn!(
                "Converting identity column '{}' from {} to {} for Shentong AUTO_INCREMENT compatibility",
                column.name, st_type, int_type
            );
        }
        parts.pop();
        parts.push(int_type.to_string());
        parts.push("AUTO_INCREMENT".to_string());
    } else if let Some(default) = column
        .default_value
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        let converted = convert_default_for_shentong(default, &st_type);
        parts.push(format!("DEFAULT {}", converted));
    }

    let nullability = if column.nullable { "NULL" } else { "NOT NULL" };
    parts.push(nullability.to_string());

    parts.join(" ")
}

/// Convert a DM8 DEFAULT value expression to Shentong-compatible syntax.
///
/// Key differences:
/// - BOOLEAN columns (from DM8 BIT): `DEFAULT 0` → `DEFAULT FALSE`, `DEFAULT 1` → `DEFAULT TRUE`
/// - `CURRENT_TIMESTAMP()` → `CURRENT_TIMESTAMP` (Shentong rejects parentheses)
/// - `SYSTIMESTAMP` → `CURRENT_TIMESTAMP` (not supported in Shentong)
/// - `CURRENT_DATE()` → `CURRENT_DATE`
/// - `CURRENT_TIME()` → `CURRENT_TIME`
fn convert_default_for_shentong(default: &str, shentong_type: &str) -> String {
    // DM8 BIT → Shentong BOOLEAN: convert integer defaults to boolean literals
    if shentong_type.to_uppercase() == "BOOLEAN" {
        return match default {
            "0" => "FALSE".to_string(),
            "1" => "TRUE".to_string(),
            _ => default.to_string(),
        };
    }

    let upper = default.trim().to_uppercase();

    // SYSTIMESTAMP → CURRENT_TIMESTAMP (Shentong does not support SYSTIMESTAMP)
    if upper == "SYSTIMESTAMP" || upper == "SYSTIMESTAMP()" {
        return "CURRENT_TIMESTAMP".to_string();
    }

    // CURRENT_TIMESTAMP() → CURRENT_TIMESTAMP (strip empty parentheses)
    if upper == "CURRENT_TIMESTAMP()" {
        return "CURRENT_TIMESTAMP".to_string();
    }

    // CURRENT_DATE() → CURRENT_DATE
    if upper == "CURRENT_DATE()" {
        return "CURRENT_DATE".to_string();
    }

    // CURRENT_TIME() → CURRENT_TIME
    if upper == "CURRENT_TIME()" {
        return "CURRENT_TIME".to_string();
    }

    default.to_string()
}

/// Generate a Shentong-compatible CREATE TABLE statement with COMMENTs.
///
/// Returns `(ddl_string, pk_inlined)` — `pk_inlined` is true when the PRIMARY KEY
/// was embedded inside CREATE TABLE (required for AUTO_INCREMENT columns).
fn generate_shentong_create_table(table: &TableDetails) -> (String, bool) {
    let table_ident = quote_qualified_identifier(&table.name);

    let has_auto_increment = table.columns.iter().any(|col| col.identity);
    let indexed_columns = shentong_indexed_columns(table);

    let column_lines = table
        .columns
        .iter()
        .map(|col| {
            let force_indexable = indexed_columns.contains(&col.name.to_uppercase());
            format!("    {}", format_shentong_column_def(col, force_indexable))
        })
        .collect::<Vec<_>>()
        .join(",\n");

    // Shentong requires AUTO_INCREMENT columns to have a PRIMARY KEY or UNIQUE constraint
    // defined in the same CREATE TABLE statement. Inline the PK when AUTO_INCREMENT is present.
    let pk_inlined = has_auto_increment && !table.primary_keys.is_empty();
    let pk_line = if pk_inlined {
        let pk_cols = table
            .primary_keys
            .iter()
            .map(|s| quote_identifier(s))
            .collect::<Vec<_>>()
            .join(", ");
        let base_name = table.name.rsplit('.').next().unwrap_or(&table.name);
        let constraint_name = format!("PK_{}", base_name);
        format!(
            ",\n    CONSTRAINT {} PRIMARY KEY ({})",
            quote_identifier(&constraint_name),
            pk_cols
        )
    } else {
        String::new()
    };

    // If there's an AUTO_INCREMENT column with a custom start value, append table-level clause
    let auto_inc_suffix = table
        .columns
        .iter()
        .find(|col| col.identity)
        .and_then(|col| col.identity_start.filter(|&s| s != 1))
        .map(|start| format!(" AUTO_INCREMENT = {}", start))
        .unwrap_or_default();

    let mut ddl = format!(
        "CREATE TABLE {} (\n{}{}\n){};\n",
        table_ident, column_lines, pk_line, auto_inc_suffix
    );

    if let Some(comment) = table
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        ddl.push_str(&format!(
            "COMMENT ON TABLE {} IS '{}';\n",
            table_ident,
            comment.replace('\'', "''")
        ));
    }

    for column in &table.columns {
        if let Some(comment) = column
            .comment
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            ddl.push_str(&format!(
                "COMMENT ON COLUMN {}.{} IS '{}';\n",
                table_ident,
                quote_identifier(&column.name),
                comment.replace('\'', "''")
            ));
        }
    }

    (ddl.trim_end().to_string(), pk_inlined)
}

/// Generate Shentong-compatible CREATE SEQUENCE statements.
/// Shentong uses: NO CACHE (not NOCACHE), NO CYCLE (not NOCYCLE).
/// ORDER is accepted but ignored; NOORDER does not exist in CREATE SEQUENCE.
fn generate_sequences_for_shentong(sequences: &[Sequence]) -> Vec<String> {
    sequences
        .iter()
        .map(|seq| {
            let mut stmt = format!("CREATE SEQUENCE {}", quote_identifier(&seq.name));
            if let Some(start) = seq.start_with {
                stmt.push_str(&format!(" START WITH {}", start));
            }
            if let Some(min) = seq.min_value {
                stmt.push_str(&format!(" MINVALUE {}", min));
            }
            if let Some(max) = seq.max_value {
                stmt.push_str(&format!(" MAXVALUE {}", max));
            }
            stmt.push_str(&format!(" INCREMENT BY {}", seq.increment_by));
            match seq.cache_size {
                Some(cache) if cache > 0 => stmt.push_str(&format!(" CACHE {}", cache)),
                _ => stmt.push_str(" NO CACHE"),
            }
            if seq.cycle {
                stmt.push_str(" CYCLE");
            } else {
                stmt.push_str(" NO CYCLE");
            }
            // ORDER is accepted but NOT implemented in Shentong (silently ignored).
            // NOORDER does not exist in CREATE SEQUENCE syntax.
            // Only emit ORDER when explicitly requested; otherwise omit entirely.
            if seq.order {
                stmt.push_str(" ORDER");
            }
            stmt.push(';');
            stmt
        })
        .collect()
}

/// Adapt a trigger statement for Shentong compatibility:
/// 1. Remove `REFERENCING OLD AS OLD NEW AS NEW` (unsupported in Shentong)
/// 2. In WHEN clause, use `NEW.col` / `OLD.col` without colon
///    (Shentong WHEN clause follows standard Oracle: no colon; colon is only for BEGIN..END body)
fn adapt_trigger_for_shentong(stmt: &str) -> String {
    let result = stmt.replace(" REFERENCING OLD AS OLD NEW AS NEW", "");
    let mut lines = Vec::new();
    for line in result.lines() {
        let upper = line.trim_start().to_uppercase();
        if upper.starts_with("WHEN ") || upper.starts_with("WHEN(") {
            lines.push(line.replace(":NEW.", "NEW.").replace(":OLD.", "OLD."));
        } else {
            lines.push(line.to_string());
        }
    }
    lines.join("\n")
}

/// Convert Oracle-style sequence references to Shentong function-call syntax.
///
/// DM8/Oracle: `SEQ_NAME.NEXTVAL`, `SEQ_NAME.CURRVAL`
/// Shentong:   `NEXTVAL('SEQ_NAME')`, `CURRVAL('SEQ_NAME')`
///
/// Handles both unquoted (`SEQ.NEXTVAL`) and quoted (`"SEQ".NEXTVAL`) identifiers.
fn convert_sequence_refs_to_shentong(sql: &str) -> String {
    let upper = sql.to_uppercase();
    let bytes = upper.as_bytes();
    let mut result = String::with_capacity(sql.len());
    let mut last = 0; // byte offset of unconsumed input

    // Scan for `.NEXTVAL` and `.CURRVAL` patterns
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] == b'.' && pos > 0 && pos + 7 <= bytes.len() {
            let suffix = &upper[pos + 1..];
            let (func, func_len) = if suffix.starts_with("NEXTVAL") {
                ("NEXTVAL", 7)
            } else if suffix.starts_with("CURRVAL") {
                ("CURRVAL", 7)
            } else {
                pos += 1;
                continue;
            };

            // Check word boundary after the function name
            let after_pos = pos + 1 + func_len;
            if after_pos < bytes.len() {
                let ch = bytes[after_pos];
                if ch.is_ascii_alphanumeric() || ch == b'_' {
                    pos += 1;
                    continue;
                }
            }

            // Extract the identifier before the dot
            let (ident_start, bare_name) = extract_ident_before_dot(sql, pos);
            if !bare_name.is_empty() {
                // Copy everything from last up to ident_start
                result.push_str(&sql[last..ident_start]);
                // Emit Shentong function call
                result.push_str(&format!("{}('{}')", func, bare_name));
                last = after_pos;
                pos = after_pos;
                continue;
            }
        }
        pos += 1;
    }
    result.push_str(&sql[last..]);
    result
}

/// Extract the identifier immediately before `dot_pos` in `sql`.
/// Returns `(byte_start_of_identifier, bare_name_without_quotes)`.
/// Returns `(dot_pos, "")` if no valid identifier is found.
fn extract_ident_before_dot(sql: &str, dot_pos: usize) -> (usize, String) {
    let bytes = sql.as_bytes();
    if dot_pos == 0 {
        return (dot_pos, String::new());
    }
    let mut j = dot_pos - 1;

    // Skip whitespace
    while j > 0 && bytes[j].is_ascii_whitespace() {
        j -= 1;
    }

    if bytes[j] == b'"' {
        // Quoted identifier: find the opening quote
        if j == 0 {
            return (dot_pos, String::new());
        }
        let close = j;
        j -= 1;
        while j > 0 && bytes[j] != b'"' {
            j -= 1;
        }
        if bytes[j] == b'"' {
            let ident = &sql[j + 1..close]; // content between quotes
            return (j, ident.to_string());
        }
        (dot_pos, String::new())
    } else if bytes[j].is_ascii_alphanumeric()
        || bytes[j] == b'_'
        || bytes[j] == b'$'
        || bytes[j] == b'#'
    {
        // Unquoted identifier
        let end = j + 1; // exclusive
        while j > 0
            && (bytes[j - 1].is_ascii_alphanumeric()
                || bytes[j - 1] == b'_'
                || bytes[j - 1] == b'$'
                || bytes[j - 1] == b'#')
        {
            j -= 1;
        }
        let ch = bytes[j];
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let ident = &sql[j..end];
            return (j, ident.to_string());
        }
        (dot_pos, String::new())
    } else {
        (dot_pos, String::new())
    }
}

/// Strip the `CREATE [OR REPLACE] TRIGGER ... ON table ... FOR EACH ROW` header
/// from a trigger body so that `generate_triggers()` reconstructs it with proper
/// schema-qualified table names.
///
/// Returns everything starting from `WHEN`, `BEGIN`, or `DECLARE` — whichever
/// comes first. If none found, returns the original body unchanged.
fn strip_trigger_create_header(body: &str) -> String {
    let upper = body.to_uppercase();
    if !upper.trim_start().starts_with("CREATE TRIGGER")
        && !upper.trim_start().starts_with("CREATE OR REPLACE TRIGGER")
    {
        return body.to_string();
    }

    // Find the earliest occurrence of WHEN/BEGIN/DECLARE at a line boundary
    let mut best: Option<usize> = None;
    for keyword in &["WHEN", "BEGIN", "DECLARE"] {
        // Search for keyword at start of a line (after newline + optional whitespace)
        let mut search_from = 0;
        while let Some(pos) = upper[search_from..].find(keyword) {
            let abs_pos = search_from + pos;
            // Check it's at a word boundary (start of line or after whitespace/newline)
            let at_boundary = abs_pos == 0
                || body.as_bytes()[abs_pos - 1] == b'\n'
                || body.as_bytes()[abs_pos - 1] == b'\r'
                || body.as_bytes()[abs_pos - 1] == b' '
                || body.as_bytes()[abs_pos - 1] == b'\t';
            // Check word boundary after keyword
            let end_pos = abs_pos + keyword.len();
            let ends_at_boundary = end_pos >= body.len()
                || !body.as_bytes()[end_pos].is_ascii_alphanumeric()
                    && body.as_bytes()[end_pos] != b'_';
            if at_boundary && ends_at_boundary {
                best = Some(match best {
                    Some(prev) => prev.min(abs_pos),
                    None => abs_pos,
                });
                break;
            }
            search_from = abs_pos + keyword.len();
        }
    }

    match best {
        Some(pos) => body[pos..].to_string(),
        None => body.to_string(),
    }
}

/// Export DM8 schema DDL to Shentong-compatible SQL.
pub fn export_dm8_to_shentong_ddl(
    connection: &Connection<'_>,
    plan: &LegacyExportPlan,
    identifier_case: &str,
) -> Result<()> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let source_schema = plan.source_schema.to_uppercase();
    let target_schema = plan.target_schema.to_uppercase();

    // 批量查询所有表元数据
    let table_names: Vec<String> = plan.tables.iter().map(|t| t.name.clone()).collect();
    let mut table_cache: Vec<TableDetails> =
        get_tables_details_batch(connection, &source_schema, &table_names)
            .context("Failed to batch-fetch DM8 table metadata")?;

    // Filter FK references to only the selected tables
    let selected_tables: HashSet<String> = table_cache
        .iter()
        .map(|t| t.name.rsplit('.').next().unwrap_or(&t.name).to_uppercase())
        .collect();
    for table in &mut table_cache {
        table.foreign_keys.retain(|fk| {
            let ref_name = fk
                .referenced_table
                .rsplit('.')
                .next()
                .unwrap_or(&fk.referenced_table)
                .to_uppercase();
            selected_tables.contains(&ref_name)
        });
    }

    // Fetch sequences and filter to those used by the selected tables
    let all_sequences = fetch_sequences(connection, &source_schema).unwrap_or_default();
    let mut sequences = filter_sequences_for_tables(&all_sequences, &table_cache);

    // Apply identifier case transformation
    for table in &mut table_cache {
        apply_identifier_case(table, identifier_case);
    }
    for seq in &mut sequences {
        apply_sequence_case(seq, identifier_case);
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);

    // File header
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let table_names: Vec<String> = table_cache.iter().map(|t| t.name.clone()).collect();
    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- DM8 -> Shentong (OSCAR) DDL 导出脚本")?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- 生成时间: {}", timestamp)?;
    writeln!(writer, "-- 源模式: {}", source_schema)?;
    writeln!(writer, "-- 目标模式: {}", target_schema)?;
    writeln!(writer, "-- 表数量: {}", plan.tables.len())?;
    writeln!(writer, "-- 涉及的表: {}", table_names.join(", "))?;
    writeln!(writer, "--")?;
    writeln!(
        writer,
        "-- 注意: DM8 与 Shentong 均为 Oracle 兼容数据库，大部分语法可直接使用"
    )?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer)?;

    // Sequences MUST be created before tables (columns may reference DEFAULT NEXTVAL('seq'))
    let seq_stmts = generate_sequences_for_shentong(&sequences);
    if !seq_stmts.is_empty() {
        writeln!(writer, "-- 序列（必须在建表之前执行）")?;
        for seq in &sequences {
            writeln!(
                writer,
                "DROP SEQUENCE IF EXISTS {};",
                quote_identifier(&seq.name)
            )?;
        }
        for stmt in &seq_stmts {
            writeln!(writer, "{}", stmt)?;
        }
        writeln!(writer)?;
    }

    // Per-table DDL: CREATE TABLE + comments + PK + unique + check + indexes
    for (i, table_details) in table_cache.iter().enumerate() {
        let mut render_table = table_details.clone();
        render_table.name = table_details.name.clone();
        // Strip schema prefix from FK referenced tables
        for fk in &mut render_table.foreign_keys {
            if let Some((_schema, bare)) = fk.referenced_table.split_once('.') {
                fk.referenced_table = bare.to_string();
            }
        }

        if i > 0 {
            writeln!(writer)?;
        }

        let table_ident = quote_qualified_identifier(&render_table.name);
        writeln!(writer, "-- 表: {}", table_ident)?;

        // 显式删除独立索引（神通 DROP TABLE CASCADE 可能不会自动清理）
        // 注意：PK/UNIQUE 约束的隐式索引不能用 DROP INDEX 删除，由 DROP TABLE CASCADE 处理
        for idx in &render_table.indexes {
            writeln!(
                writer,
                "DROP INDEX IF EXISTS {};",
                quote_identifier(&idx.name)
            )?;
        }
        writeln!(writer, "DROP TABLE IF EXISTS {} CASCADE;", table_ident)?;

        let (create_ddl, pk_inlined) = generate_shentong_create_table(&render_table);
        writeln!(writer, "{}", create_ddl)?;

        // Skip separate ALTER TABLE ADD PK when it was already inlined (AUTO_INCREMENT requires it)
        if !pk_inlined {
            if let Some(pk_stmt) = generate_primary_key(&render_table) {
                writeln!(writer)?;
                writeln!(writer, "{}", pk_stmt)?;
            }
        }

        let unique_stmts = generate_unique_constraints(&render_table);
        if !unique_stmts.is_empty() {
            writeln!(writer)?;
            for stmt in unique_stmts {
                writeln!(writer, "{}", stmt)?;
            }
        }

        let check_stmts = generate_check_constraints(&render_table);
        if !check_stmts.is_empty() {
            writeln!(writer)?;
            for stmt in check_stmts {
                writeln!(writer, "{}", stmt)?;
            }
        }

        let index_stmts = generate_indexes(&render_table);
        if !index_stmts.is_empty() {
            writeln!(writer)?;
            for stmt in index_stmts {
                writeln!(writer, "{}", stmt)?;
            }
        }
    }

    // Foreign keys — emit after all tables to reduce dependency issues
    let mut fk_stmts: Vec<String> = Vec::new();
    for table_details in &table_cache {
        let mut render_table = table_details.clone();
        render_table.name = table_details.name.clone();
        for fk in &mut render_table.foreign_keys {
            if let Some((_schema, bare)) = fk.referenced_table.split_once('.') {
                fk.referenced_table = bare.to_string();
            }
        }
        fk_stmts.extend(generate_foreign_keys(&render_table));
    }
    if !fk_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- 外键")?;
        for stmt in fk_stmts {
            writeln!(writer, "{}", stmt)?;
        }
    }

    // Triggers (sequences already emitted before tables)
    let mut trig_stmts: Vec<String> = Vec::new();
    for table_details in &table_cache {
        // Strip CREATE header from trigger bodies so generate_triggers()
        // reconstructs the header with proper schema-qualified table names.
        let mut triggers = table_details.triggers.clone();
        for tr in &mut triggers {
            tr.body = strip_trigger_create_header(&tr.body);
        }
        let raw_stmts = generate_triggers(&target_schema, &triggers, TriggerTerminator::Plain);
        // Adapt triggers for Shentong: remove REFERENCING clause, fix WHEN clause
        // then convert Oracle-style SEQ.NEXTVAL → Shentong NEXTVAL('SEQ')
        for stmt in raw_stmts {
            let adapted = adapt_trigger_for_shentong(&stmt);
            trig_stmts.push(convert_sequence_refs_to_shentong(&adapted));
        }
    }

    if !trig_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- ============================================")?;
        writeln!(writer, "-- 触发器（Oracle 兼容语法）")?;
        writeln!(writer, "-- ============================================")?;
        for stmt in trig_stmts {
            writeln!(writer, "{}", stmt)?;
            writeln!(writer)?;
        }
    }

    writer
        .flush()
        .context("Failed to flush dm8->shentong ddl export")?;
    Ok(())
}

/// Export DM8 data to Shentong-compatible INSERT statements.
pub fn export_dm8_to_shentong_data(
    connection: &Connection<'_>,
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
    identifier_case: &str,
) -> Result<usize> {
    use crate::export::pipeline::DataExportPipeline;

    // Batch-fetch all table metadata
    let batch_table_names: Vec<String> = plan.tables.iter().map(|t| t.name.clone()).collect();
    let all_details = get_tables_details_batch(connection, &plan.source_schema, &batch_table_names)
        .context("Failed to batch-fetch DM8 table metadata for data export")?;

    let renderer = renderer_for(&DbType::Shentong);
    let pipeline = DataExportPipeline {
        source_schema: plan.source_schema.clone(),
        target_schema: plan.target_schema.clone(),
        batch_size,
        identifier_case: identifier_case.to_string(),
        max_parallelism: 4,
        skip_fk_toggle: true, // Cross-DB: DM8 FK constraint names don't exist in ShenTong
        truncate_cascade: true, // ShenTong (PG-compat) requires CASCADE for FK-referenced tables
        skip_trigger_toggle: false,
        use_foreign_key_checks: false,
        unicode_safe_text: true,
    };

    pipeline.execute(
        connection,
        config,
        renderer.as_ref(),
        &all_details,
        &plan.output_path,
    )
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_qualified_identifier(name: &str) -> String {
    name.split('.')
        .map(quote_identifier)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::{
        adapt_trigger_for_shentong, convert_sequence_refs_to_shentong,
        generate_shentong_create_table,
    };
    use crate::models::{
        CheckConstraint, Column, ForeignKey, Index, TableDetails, TriggerDefinition,
        UniqueConstraint,
    };

    fn make_column(name: &str, data_type: &str, length: Option<i32>, nullable: bool) -> Column {
        Column {
            name: name.to_string(),
            data_type: data_type.to_string(),
            length,
            precision: None,
            scale: None,
            char_semantics: None,
            nullable,
            comment: None,
            default_value: None,
            identity: false,
            identity_start: None,
            identity_increment: None,
        }
    }

    fn table_with_columns(name: &str, columns: Vec<Column>) -> TableDetails {
        TableDetails {
            name: name.to_string(),
            comment: None,
            columns,
            primary_keys: Vec::new(),
            indexes: Vec::new(),
            unique_constraints: Vec::<UniqueConstraint>::new(),
            foreign_keys: Vec::<ForeignKey>::new(),
            check_constraints: Vec::<CheckConstraint>::new(),
            triggers: Vec::<TriggerDefinition>::new(),
        }
    }

    #[test]
    fn create_table_keeps_unbounded_non_key_varchar_as_clob() {
        let table =
            table_with_columns("docs", vec![make_column("payload", "VARCHAR2", None, true)]);

        let (ddl, _pk_inlined) = generate_shentong_create_table(&table);

        assert!(
            ddl.contains("\"payload\" CLOB NULL"),
            "non-key unbounded text should remain CLOB, got: {}",
            ddl
        );
    }

    #[test]
    fn create_table_forces_unbounded_pk_varchar_to_indexable_type() {
        let mut table = table_with_columns(
            "excel_template",
            vec![make_column("id", "VARCHAR2", None, false)],
        );
        table.primary_keys = vec!["id".to_string()];

        let (ddl, _pk_inlined) = generate_shentong_create_table(&table);

        assert!(
            ddl.contains("\"id\" VARCHAR(255) NOT NULL"),
            "PK column must not render as CLOB, got: {}",
            ddl
        );
        assert!(
            !ddl.contains("\"id\" CLOB NOT NULL"),
            "PK column should avoid Shentong LOB index errors"
        );
    }

    #[test]
    fn create_table_forces_lob_index_column_to_indexable_type() {
        let mut table =
            table_with_columns("docs", vec![make_column("external_id", "CLOB", None, true)]);
        table.indexes = vec![Index {
            name: "idx_docs_external_id".to_string(),
            columns: vec!["external_id".to_string()],
            unique: false,
        }];

        let (ddl, _pk_inlined) = generate_shentong_create_table(&table);

        assert!(
            ddl.contains("\"external_id\" VARCHAR(255) NULL"),
            "indexed LOB column must use an indexable type, got: {}",
            ddl
        );
    }

    #[test]
    fn converts_unquoted_nextval() {
        let input = "SELECT SEQ_USER_ID.NEXTVAL INTO :NEW.ID FROM DUAL";
        let result = convert_sequence_refs_to_shentong(input);
        assert_eq!(
            result,
            "SELECT NEXTVAL('SEQ_USER_ID') INTO :NEW.ID FROM DUAL"
        );
    }

    #[test]
    fn converts_unquoted_currval() {
        let input = "SEQ_ORDER.CURRVAL";
        let result = convert_sequence_refs_to_shentong(input);
        assert_eq!(result, "CURRVAL('SEQ_ORDER')");
    }

    #[test]
    fn converts_quoted_nextval() {
        let input = r#""MY_SEQ".NEXTVAL"#;
        let result = convert_sequence_refs_to_shentong(input);
        assert_eq!(result, "NEXTVAL('MY_SEQ')");
    }

    #[test]
    fn does_not_convert_table_column_refs() {
        let input = "SELECT T.NAME FROM T";
        let result = convert_sequence_refs_to_shentong(input);
        assert_eq!(result, input, "table.column should not be converted");
    }

    #[test]
    fn converts_multiple_occurrences() {
        let input = "SEQ_A.NEXTVAL + SEQ_B.CURRVAL";
        let result = convert_sequence_refs_to_shentong(input);
        assert_eq!(result, "NEXTVAL('SEQ_A') + CURRVAL('SEQ_B')");
    }

    #[test]
    fn case_insensitive_nextval() {
        let input = "seq_foo.NextVal";
        let result = convert_sequence_refs_to_shentong(input);
        assert_eq!(result, "NEXTVAL('seq_foo')");
    }

    #[test]
    fn adapt_trigger_removes_referencing_clause() {
        let input = "CREATE OR REPLACE TRIGGER \"T\"\n\
                      BEFORE INSERT ON \"TBL\" REFERENCING OLD AS OLD NEW AS NEW\n\
                      FOR EACH ROW\n\
                      BEGIN\n  :NEW.ID := 1;\nEND;";
        let result = adapt_trigger_for_shentong(input);
        assert!(
            !result.contains("REFERENCING"),
            "REFERENCING clause should be removed"
        );
        assert!(result.contains(":NEW.ID"), "body :NEW should be preserved");
    }

    #[test]
    fn adapt_trigger_fixes_when_clause_colon() {
        let input = "CREATE OR REPLACE TRIGGER \"T\"\n\
                      BEFORE INSERT ON \"TBL\"\n\
                      FOR EACH ROW\n\
                      WHEN (:NEW.ID IS NULL)\n\
                      BEGIN\n  :NEW.ID := 1;\nEND;";
        let result = adapt_trigger_for_shentong(input);
        assert!(
            result.contains("WHEN (NEW.ID IS NULL)"),
            "WHEN should use NEW without colon"
        );
        assert!(
            result.contains(":NEW.ID := 1"),
            "body :NEW should be preserved"
        );
    }
}
