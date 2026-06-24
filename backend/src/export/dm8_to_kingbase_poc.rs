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

/// Map a DM8 column type to its KingBase (PostgreSQL-compatible) equivalent.
fn dm8_type_to_kingbase(column: &Column) -> String {
    let raw = column.data_type.trim().to_uppercase();
    let base = if let Some(pos) = raw.find('(') {
        raw[..pos].trim().to_string()
    } else {
        raw.clone()
    };

    match base.as_str() {
        // LOB / large object types
        "BLOB" | "LONGVARBINARY" => "BYTEA".to_string(),
        "CLOB" | "NCLOB" | "LONG" | "TEXT" => "TEXT".to_string(),

        // Float / double
        "DOUBLE" | "FLOAT" => "DOUBLE PRECISION".to_string(),
        "REAL" => "REAL".to_string(),

        // Binary
        "RAW" | "BINARY" | "VARBINARY" => "BYTEA".to_string(),

        // Tiny int (not in PostgreSQL)
        "TINYINT" => "SMALLINT".to_string(),

        // Oracle NUMBER → NUMERIC
        "NUMBER" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                let scale = column.scale.unwrap_or(0);
                format!("NUMERIC({},{})", prec, scale)
            } else {
                "NUMERIC".to_string()
            }
        }

        // DECIMAL / NUMERIC — preserve parenthetical if present but rename to NUMERIC
        "DECIMAL" | "NUMERIC" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                let scale = column.scale.unwrap_or(0);
                format!("NUMERIC({},{})", prec, scale)
            } else if raw.contains('(') {
                raw.replacen("DECIMAL", "NUMERIC", 1)
            } else {
                "NUMERIC".to_string()
            }
        }

        // VARCHAR / VARCHAR2 — strip CHAR/BYTE semantics qualifier
        "VARCHAR" | "VARCHAR2" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("VARCHAR({})", len)
            } else {
                "TEXT".to_string()
            }
        }

        // NVARCHAR / NVARCHAR2 → VARCHAR (KingBase uses UTF-8 natively)
        "NVARCHAR" | "NVARCHAR2" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("VARCHAR({})", len)
            } else {
                "TEXT".to_string()
            }
        }

        // CHAR — strip CHAR/BYTE semantics qualifier
        "CHAR" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("CHAR({})", len)
            } else {
                "CHAR(1)".to_string()
            }
        }

        // NCHAR → CHAR
        "NCHAR" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                format!("CHAR({})", len)
            } else {
                "CHAR(1)".to_string()
            }
        }

        // TIMESTAMP / DATETIME — preserve fractional seconds precision
        "TIMESTAMP" | "DATETIME" => {
            if let Some(fsp) = column.scale.filter(|s| *s >= 0 && *s <= 9) {
                if fsp != 6 {
                    return format!("TIMESTAMP({})", fsp);
                }
            }
            "TIMESTAMP".to_string()
        }

        // BIT → SMALLINT (KingBase BIT type has different semantics)
        "BIT" => "SMALLINT".to_string(),

        // DATE → TIMESTAMP(0): DM8/Oracle DATE includes time (to seconds),
        // but KingBase/PostgreSQL DATE is date-only, causing Java LocalDateTime errors.
        "DATE" => "TIMESTAMP(0)".to_string(),

        // Pass through unchanged: INT, INTEGER, BIGINT, SMALLINT, BOOLEAN, etc.
        _ => raw,
    }
}

fn format_kingbase_column_def(column: &Column) -> String {
    let mut parts = Vec::new();
    parts.push(quote_identifier(&column.name));
    let kb_type = dm8_type_to_kingbase(column);
    parts.push(kb_type.clone());

    if column.identity {
        // KingBase IDENTITY 列只允许 tinyint/smallint/integer/bigint 类型，
        // 当映射后的类型不在允许范围内时（如 NUMERIC），需要强制替换
        let kb_upper = kb_type.to_uppercase();
        let is_identity_compatible = kb_upper == "TINYINT"
            || kb_upper == "SMALLINT"
            || kb_upper == "INTEGER"
            || kb_upper == "INT"
            || kb_upper == "BIGINT";
        if !is_identity_compatible {
            // 根据精度选择最合适的整数类型
            let int_type = match column.precision {
                Some(p) if p <= 4 => "SMALLINT",
                Some(p) if p <= 9 => "INTEGER",
                _ => "BIGINT",
            };
            tracing::warn!(
                "Converting identity column '{}' from {} to {} for KingBase IDENTITY compatibility",
                column.name,
                kb_type,
                int_type
            );
            // 替换已 push 的类型
            parts.pop();
            parts.push(int_type.to_string());
        }
        let start = column.identity_start.unwrap_or(1);
        let inc = column.identity_increment.unwrap_or(1);
        parts.push(format!(
            "GENERATED BY DEFAULT AS IDENTITY (START WITH {} INCREMENT BY {})",
            start, inc
        ));
    } else if let Some(default) = column
        .default_value
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        if let Some(pg_default) = sanitize_kingbase_default(default, &kb_type) {
            parts.push(format!("DEFAULT {}", pg_default));
        }
    }

    let nullability = if column.nullable { "NULL" } else { "NOT NULL" };
    parts.push(nullability.to_string());

    parts.join(" ")
}

