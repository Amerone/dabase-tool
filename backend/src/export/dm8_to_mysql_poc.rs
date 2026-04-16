use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::Connection;

use crate::{
    db::schema,
    dialect::renderer_for,
    export::orchestrator::LegacyExportPlan,
    models::{Column, ConnectionConfig, DbType, TableDetails},
};

use super::common::apply_identifier_case;

/// Map a DM8 column type to its MySQL equivalent, preserving precision/length.
fn dm8_type_to_mysql(column: &Column) -> String {
    let raw = column.data_type.trim().to_uppercase();
    let base = if let Some(pos) = raw.find('(') {
        raw[..pos].trim().to_string()
    } else {
        raw.clone()
    };

    match base.as_str() {
        // LOB / large object types
        "BLOB" | "LONGVARBINARY" => "LONGBLOB".to_string(),
        "CLOB" | "NCLOB" | "LONG" | "TEXT" => "LONGTEXT".to_string(),

        // Float / double
        "DOUBLE" | "FLOAT" => "DOUBLE".to_string(),
        "REAL" => "FLOAT".to_string(),

        // Binary types
        "RAW" | "BINARY" | "VARBINARY" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("VARBINARY({})", len)
            } else {
                "LONGBLOB".to_string()
            }
        }

        // TINYINT
        "TINYINT" => "TINYINT".to_string(),

        // BIT → TINYINT(1) (MySQL boolean convention)
        "BIT" => "TINYINT(1)".to_string(),

        // BIGINT
        "BIGINT" => "BIGINT".to_string(),

        // INTEGER types
        "INTEGER" | "INT" => "INT".to_string(),

        // SMALLINT
        "SMALLINT" => "SMALLINT".to_string(),

        // Oracle NUMBER → DECIMAL
        "NUMBER" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                let scale = column.scale.unwrap_or(0);
                if scale == 0 {
                    // Integer-like NUMBER: pick smallest MySQL int type
                    match prec {
                        1..=2 => "TINYINT".to_string(),
                        3..=4 => "SMALLINT".to_string(),
                        5..=6 => "MEDIUMINT".to_string(),
                        7..=9 => "INT".to_string(),
                        10..=18 => "BIGINT".to_string(),
                        _ => format!("DECIMAL({},{})", prec, scale),
                    }
                } else {
                    format!("DECIMAL({},{})", prec, scale)
                }
            } else {
                "DECIMAL(38,10)".to_string()
            }
        }

        // DECIMAL / NUMERIC
        "DECIMAL" | "NUMERIC" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                let scale = column.scale.unwrap_or(0);
                format!("DECIMAL({},{})", prec, scale)
            } else if raw.contains('(') {
                raw.replacen("NUMERIC", "DECIMAL", 1)
            } else {
                "DECIMAL(38,10)".to_string()
            }
        }

        // VARCHAR / VARCHAR2
        "VARCHAR" | "VARCHAR2" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                if len > 16383 {
                    "LONGTEXT".to_string()
                } else {
                    format!("VARCHAR({})", len)
                }
            } else {
                "LONGTEXT".to_string()
            }
        }

        // NVARCHAR / NVARCHAR2
        "NVARCHAR" | "NVARCHAR2" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                if len > 16383 {
                    "LONGTEXT".to_string()
                } else {
                    format!("VARCHAR({})", len)
                }
            } else {
                "LONGTEXT".to_string()
            }
        }

        // CHAR
        "CHAR" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                if len > 255 {
                    format!("VARCHAR({})", len)
                } else {
                    format!("CHAR({})", len)
                }
            } else {
                "CHAR(1)".to_string()
            }
        }

        // NCHAR
        "NCHAR" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                if len > 255 {
                    format!("VARCHAR({})", len)
                } else {
                    format!("CHAR({})", len)
                }
            } else {
                "CHAR(1)".to_string()
            }
        }

        // TIMESTAMP / DATETIME — preserve fractional seconds (MySQL max 6)
        "TIMESTAMP" | "DATETIME" => {
            if let Some(fsp) = column.scale.filter(|s| *s >= 0 && *s <= 6) {
                if fsp != 0 {
                    return format!("DATETIME({})", fsp);
                }
            }
            "DATETIME".to_string()
        }

        // DATE — DM8 DATE includes time, MySQL DATE is date-only → use DATETIME
        "DATE" => "DATETIME".to_string(),

        // BOOLEAN
        "BOOLEAN" | "BOOL" => "TINYINT(1)".to_string(),

        // Pass through unchanged
        _ => raw,
    }
}