fn generate_kingbase_create_table(table: &TableDetails) -> String {
    let table_ident = quote_qualified_identifier(&table.name);

    let column_lines = table
        .columns
        .iter()
        .map(|col| format!("    {}", format_kingbase_column_def(col)))
        .collect::<Vec<_>>()
        .join(",\n");

    let mut ddl = format!("CREATE TABLE {} (\n{}\n);\n", table_ident, column_lines);

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

    ddl.trim_end().to_string()
}

/// Generate KingBase-compatible CREATE SEQUENCE statements.
/// KingBase (PostgreSQL) does not support ORDER/NOORDER; NOCACHE is expressed as CACHE 1.
fn generate_sequences_for_kingbase(sequences: &[Sequence]) -> Vec<String> {
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
                _ => stmt.push_str(" CACHE 1"), // KingBase has no NOCACHE; CACHE 1 disables caching
            }
            if seq.cycle {
                stmt.push_str(" CYCLE");
            } else {
                stmt.push_str(" NO CYCLE");
            }
            // ORDER / NOORDER is Oracle-only — omit for KingBase
            stmt.push(';');
            stmt
        })
        .collect()
}

pub fn export_dm8_to_kingbase_ddl(
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
    writeln!(writer, "-- DM8 -> KingBase DDL 导出脚本")?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- 生成时间: {}", timestamp)?;
    writeln!(writer, "-- 源模式: {}", source_schema)?;
    writeln!(writer, "-- 目标模式: {}", target_schema)?;
    writeln!(writer, "-- 表数量: {}", plan.tables.len())?;
    writeln!(writer, "-- 涉及的表: {}", table_names.join(", "))?;
    writeln!(writer, "--")?;
    writeln!(
        writer,
        "-- 注意: 触发器使用 Oracle 兼容语法，请确保 KingBase 运行在 Oracle 兼容模式下"
    )?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer)?;

    // Per-table DDL: CREATE TABLE + comments + PK + unique + check + indexes
    for (i, table_details) in table_cache.iter().enumerate() {
        let mut render_table = table_details.clone();
        // Use bare table name without schema prefix for KingBase
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

        writeln!(
            writer,
            "-- 表: {}",
            quote_qualified_identifier(&render_table.name)
        )?;
        writeln!(writer, "{}", generate_kingbase_create_table(&render_table))?;

        if let Some(pk_stmt) = generate_primary_key(&render_table) {
            writeln!(writer)?;
            writeln!(writer, "{}", pk_stmt)?;
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

    // Sequences and triggers
    let seq_stmts = generate_sequences_for_kingbase(&sequences);
    let mut trig_stmts: Vec<String> = Vec::new();
    for table_details in &table_cache {
        let render_table = table_details.clone();
        trig_stmts.extend(generate_triggers(
            "",
            &render_table.triggers,
            TriggerTerminator::Plain,
        ));
    }

    if !seq_stmts.is_empty() || !trig_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- ============================================")?;
        writeln!(writer, "-- 序列与触发器")?;
        writeln!(writer, "-- ============================================")?;
        writeln!(writer, "-- 重要: 必须先执行序列，再执行触发器")?;
        writeln!(writer, "-- ============================================")?;
    }

    if !seq_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- 序列（第一步：请先执行）")?;
        for stmt in seq_stmts {
            writeln!(writer, "{}", stmt)?;
        }
    }

    if !trig_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(
            writer,
            "-- 触发器（第二步：Oracle 兼容语法，需 KingBase Oracle 兼容模式）"
        )?;
        for stmt in trig_stmts {
            writeln!(writer, "{}", stmt)?;
            writeln!(writer)?;
        }
    }

    writer
        .flush()
        .context("Failed to flush dm8->kingbase ddl export")?;
    Ok(())
}