/// Format a column definition for MySQL DDL output.
/// `type_override` allows replacing the mapped type (used for row-size overflow → TEXT demotion).
fn format_mysql_column_def(column: &Column, type_override: Option<&str>) -> String {
    let mut parts = Vec::new();
    parts.push(mysql_quote(&column.name));
    let my_type = type_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| dm8_type_to_mysql(column));
    parts.push(my_type.clone());

    // MySQL does not allow DEFAULT values on TEXT, BLOB, JSON, or GEOMETRY columns.
    let is_lob_type = {
        let t = my_type.to_uppercase();
        t.contains("TEXT") || t.contains("BLOB") || t == "JSON" || t == "GEOMETRY"
    };

    if column.identity {
        // MySQL uses AUTO_INCREMENT for identity columns
        parts.push("AUTO_INCREMENT".to_string());
    } else if !is_lob_type {
        if let Some(default) = column
            .default_value
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            let mysql_default = convert_default_for_mysql(default, &my_type);
            parts.push(format!("DEFAULT {}", mysql_default));
        }
    }

    if !column.nullable {
        parts.push("NOT NULL".to_string());
    }

    if let Some(comment) = column
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        parts.push(format!("COMMENT '{}'", comment.replace('\'', "''")));
    }

    parts.join(" ")
}

/// Convert DM8 DEFAULT expressions to MySQL equivalents.
/// `mysql_type` is the resolved MySQL column type (e.g. "DATETIME(6)") — used to
/// match CURRENT_TIMESTAMP precision with DATETIME fractional seconds.
fn convert_default_for_mysql(default: &str, mysql_type: &str) -> String {
    let trimmed = default.trim();
    let upper = trimmed.to_uppercase();
    let mysql_upper = mysql_type.trim().to_uppercase();

    // Extract fractional seconds precision from DATETIME(n)
    let fsp = extract_datetime_fsp(mysql_type);

    // DM8 timestamp/date functions → MySQL CURRENT_TIMESTAMP
    let is_ts_func = matches!(
        upper.as_str(),
        "SYSDATE"
            | "SYSDATE()"
            | "SYSTIMESTAMP"
            | "SYSTIMESTAMP()"
            | "CURRENT_TIMESTAMP"
            | "CURRENT_TIMESTAMP()"
            | "NOW()"
            | "GETDATE()"
            | "LOCALTIMESTAMP"
            | "LOCALTIMESTAMP()"
            | "CURRENT_DATE"
            | "CURRENT_DATE()"
    ) || upper.starts_with("SYSTIMESTAMP(")
        || upper.starts_with("CURRENT_TIMESTAMP(");

    if is_ts_func {
        // For DATE-only MySQL columns, use CURRENT_TIMESTAMP (MySQL accepts it for DATETIME)
        return if fsp > 0 {
            format!("CURRENT_TIMESTAMP({})", fsp)
        } else {
            "CURRENT_TIMESTAMP".to_string()
        };
    }

    // DM8 CURRENT_TIME variants
    if upper == "CURRENT_TIME" || upper == "CURRENT_TIME()" {
        return "CURRENT_TIME".to_string();
    }

    // For DATETIME/DATE/TIMESTAMP columns: if the default value is not a recognized
    // MySQL expression (string literal, number, NULL, CURRENT_*), drop it to avoid
    // "Invalid default value" errors from DM8-specific functions leaking through.
    if mysql_upper.starts_with("DATETIME")
        || mysql_upper == "DATE"
        || mysql_upper.starts_with("TIMESTAMP")
    {
        // Allow quoted string literals (e.g. '2020-01-01 00:00:00')
        if trimmed.starts_with('\'') && trimmed.ends_with('\'') {
            return trimmed.to_string();
        }
        // Allow NULL
        if upper == "NULL" {
            return "NULL".to_string();
        }
        // Anything else for date/time columns is likely DM8-specific — drop it
        tracing::debug!(
            "Dropping unrecognized DM8 default '{}' for MySQL {} column",
            trimmed,
            mysql_type
        );
        return "NULL".to_string();
    }

    // Pass through other defaults (numbers, strings, etc.)
    trimmed.to_string()
}