pub fn export_dm8_to_kingbase_data(
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

    let renderer = renderer_for(&DbType::Kingbase);
    let pipeline = DataExportPipeline {
        source_schema: plan.source_schema.clone(),
        target_schema: plan.target_schema.clone(),
        batch_size,
        identifier_case: identifier_case.to_string(),
        max_parallelism: 4,
        skip_fk_toggle: true, // Cross-DB: DM8 FK constraint names don't exist in Kingbase
        truncate_cascade: true, // Kingbase (PG-compat) requires CASCADE for FK-referenced tables
        skip_trigger_toggle: false,
        use_foreign_key_checks: false,
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

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_qualified_identifier(name: &str) -> String {
    name.split('.')
        .map(quote_identifier)
        .collect::<Vec<_>>()
        .join(".")
}

/// Map a DM8 / MySQL-style column default expression to a KingBase-safe expression.
///
/// Returns `None` when the default cannot be represented safely on KingBase
/// and should be dropped (KingBase column defaults must match the column type;
/// e.g. `TIMESTAMP DEFAULT 0` from MySQL is rejected as `integer != timestamp`).
///
/// Handled cases:
/// - `SYSDATE` / `SYSTIMESTAMP` → `CURRENT_TIMESTAMP`
/// - `CURRENT_TIMESTAMP()` (MySQL empty parens) → `CURRENT_TIMESTAMP`
/// - Bare `0` on a date/time-mapped column → drop (caller emits no DEFAULT)
fn sanitize_kingbase_default(raw: &str, kb_type: &str) -> Option<String> {
    let trimmed = raw.trim();
    let upper = trimmed.to_ascii_uppercase();
    let kb_upper = kb_type.trim().to_ascii_uppercase();
    let is_temporal = kb_upper.starts_with("TIMESTAMP")
        || kb_upper.starts_with("DATE")
        || kb_upper.starts_with("TIME");

    // MySQL "zero timestamp" placeholder — KingBase rejects integer-typed default
    // on temporal columns. Drop so the column simply has no DEFAULT.
    if is_temporal
        && (trimmed == "0"
            || trimmed == "'0'"
            || upper == "'0000-00-00 00:00:00'"
            || upper == "'0000-00-00'")
    {
        return None;
    }

    // Strip MySQL-style empty parentheses on CURRENT_TIMESTAMP keyword.
    // Preserve precision form `CURRENT_TIMESTAMP(n)` unchanged.
    if upper == "CURRENT_TIMESTAMP()" || upper.replace(' ', "") == "CURRENT_TIMESTAMP()" {
        return Some("CURRENT_TIMESTAMP".to_string());
    }

    let mapped = match upper.as_str() {
        "SYSDATE" | "SYSTIMESTAMP" => "CURRENT_TIMESTAMP".to_string(),
        _ => trimmed.to_string(),
    };
    Some(mapped)
}

#[cfg(test)]
mod default_sanitize_tests {
    use super::sanitize_kingbase_default;

    #[test]
    fn drops_zero_default_on_timestamp_column() {
        assert_eq!(sanitize_kingbase_default("0", "TIMESTAMP"), None);
        assert_eq!(sanitize_kingbase_default("0", "TIMESTAMP(0)"), None);
        assert_eq!(sanitize_kingbase_default("0", "DATE"), None);
        assert_eq!(
            sanitize_kingbase_default("'0000-00-00 00:00:00'", "TIMESTAMP"),
            None
        );
        assert_eq!(sanitize_kingbase_default("'0000-00-00'", "DATE"), None);
    }

    #[test]
    fn keeps_zero_default_on_numeric_column() {
        assert_eq!(
            sanitize_kingbase_default("0", "INTEGER"),
            Some("0".to_string())
        );
        assert_eq!(
            sanitize_kingbase_default("0", "SMALLINT"),
            Some("0".to_string())
        );
    }

    #[test]
    fn strips_empty_parens_from_current_timestamp() {
        assert_eq!(
            sanitize_kingbase_default("CURRENT_TIMESTAMP()", "TIMESTAMP"),
            Some("CURRENT_TIMESTAMP".to_string())
        );
        assert_eq!(
            sanitize_kingbase_default("current_timestamp ( )", "TIMESTAMP"),
            Some("CURRENT_TIMESTAMP".to_string())
        );
    }

    #[test]
    fn keeps_current_timestamp_with_precision() {
        assert_eq!(
            sanitize_kingbase_default("CURRENT_TIMESTAMP(6)", "TIMESTAMP"),
            Some("CURRENT_TIMESTAMP(6)".to_string())
        );
    }

    #[test]
    fn maps_oracle_sysdate_keywords() {
        assert_eq!(
            sanitize_kingbase_default("SYSDATE", "TIMESTAMP"),
            Some("CURRENT_TIMESTAMP".to_string())
        );
        assert_eq!(
            sanitize_kingbase_default("SYSTIMESTAMP", "TIMESTAMP"),
            Some("CURRENT_TIMESTAMP".to_string())
        );
    }
}