/// Extract fractional seconds precision from a MySQL type like "DATETIME(6)" → 6.
/// Returns 0 if no precision specified or not a DATETIME/TIMESTAMP type.
fn extract_datetime_fsp(mysql_type: &str) -> i32 {
    let upper = mysql_type.trim().to_uppercase();
    for prefix in &["DATETIME(", "TIMESTAMP("] {
        if upper.starts_with(prefix) {
            if let Some(n) = upper
                .trim_start_matches(prefix)
                .trim_end_matches(')')
                .parse::<i32>()
                .ok()
            {
                return n;
            }
        }
    }
    0
}

/// MySQL InnoDB row size limit (excluding BLOBs/TEXT).
const MYSQL_MAX_ROW_SIZE: i64 = 65535;

/// Estimate the index key byte cost of a MySQL column type.
/// Used for PK length validation (InnoDB max key = 3072 bytes with utf8mb4).
fn mysql_type_index_bytes(mysql_type: &str) -> i64 {
    let upper = mysql_type.to_uppercase();
    if upper.starts_with("VARCHAR(") {
        if let Some(n) = upper
            .trim_start_matches("VARCHAR(")
            .trim_end_matches(')')
            .parse::<i64>()
            .ok()
        {
            return n * 4 + 2; // utf8mb4: n*4 bytes + 2 length prefix
        }
    }
    if upper.starts_with("CHAR(") {
        if let Some(n) = upper
            .trim_start_matches("CHAR(")
            .trim_end_matches(')')
            .parse::<i64>()
            .ok()
        {
            return n * 4;
        }
    }
    // Fixed-size types in index
    match upper.as_str() {
        "TINYINT" | "TINYINT(1)" => 1,
        "SMALLINT" => 2,
        "MEDIUMINT" => 3,
        "INT" => 4,
        "BIGINT" => 8,
        "FLOAT" => 4,
        "DOUBLE" => 8,
        "DATE" => 3,
        "DATETIME" => 8,
        "TIMESTAMP" => 4,
        _ => {
            if upper.starts_with("DATETIME(") || upper.starts_with("DECIMAL(") {
                8
            } else {
                20 // conservative fallback
            }
        }
    }
}

/// Estimate the byte cost of a MySQL column type for row-size calculation.
/// TEXT/BLOB types only cost 9-12 bytes (pointer+length) in the row, not their full size.
fn mysql_type_row_bytes(mysql_type: &str) -> i64 {
    let upper = mysql_type.to_uppercase();
    // TEXT/BLOB family: only pointer overhead in row
    if upper.contains("TEXT") || upper.contains("BLOB") {
        return 12;
    }
    // VARCHAR(n) with utf8mb4: n*4 + 2 bytes length prefix
    if upper.starts_with("VARCHAR(") {
        if let Some(n) = upper
            .trim_start_matches("VARCHAR(")
            .trim_end_matches(')')
            .parse::<i64>()
            .ok()
        {
            return n * 4 + 2;
        }
    }
    // CHAR(n) with utf8mb4: n*4
    if upper.starts_with("CHAR(") {
        if let Some(n) = upper
            .trim_start_matches("CHAR(")
            .trim_end_matches(')')
            .parse::<i64>()
            .ok()
        {
            return n * 4;
        }
    }
    // VARBINARY(n): n + 2
    if upper.starts_with("VARBINARY(") {
        if let Some(n) = upper
            .trim_start_matches("VARBINARY(")
            .trim_end_matches(')')
            .parse::<i64>()
            .ok()
        {
            return n + 2;
        }
    }
    // Fixed-size types
    match upper.as_str() {
        "TINYINT" | "TINYINT(1)" => 1,
        "SMALLINT" => 2,
        "MEDIUMINT" => 3,
        "INT" => 4,
        "BIGINT" => 8,
        "FLOAT" => 4,
        "DOUBLE" => 8,
        "DATE" => 3,
        "DATETIME" => 8,
        "TIMESTAMP" => 4,
        _ => {
            // DECIMAL(p,s): roughly ceil((p-s)/9)*4 + ceil(s/9)*4, estimate conservatively
            if upper.starts_with("DECIMAL(") {
                20
            } else if upper.starts_with("DATETIME(") {
                8
            } else {
                20 // conservative fallback
            }
        }
    }
}

/// Generate a MySQL-compatible CREATE TABLE statement with comments.
/// Automatically demotes VARCHAR columns to TEXT when total row size exceeds 65535 bytes.
fn generate_mysql_create_table(table: &TableDetails) -> String {
    let table_ident = mysql_quote(&table.name);

    // Phase 1: Map all column types and calculate row size
    let mut col_types: Vec<String> = table
        .columns
        .iter()
        .map(|col| dm8_type_to_mysql(col))
        .collect();

    // Phase 2: Check row size, demote largest VARCHAR→TEXT until under limit.
    // Columns used in PK, indexes, unique constraints, or FK cannot be demoted to TEXT
    // because MySQL requires a prefix length for TEXT/BLOB in key specifications.
    let key_columns: std::collections::HashSet<String> = {
        let mut set = std::collections::HashSet::new();
        for pk in &table.primary_keys {
            set.insert(pk.to_uppercase());
        }
        for idx in &table.indexes {
            for col in &idx.columns {
                set.insert(col.to_uppercase());
            }
        }
        for uc in &table.unique_constraints {
            for col in &uc.columns {
                set.insert(col.to_uppercase());
            }
        }
        for fk in &table.foreign_keys {
            for col in &fk.columns {
                set.insert(col.to_uppercase());
            }
        }
        set
    };
    loop {
        let total: i64 = col_types.iter().map(|t| mysql_type_row_bytes(t)).sum();
        if total <= MYSQL_MAX_ROW_SIZE {
            break;
        }
        // Find the VARCHAR column with the largest byte cost, excluding key columns
        let mut worst_idx: Option<usize> = None;
        let mut worst_cost: i64 = 0;
        for (i, t) in col_types.iter().enumerate() {
            let upper = t.to_uppercase();
            if upper.starts_with("VARCHAR(") {
                // Skip columns used in any key specification
                if key_columns.contains(&table.columns[i].name.to_uppercase()) {
                    continue;
                }
                let cost = mysql_type_row_bytes(t);
                if cost > worst_cost {
                    worst_cost = cost;
                    worst_idx = Some(i);
                }
            }
        }
        match worst_idx {
            Some(idx) => {
                tracing::info!(
                    "Table '{}': demoting column '{}' from {} to TEXT (row size {} > {})",
                    table.name,
                    table.columns[idx].name,
                    col_types[idx],
                    total,
                    MYSQL_MAX_ROW_SIZE
                );
                col_types[idx] = "TEXT".to_string();
            }
            None => break, // No more non-key VARCHAR columns to demote
        }
    }

    // Phase 2.1: Protect key columns — if Phase 1 mapped a key column to TEXT/BLOB,
    // force it back to VARCHAR(255). MySQL cannot use TEXT/BLOB in key specifications
    // without a prefix length, and we generate keys without prefix lengths.
    for (i, t) in col_types.iter_mut().enumerate() {
        let upper = t.to_uppercase();
        if (upper.contains("TEXT") || upper.contains("BLOB"))
            && key_columns.contains(&table.columns[i].name.to_uppercase())
        {
            tracing::info!(
                "Table '{}': key column '{}' mapped to {} — forcing to VARCHAR(255) for MySQL key compatibility",
                table.name,
                table.columns[i].name,
                t
            );
            *t = "VARCHAR(255)".to_string();
        }
    }

    // Phase 2.5: Shrink PK VARCHAR columns if composite PK exceeds 3072 bytes.
    // MySQL InnoDB max index key length = 3072 bytes (utf8mb4: VARCHAR(n) = n*4 bytes in index).
    if table.primary_keys.len() > 1 {
        const MYSQL_MAX_KEY_LENGTH: i64 = 3072;
        let pk_set: std::collections::HashSet<&str> =
            table.primary_keys.iter().map(|s| s.as_str()).collect();
        let pk_indices: Vec<usize> = table
            .columns
            .iter()
            .enumerate()
            .filter(|(_, col)| pk_set.contains(col.name.as_str()))
            .map(|(i, _)| i)
            .collect();

        let pk_key_bytes: i64 = pk_indices
            .iter()
            .map(|&i| mysql_type_index_bytes(&col_types[i]))
            .sum();

        if pk_key_bytes > MYSQL_MAX_KEY_LENGTH {
            // Fixed-size PK columns (INT, BIGINT, etc.) can't be shrunk
            let fixed_bytes: i64 = pk_indices
                .iter()
                .filter(|&&i| !col_types[i].to_uppercase().starts_with("VARCHAR("))
                .map(|&i| mysql_type_index_bytes(&col_types[i]))
                .sum();
            let available_for_varchar = (MYSQL_MAX_KEY_LENGTH - fixed_bytes).max(0);

            // Collect VARCHAR PK columns and their current char counts
            let varchar_pk: Vec<(usize, i64)> = pk_indices
                .iter()
                .filter(|&&i| col_types[i].to_uppercase().starts_with("VARCHAR("))
                .map(|&i| {
                    let n = col_types[i]
                        .to_uppercase()
                        .trim_start_matches("VARCHAR(")
                        .trim_end_matches(')')
                        .parse::<i64>()
                        .unwrap_or(255);
                    (i, n)
                })
                .collect();

            if !varchar_pk.is_empty() {
                // Distribute available bytes proportionally among VARCHAR PK columns
                let total_current_chars: i64 = varchar_pk.iter().map(|(_, n)| *n).sum();
                for (i, old_chars) in &varchar_pk {
                    // Each VARCHAR(n) costs n*4+2 in index; solve for n: n = (budget - 2) / 4
                    let proportion = if total_current_chars > 0 {
                        *old_chars as f64 / total_current_chars as f64
                    } else {
                        1.0 / varchar_pk.len() as f64
                    };
                    let col_budget = (available_for_varchar as f64 * proportion) as i64;
                    let new_chars = ((col_budget - 2) / 4).max(1).min(*old_chars);
                    if new_chars < *old_chars {
                        tracing::info!(
                            "Table '{}': shrinking PK column '{}' from VARCHAR({}) to VARCHAR({}) (PK key {} > {})",
                            table.name,
                            table.columns[*i].name,
                            old_chars,
                            new_chars,
                            pk_key_bytes,
                            MYSQL_MAX_KEY_LENGTH
                        );
                        col_types[*i] = format!("VARCHAR({})", new_chars);
                    }
                }
            }
        }
    }

    // Phase 2.6: Shrink FK VARCHAR columns if any FK column group exceeds 3072 bytes.
    // MySQL auto-creates an index on FK columns; that index is subject to the same 3072-byte limit.
    {
        const MYSQL_MAX_KEY_LENGTH: i64 = 3072;
        let col_index: std::collections::HashMap<String, usize> = table
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.to_uppercase(), i))
            .collect();

        for fk in &table.foreign_keys {
            let fk_indices: Vec<usize> = fk
                .columns
                .iter()
                .filter_map(|c| col_index.get(&c.to_uppercase()).copied())
                .collect();

            let fk_key_bytes: i64 = fk_indices
                .iter()
                .map(|&i| mysql_type_index_bytes(&col_types[i]))
                .sum();

            if fk_key_bytes > MYSQL_MAX_KEY_LENGTH {
                let fixed_bytes: i64 = fk_indices
                    .iter()
                    .filter(|&&i| !col_types[i].to_uppercase().starts_with("VARCHAR("))
                    .map(|&i| mysql_type_index_bytes(&col_types[i]))
                    .sum();
                let available_for_varchar = (MYSQL_MAX_KEY_LENGTH - fixed_bytes).max(0);

                let varchar_fk: Vec<(usize, i64)> = fk_indices
                    .iter()
                    .filter(|&&i| col_types[i].to_uppercase().starts_with("VARCHAR("))
                    .map(|&i| {
                        let n = col_types[i]
                            .to_uppercase()
                            .trim_start_matches("VARCHAR(")
                            .trim_end_matches(')')
                            .parse::<i64>()
                            .unwrap_or(255);
                        (i, n)
                    })
                    .collect();

                if !varchar_fk.is_empty() {
                    let total_current_chars: i64 = varchar_fk.iter().map(|(_, n)| *n).sum();
                    for (i, old_chars) in &varchar_fk {
                        let proportion = if total_current_chars > 0 {
                            *old_chars as f64 / total_current_chars as f64
                        } else {
                            1.0 / varchar_fk.len() as f64
                        };
                        let col_budget = (available_for_varchar as f64 * proportion) as i64;
                        let new_chars = ((col_budget - 2) / 4).max(1).min(*old_chars);
                        if new_chars < *old_chars {
                            tracing::info!(
                                "Table '{}': shrinking FK column '{}' from VARCHAR({}) to VARCHAR({}) (FK key {} > {})",
                                table.name,
                                table.columns[*i].name,
                                old_chars,
                                new_chars,
                                fk_key_bytes,
                                MYSQL_MAX_KEY_LENGTH
                            );
                            col_types[*i] = format!("VARCHAR({})", new_chars);
                        }
                    }
                }
            }
        }
    }

    // Phase 3: Generate column definitions with resolved types
    let column_lines: Vec<String> = table
        .columns
        .iter()
        .zip(col_types.iter())
        .map(|(col, resolved_type)| {
            format!("    {}", format_mysql_column_def(col, Some(resolved_type)))
        })
        .collect();

    let mut all_lines: Vec<String> = column_lines;

    if !table.primary_keys.is_empty() {
        let pk_cols = table
            .primary_keys
            .iter()
            .map(|s| mysql_quote(s))
            .collect::<Vec<_>>()
            .join(", ");
        all_lines.push(format!("    PRIMARY KEY ({})", pk_cols));
    }

    let mut ddl = format!(
        "CREATE TABLE {} (\n{}\n)",
        table_ident,
        all_lines.join(",\n")
    );

    // Table-level options
    let mut options = Vec::new();
    options.push("ENGINE=InnoDB".to_string());
    options.push("DEFAULT CHARSET=utf8mb4".to_string());

    if let Some(comment) = table
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        options.push(format!("COMMENT='{}'", comment.replace('\'', "''")));
    }

    ddl.push_str(&format!(" {}", options.join(" ")));
    ddl.push(';');
    ddl
}

fn mysql_quote(name: &str) -> String {
    // Strip schema prefix (e.g. "PLATFORM.TABLE" → "TABLE") — MySQL DDL doesn't use schema-qualified names
    let bare = name.rsplit('.').next().unwrap_or(name);
    format!("`{}`", bare.replace('`', "``"))
}

pub fn export_dm8_to_mysql_ddl(
    connection: &Connection<'_>,
    plan: &LegacyExportPlan,
    identifier_case: &str,
) -> Result<()> {
    if let Some(parent) = plan.output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output parent {}", parent.display()))?;
    }

    let source_schema = plan.source_schema.to_uppercase();

    let table_names: Vec<String> = plan.tables.iter().map(|t| t.name.clone()).collect();
    tracing::info!(
        "dm8_to_mysql_ddl: fetching metadata for {} tables",
        table_names.len()
    );
    let mut all_details =
        schema::get_tables_details_batch(connection, &source_schema, &table_names)
            .context("Failed to batch-fetch DM8 table metadata for MySQL DDL export")?;
    tracing::info!(
        "dm8_to_mysql_ddl: metadata fetched, got {} tables",
        all_details.len()
    );

    // Filter FK references to only the selected tables
    let selected_tables: HashSet<String> = all_details
        .iter()
        .map(|t| t.name.rsplit('.').next().unwrap_or(&t.name).to_uppercase())
        .collect();
    for table in &mut all_details {
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

    // Apply identifier case transformation
    for table in &mut all_details {
        apply_identifier_case(table, identifier_case);
    }

    let file = File::create(&plan.output_path)
        .with_context(|| format!("Failed to create {}", plan.output_path.display()))?;
    let mut writer = BufWriter::new(file);

    // File header
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let table_list: Vec<String> = all_details.iter().map(|t| t.name.clone()).collect();
    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- DM8 -> MySQL DDL 导出脚本")?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- 生成时间: {}", timestamp)?;
    writeln!(writer, "-- 源模式: {}", source_schema)?;
    writeln!(writer, "-- 目标模式: {}", plan.target_schema)?;
    writeln!(writer, "-- 表数量: {}", all_details.len())?;
    writeln!(writer, "-- 涉及的表: {}", table_list.join(", "))?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer)?;

    for (idx, details) in all_details.iter().enumerate() {
        if idx > 0 {
            writeln!(writer)?;
        }

        let table_ident = mysql_quote(&details.name);
        writeln!(writer, "-- 表: {}", table_ident)?;
        writeln!(writer, "DROP TABLE IF EXISTS {};", table_ident)?;
        writeln!(writer, "{}", generate_mysql_create_table(details))?;
    }

    // Foreign keys — emit after all tables
    let mut fk_stmts: Vec<String> = Vec::new();
    for details in &all_details {
        for fk in &details.foreign_keys {
            let cols = fk
                .columns
                .iter()
                .map(|c| mysql_quote(c))
                .collect::<Vec<_>>()
                .join(", ");
            let ref_cols = fk
                .referenced_columns
                .iter()
                .map(|c| mysql_quote(c))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = format!(
                "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
                mysql_quote(&details.name),
                mysql_quote(&fk.name),
                cols,
                mysql_quote(&fk.referenced_table),
                ref_cols,
            );
            if let Some(ref rule) = fk.delete_rule {
                if rule != "NO ACTION" {
                    stmt.push_str(&format!(" ON DELETE {}", rule));
                }
            }
            if let Some(ref rule) = fk.update_rule {
                if rule != "NO ACTION" {
                    stmt.push_str(&format!(" ON UPDATE {}", rule));
                }
            }
            stmt.push(';');
            fk_stmts.push(stmt);
        }
    }
    if !fk_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- 外键")?;
        for stmt in fk_stmts {
            writeln!(writer, "{}", stmt)?;
        }
    }

    writer
        .flush()
        .context("Failed to flush dm8->mysql ddl export")?;
    tracing::info!("dm8_to_mysql_ddl: file written and flushed successfully");
    Ok(())
}

pub fn export_dm8_to_mysql_data(
    connection: &Connection<'_>,
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
    identifier_case: &str,
) -> Result<usize> {
    use crate::export::pipeline::DataExportPipeline;

    // Batch-fetch all table metadata
    let batch_table_names: Vec<String> = plan.tables.iter().map(|t| t.name.clone()).collect();
    let all_details =
        schema::get_tables_details_batch(connection, &plan.source_schema, &batch_table_names)
            .context("Failed to batch-fetch DM8 table metadata for MySQL data export")?;

    let renderer = renderer_for(&DbType::Mysql);
    let pipeline = DataExportPipeline {
        source_schema: plan.source_schema.clone(),
        target_schema: plan.target_schema.clone(),
        batch_size,
        identifier_case: identifier_case.to_string(),
        max_parallelism: 4,
        skip_fk_toggle: true,
        truncate_cascade: false,
        skip_trigger_toggle: true, // MySQL doesn't support ALTER TABLE ... DISABLE ALL TRIGGERS
        use_foreign_key_checks: true, // MySQL needs SET FOREIGN_KEY_CHECKS = 0 for TRUNCATE
        unicode_safe_text: false,
    };

    pipeline.execute(
        connection,
        config,
        renderer.as_ref(),
        &all_details,
        &plan.output_path,
    )
}

#[cfg(test)]
mod tests {
    use super::{dm8_type_to_mysql, generate_mysql_create_table};
    use crate::domain::canonical::{CanonicalValue, LogicalType};
    use crate::export::pipeline::{parse_dm8_value, parse_hex_bytes};
    use crate::models::Column;
    use crate::models::TableDetails;

    #[test]
    fn parse_hex_bytes_supports_prefixed_input() {
        assert_eq!(parse_hex_bytes("0x0AFF"), Some(vec![0x0A, 0xFF]));
        assert_eq!(parse_hex_bytes(""), Some(vec![]));
        assert_eq!(parse_hex_bytes("ABC"), None);
    }

    #[test]
    fn parse_dm8_value_maps_numeric_and_bool() {
        assert_eq!(
            parse_dm8_value(&LogicalType::Integer, Some("42".to_string())),
            CanonicalValue::Integer(42)
        );
        assert_eq!(
            parse_dm8_value(&LogicalType::Boolean, Some("Y".to_string())),
            CanonicalValue::Boolean(true)
        );
        assert_eq!(
            parse_dm8_value(&LogicalType::Boolean, Some("0".to_string())),
            CanonicalValue::Boolean(false)
        );
    }

    fn make_col(
        data_type: &str,
        length: Option<i32>,
        precision: Option<i32>,
        scale: Option<i32>,
    ) -> Column {
        Column {
            name: "test".to_string(),
            data_type: data_type.to_string(),
            length,
            precision,
            scale,
            char_semantics: None,
            nullable: true,
            comment: None,
            default_value: None,
            identity: false,
            identity_start: None,
            identity_increment: None,
        }
    }

    #[test]
    fn dm8_type_mapping_basic() {
        assert_eq!(
            dm8_type_to_mysql(&make_col("BIGINT", None, None, None)),
            "BIGINT"
        );
        assert_eq!(
            dm8_type_to_mysql(&make_col("VARCHAR2", Some(100), None, None)),
            "VARCHAR(100)"
        );
        assert_eq!(
            dm8_type_to_mysql(&make_col("CLOB", None, None, None)),
            "LONGTEXT"
        );
        assert_eq!(
            dm8_type_to_mysql(&make_col("BLOB", None, None, None)),
            "LONGBLOB"
        );
        assert_eq!(
            dm8_type_to_mysql(&make_col("DATE", None, None, None)),
            "DATETIME"
        );
        assert_eq!(
            dm8_type_to_mysql(&make_col("NUMBER", None, Some(10), Some(0))),
            "BIGINT"
        );
        assert_eq!(
            dm8_type_to_mysql(&make_col("NUMBER", None, Some(10), Some(2))),
            "DECIMAL(10,2)"
        );
    }

    #[test]
    fn mysql_create_table_has_comment_and_no_schema() {
        let details = TableDetails {
            name: "users".to_string(),
            comment: Some("用户表".to_string()),
            columns: vec![
                Column {
                    name: "id".to_string(),
                    data_type: "BIGINT".to_string(),
                    length: None,
                    precision: None,
                    scale: None,
                    char_semantics: None,
                    nullable: false,
                    comment: Some("主键".to_string()),
                    default_value: None,
                    identity: true,
                    identity_start: Some(1),
                    identity_increment: Some(1),
                },
                Column {
                    name: "name".to_string(),
                    data_type: "VARCHAR2".to_string(),
                    length: Some(100),
                    precision: None,
                    scale: None,
                    char_semantics: None,
                    nullable: true,
                    comment: Some("用户名".to_string()),
                    default_value: None,
                    identity: false,
                    identity_start: None,
                    identity_increment: None,
                },
            ],
            primary_keys: vec!["id".to_string()],
            indexes: vec![],
            unique_constraints: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
            triggers: vec![],
        };

        let ddl = generate_mysql_create_table(&details);
        assert!(
            ddl.contains("CREATE TABLE `users`"),
            "should use bare name without schema"
        );
        assert!(!ddl.contains("SYSDBA"), "should not contain schema prefix");
        assert!(
            ddl.contains("COMMENT='用户表'"),
            "should have table comment"
        );
        assert!(
            ddl.contains("COMMENT '主键'"),
            "should have column comment for id"
        );
        assert!(
            ddl.contains("COMMENT '用户名'"),
            "should have column comment for name"
        );
        assert!(
            ddl.contains("AUTO_INCREMENT"),
            "identity column should use AUTO_INCREMENT"
        );
        assert!(ddl.contains("PRIMARY KEY (`id`)"), "should have PK");
    }

    #[test]
    fn mysql_create_table_demotes_varchar_to_text_when_row_too_large() {
        // 10 columns × VARCHAR(4000) = 10 × (4000*4+2) = 160020 bytes > 65535
        let columns: Vec<Column> = (0..10)
            .map(|i| Column {
                name: format!("col{}", i),
                data_type: "VARCHAR2".to_string(),
                length: Some(4000),
                precision: None,
                scale: None,
                char_semantics: None,
                nullable: true,
                comment: None,
                default_value: None,
                identity: false,
                identity_start: None,
                identity_increment: None,
            })
            .collect();

        let details = TableDetails {
            name: "wide_table".to_string(),
            comment: None,
            columns,
            primary_keys: vec![],
            indexes: vec![],
            unique_constraints: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
            triggers: vec![],
        };

        let ddl = generate_mysql_create_table(&details);
        // Some columns should have been demoted to TEXT
        // Count lines containing " TEXT" (not "LONGTEXT") as demoted columns
        let text_count = ddl
            .lines()
            .filter(|l| {
                let trimmed = l.trim();
                trimmed.contains(" TEXT")
                    && !trimmed.contains("LONGTEXT")
                    && !trimmed.contains("CHARSET")
            })
            .count();
        let varchar_count = ddl.matches("VARCHAR(4000)").count();
        assert!(
            text_count > 0,
            "should demote some VARCHAR to TEXT, got DDL:\n{}",
            ddl
        );
        assert!(
            varchar_count < 10,
            "not all columns should remain VARCHAR(4000)"
        );
        // Total should still be 10 columns
        assert_eq!(text_count + varchar_count, 10, "total columns should be 10");
    }
}
