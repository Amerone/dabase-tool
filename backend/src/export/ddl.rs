use std::{
    collections::HashSet,
    fmt::Write as FmtWrite,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use chrono::Local;
use odbc_api::Connection;

use crate::{
    db::schema::{fetch_sequences, get_table_details},
    models::{Column, Index, Sequence, TableDetails, TriggerDefinition},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTerminator {
    DataGrip,
    Script,
    DataGripScript,
}

pub fn generate_create_table(table: &TableDetails) -> String {
    let table_ident = quote_identifier(&table.name);

    let column_lines = table
        .columns
        .iter()
        .map(|col| format!("    {}", format_column_definition(col)))
        .collect::<Vec<_>>()
        .join(",\n");

    let mut ddl = String::new();
    let _ = writeln!(
        ddl,
        "CREATE TABLE {} (\n{}\n);",
        table_ident, column_lines
    );

    if let Some(comment) = table.comment.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        let _ = writeln!(
            ddl,
            "COMMENT ON TABLE {} IS '{}';",
            table_ident,
            escape_single_quotes(comment)
        );
    }

    for column in &table.columns {
        if let Some(comment) = column.comment.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            let _ = writeln!(
                ddl,
                "COMMENT ON COLUMN {}.{} IS '{}';",
                table_ident,
                quote_identifier(&column.name),
                escape_single_quotes(comment)
            );
        }
    }

    ddl.trim_end().to_string()
}

pub fn generate_primary_key(table: &TableDetails) -> Option<String> {
    if table.primary_keys.is_empty() {
        return None;
    }

    let columns = table
        .primary_keys
        .iter()
        .map(|s| quote_identifier(s))
        .collect::<Vec<_>>()
        .join(", ");

    let base_name = table
        .name
        .rsplit('.')
        .next()
        .unwrap_or(&table.name);
    let constraint_name = format!("PK_{}", base_name);

    Some(format!(
        "ALTER TABLE {} ADD CONSTRAINT {} PRIMARY KEY ({});",
        quote_identifier(&table.name),
        quote_identifier(&constraint_name),
        columns
    ))
}

pub fn generate_indexes(table: &TableDetails) -> Vec<String> {
    let mut reserved_sets: HashSet<String> = HashSet::new();
    let mut seen_index_keys: HashSet<String> = HashSet::new();

    if !table.primary_keys.is_empty() {
        reserved_sets.insert(normalize_columns_sorted(&table.primary_keys));
    }
    for uc in &table.unique_constraints {
        if !uc.columns.is_empty() {
            reserved_sets.insert(normalize_columns_sorted(&uc.columns));
        }
    }

    table
        .indexes
        .iter()
        .filter_map(|index| {
            if index.columns.is_empty() {
                return None;
            }

            let ordered_key = normalize_columns_ordered(&index.columns);
            let sorted_key = normalize_columns_sorted(&index.columns);

            // Skip indexes that cover the same column set as PK/unique constraints.
            if reserved_sets.contains(&sorted_key) {
                return None;
            }

            // Skip duplicate indexes that use the same ordered column list.
            if seen_index_keys.contains(&ordered_key) {
                return None;
            }
            seen_index_keys.insert(ordered_key);

            let columns = index
                .columns
                .iter()
                .map(|s| quote_identifier(s))
                .collect::<Vec<_>>()
                .join(", ");

            let index_name = normalize_index_name(&table.name, index);

            let prefix = if index.unique {
                "CREATE UNIQUE INDEX"
            } else {
                "CREATE INDEX"
            };

            Some(format!(
                "{} {} ON {} ({});",
                prefix,
                quote_identifier(&index_name),
                quote_identifier(&table.name),
                columns
            ))
        })
        .collect()
}

fn normalize_columns_ordered(columns: &[String]) -> String {
    columns
        .iter()
        .map(|c| c.to_uppercase())
        .collect::<Vec<_>>()
        .join("|")
}

fn normalize_columns_sorted(columns: &[String]) -> String {
    let mut cols = columns.iter().map(|c| c.to_uppercase()).collect::<Vec<_>>();
    cols.sort();
    cols.join("|")
}

fn normalize_index_name(table_name: &str, index: &Index) -> String {
    let upper = index.name.to_uppercase();
    let is_plain_index_number = upper.starts_with("INDEX")
        && upper[5..].chars().all(|c| c.is_ascii_digit());

    if !is_plain_index_number {
        return index.name.clone();
    }

    let table_base = table_name
        .rsplit('.')
        .next()
        .unwrap_or(table_name)
        .to_uppercase();
    let columns = index
        .columns
        .iter()
        .map(|col| col.to_uppercase())
        .collect::<Vec<_>>()
        .join("_");
    let mut name = format!("IDX_{}_{}", table_base, columns);

    // Keep names at a reasonable length to avoid exceeding identifier limits.
    const MAX_LEN: usize = 128;
    if name.len() > MAX_LEN {
        name.truncate(MAX_LEN);
    }

    name
}

pub fn generate_unique_constraints(table: &TableDetails) -> Vec<String> {
    table
        .unique_constraints
        .iter()
        .map(|uc| {
            let columns = uc
                .columns
                .iter()
                .map(|c| quote_identifier(c))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({});",
                quote_identifier(&table.name),
                quote_identifier(&uc.name),
                columns
            )
        })
        .collect()
}

pub fn generate_check_constraints(table: &TableDetails) -> Vec<String> {
    table
        .check_constraints
        .iter()
        .map(|ck| {
            format!(
                "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({});",
                quote_identifier(&table.name),
                quote_identifier(&ck.name),
                ck.condition
            )
        })
        .collect()
}

pub fn generate_foreign_keys(table: &TableDetails) -> Vec<String> {
    table
        .foreign_keys
        .iter()
        .map(|fk| {
            let cols = fk
                .columns
                .iter()
                .map(|c| quote_identifier(c))
                .collect::<Vec<_>>()
                .join(", ");
            let ref_cols = fk
                .referenced_columns
                .iter()
                .map(|c| quote_identifier(c))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = format!(
                "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
                quote_identifier(&table.name),
                quote_identifier(&fk.name),
                cols,
                quote_identifier(&fk.referenced_table),
                ref_cols
            );
            // Add ON DELETE rule if specified and not NO ACTION
            if let Some(rule) = fk
                .delete_rule
                .as_deref()
                .filter(|r| !r.is_empty() && !r.eq_ignore_ascii_case("NO ACTION"))
            {
                stmt.push_str(&format!(" ON DELETE {}", rule));
            }
            // Add ON UPDATE rule if specified and not NO ACTION
            if let Some(rule) = fk
                .update_rule
                .as_deref()
                .filter(|r| !r.is_empty() && !r.eq_ignore_ascii_case("NO ACTION"))
            {
                stmt.push_str(&format!(" ON UPDATE {}", rule));
            }
            stmt.push(';');
            stmt
        })
        .collect()
}

pub fn generate_sequences(schema: &str, sequences: &[Sequence]) -> Vec<String> {
    sequences
        .iter()
        .map(|seq| {
            // 达梦不支持 CREATE OR REPLACE SEQUENCE，只支持 CREATE SEQUENCE
            let mut stmt = format!(
                "CREATE SEQUENCE {}.{}",
                quote_identifier(schema),
                quote_identifier(&seq.name)
            );
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
            // CACHE 0 或 None 都应输出为 NOCACHE
            match seq.cache_size {
                Some(cache) if cache > 0 => stmt.push_str(&format!(" CACHE {}", cache)),
                _ => stmt.push_str(" NOCACHE"),
            }
            if seq.cycle {
                stmt.push_str(" CYCLE");
            } else {
                stmt.push_str(" NOCYCLE");
            }
            if seq.order {
                stmt.push_str(" ORDER");
            } else {
                stmt.push_str(" NOORDER");
            }
            stmt.push(';');
            stmt
        })
        .collect()
}

pub fn generate_triggers(
    schema: &str,
    triggers: &[TriggerDefinition],
    terminator: TriggerTerminator,
) -> Vec<String> {
    // DataGripScript 模式下，触发器会被输出到单独的文件，使用 Script 格式
    let effective_terminator = if terminator == TriggerTerminator::DataGripScript {
        TriggerTerminator::Script
    } else {
        terminator
    };

    triggers
        .iter()
        .map(|tr| {
            let body_trimmed = tr.body.trim();
            let body_upper = body_trimmed.to_uppercase();
            if body_upper.starts_with("CREATE TRIGGER")
                || body_upper.starts_with("CREATE OR REPLACE TRIGGER")
            {
                let mut stmt = normalize_trigger_body(body_trimmed);
                apply_trigger_terminator(&mut stmt, effective_terminator);
                return stmt;
            }

            // Extract WHEN clause if present in body (only valid for row-level triggers)
            let (when_clause, body_without_when) = if tr.each_row {
                extract_when_clause(body_trimmed)
            } else {
                (String::new(), body_trimmed.to_string())
            };

            let events = tr.events.join(" OR ");
            let mut stmt = format!(
                "CREATE OR REPLACE TRIGGER {}.{}\n{} {} ON {}",
                quote_identifier(schema),
                quote_identifier(&tr.name),
                tr.timing,
                events,
                quote_identifier(&format!("{}.{}", schema, tr.table_name))
            );
            if tr.each_row {
                stmt.push_str(" REFERENCING OLD AS OLD NEW AS NEW");
            }
            if tr.each_row {
                stmt.push_str("\nFOR EACH ROW");
            }

            // Add WHEN clause after FOR EACH ROW if present
            let when_clause = normalize_trigger_references(&when_clause);
            if !when_clause.is_empty() {
                stmt.push_str(&format!("\nWHEN ({})", when_clause));
            }

            stmt.push('\n');
            let body_without_when = normalize_trigger_references(&body_without_when);
            let normalized_body = normalize_trigger_body(&body_without_when);
            let body_start_upper = normalized_body.trim_start().to_uppercase();

            // Don't wrap if body already starts with BEGIN or DECLARE
            if !body_start_upper.starts_with("BEGIN") && !body_start_upper.starts_with("DECLARE") {
                stmt.push_str("BEGIN\n");
                stmt.push_str(normalized_body.trim());
                stmt.push_str("\nEND");
            } else {
                stmt.push_str(normalized_body.trim());
            }
            if !stmt.trim_end().ends_with(';') {
                stmt.push(';');
            }
            apply_trigger_terminator(&mut stmt, effective_terminator);
            stmt
        })
        .collect()
}

fn apply_trigger_terminator(stmt: &mut String, terminator: TriggerTerminator) {
    if !stmt.trim_end().ends_with(';') {
        stmt.push(';');
    }

    if terminator == TriggerTerminator::Script {
        let trimmed = stmt.trim_end();
        if !trimmed.ends_with('/') {
            if !stmt.ends_with('\n') {
                stmt.push('\n');
            }
            stmt.push('/');
        }
    }
}


pub fn export_schema_ddl(
    connection: &Connection<'_>,
    source_schema: &str,
    target_schema: &str,
    tables: &[String],
    output_path: &Path,
    drop_existing: bool,
    trigger_terminator: TriggerTerminator,
) -> Result<()> {
    let source_schema = source_schema.to_uppercase();
    let target_schema = target_schema.to_uppercase();

    // Cache table details to avoid repeated queries.
    let mut table_cache = Vec::new();
    for table_name in tables {
        let details =
            get_table_details(connection, &source_schema, table_name).with_context(|| {
                format!("Failed to fetch table metadata for '{}'", table_name)
            })?;
        table_cache.push(details);
    }

    let sequences = fetch_sequences(connection, &source_schema).unwrap_or_default();

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                output_path.display()
            )
        })?;
    }

    let file = File::create(output_path).with_context(|| {
        format!("Failed to create DDL export file at {}", output_path.display())
    })?;
    let mut writer = BufWriter::new(file);

    // File header
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    // 生成表名列表
    let table_names: Vec<String> = table_cache.iter().map(|t| t.name.clone()).collect();

    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- DM8 DDL 导出脚本")?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer, "-- 生成时间: {}", timestamp)?;
    writeln!(writer, "-- 源 Schema: {}", source_schema)?;
    writeln!(writer, "-- 目标 Schema: {}", target_schema)?;
    writeln!(writer, "-- 表数量: {}", tables.len())?;
    writeln!(writer, "-- 涉及的表: {}", table_names.join(", "))?;
    writeln!(writer, "--")?;
    if trigger_terminator == TriggerTerminator::DataGripScript {
        writeln!(writer, "-- 执行方式: DataGrip 脚本模式")?;
        writeln!(writer, "-- 注意: 触发器已导出到单独的文件，请使用 DIsql 或其他达梦原生工具执行")?;
    } else if trigger_terminator == TriggerTerminator::Script {
        writeln!(writer, "-- 执行方式: 脚本模式 (DBeaver/SQLark/DIsql)")?;
        writeln!(writer, "-- 注意: 触发器使用 / 作为语句分隔符")?;
    } else {
        writeln!(writer, "-- 执行方式: DataGrip 逐语句运行")?;
        writeln!(writer, "-- 注意: 请在 DataGrip 中逐条执行语句")?;
    }
    if drop_existing {
        writeln!(writer, "-- 警告: 此脚本会先删除已存在的表再重新创建")?;
    } else {
        writeln!(writer, "-- 说明: 此脚本不会删除已存在的表")?;
    }
    writeln!(writer, "-- 重要: 触发器通常依赖 SEQUENCE (序列) 生成主键")?;
    writeln!(writer, "-- 重要: 必须先执行 SEQUENCE 再执行触发器")?;
    writeln!(writer, "-- ============================================")?;
    writeln!(writer)?;

    for (i, table_details) in table_cache.iter().enumerate() {
        let mut render_table = table_details.clone();
        render_table.name = format!("{}.{}", target_schema, table_details.name);

        if i > 0 {
            writeln!(writer)?;
        }

        writeln!(
            writer,
            "-- 表: {}",
            quote_identifier(&render_table.name)
        )?;
        if drop_existing {
            writeln!(
                writer,
                "DROP TABLE IF EXISTS {};",
                quote_identifier(&render_table.name)
            )?;
        }
        writeln!(writer, "{}", generate_create_table(&render_table))?;

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

        let index_statements = generate_indexes(&render_table);
        if !index_statements.is_empty() {
            writeln!(writer)?;
            for stmt in index_statements {
                writeln!(writer, "{}", stmt)?;
            }
        }
    }

    // Emit foreign keys after all tables to reduce dependency issues.
    let mut fk_statements = Vec::new();
    for table_details in &table_cache {
        let mut render_table = table_details.clone();
        render_table.name = format!("{}.{}", target_schema, table_details.name);
        fk_statements.extend(generate_foreign_keys(&render_table));
    }

    if !fk_statements.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- 外键")?;
        for stmt in fk_statements {
            writeln!(writer, "{}", stmt)?;
        }
    }

    // Emit sequences and triggers together as a related section.
    let seq_stmts = generate_sequences(&target_schema, &sequences);
    let mut trig_stmts = Vec::new();
    for table_details in &table_cache {
        let mut render_table = table_details.clone();
        render_table.name = format!("{}.{}", target_schema, table_details.name);
        trig_stmts.extend(generate_triggers(
            &target_schema,
            &render_table.triggers,
            trigger_terminator,
        ));
    }

    // 只有当存在 SEQUENCE 或触发器时才输出这个 section
    if !seq_stmts.is_empty() || !trig_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- ============================================")?;
        writeln!(writer, "-- SEQUENCE 与触发器")?;
        writeln!(writer, "-- ============================================")?;
        writeln!(writer, "-- 重要: 必须先执行 SEQUENCE 再执行触发器")?;
        writeln!(writer, "-- ============================================")?;
    }

    // 输出 SEQUENCE
    if !seq_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- SEQUENCE (第一步: 请先执行)")?;
        for stmt in seq_stmts {
            writeln!(writer, "{}", stmt)?;
        }
    }

    // 对于 DataGripScript 模式，将触发器输出到单独的文件
    if trigger_terminator == TriggerTerminator::DataGripScript && !trig_stmts.is_empty() {
        // 收集触发器涉及的表名
        let trigger_tables: Vec<String> = table_cache
            .iter()
            .filter(|t| !t.triggers.is_empty())
            .map(|t| t.name.clone())
            .collect();

        let trigger_path = output_path.with_extension("triggers.sql");
        let trigger_file = File::create(&trigger_path).with_context(|| {
            format!(
                "Failed to create trigger export file at {}",
                trigger_path.display()
            )
        })?;
        let mut trigger_writer = BufWriter::new(trigger_file);

        writeln!(trigger_writer, "-- ============================================")?;
        writeln!(trigger_writer, "-- DM8 触发器 DDL 导出脚本")?;
        writeln!(trigger_writer, "-- ============================================")?;
        writeln!(trigger_writer, "-- 生成时间: {}", timestamp)?;
        writeln!(trigger_writer, "-- 目标 Schema: {}", target_schema)?;
        writeln!(trigger_writer, "-- 触发器数量: {}", trig_stmts.len())?;
        writeln!(trigger_writer, "-- 涉及的表: {}", trigger_tables.join(", "))?;
        writeln!(trigger_writer, "--")?;
        writeln!(trigger_writer, "-- 执行方式:")?;
        writeln!(trigger_writer, "--   1. 使用 DIsql 命令行工具: disql USER/PASSWORD@HOST:PORT -f 此文件路径")?;
        writeln!(trigger_writer, "--   2. 使用达梦管理工具打开此文件并执行")?;
        writeln!(trigger_writer, "--   3. 在 DataGrip 中逐条选中触发器语句执行 (不要使用 Run Script)")?;
        writeln!(trigger_writer, "--")?;
        writeln!(trigger_writer, "-- 重要: 必须先执行主DDL文件中的 SEQUENCE，再执行本文件")?;
        writeln!(trigger_writer, "-- 注意: 每个触发器以 / 结尾作为语句分隔符")?;
        writeln!(trigger_writer, "-- ============================================")?;
        writeln!(trigger_writer)?;
        for stmt in &trig_stmts {
            writeln!(trigger_writer, "{}", stmt)?;
            writeln!(trigger_writer)?;
        }
        trigger_writer
            .flush()
            .context("Failed to flush trigger export to disk")?;

        // 在主文件中添加提示
        writeln!(writer)?;
        writeln!(writer, "-- 触发器 (第二步: 请在 SEQUENCE 之后执行)")?;
        writeln!(
            writer,
            "-- 注意: 触发器已导出到单独的文件: {}",
            trigger_path.file_name().unwrap_or_default().to_string_lossy()
        )?;
        writeln!(
            writer,
            "-- 请使用 DIsql 或其他达梦原生工具执行该文件"
        )?;
    } else if !trig_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- 触发器 (第二步: 请在 SEQUENCE 之后执行)")?;
        for stmt in trig_stmts {
            writeln!(writer, "{}", stmt)?;
        }
    }

    writer.flush().context("Failed to flush DDL export to disk")?;
    Ok(())
}

fn format_column_definition(column: &Column) -> String {
    let mut parts = Vec::new();
    parts.push(quote_identifier(&column.name));
    parts.push(format_data_type(column));

    if column.identity {
        // IDENTITY column - DM8 syntax: IDENTITY(seed, increment)
        // Note: IDENTITY columns cannot have DEFAULT clause
        if let (Some(start), Some(inc)) = (column.identity_start, column.identity_increment) {
            parts.push(format!("IDENTITY({}, {})", start, inc));
        } else {
            // Default: IDENTITY(1, 1)
            parts.push("IDENTITY(1, 1)".to_string());
        }
    } else if let Some(default) = column
        .default_value
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        // Non-identity column with DEFAULT value
        parts.push(format!("DEFAULT {}", format_default(column, default)));
    }

    let nullability = if column.nullable { "NULL" } else { "NOT NULL" };
    parts.push(nullability.to_string());

    parts.join(" ")
}

fn format_data_type(column: &Column) -> String {
    let mut data_type = column.data_type.trim().to_uppercase();

    // If data type already contains precision/length info, return as-is
    if data_type.contains('(') {
        return data_type;
    }

    match data_type.as_str() {
        // String types: use length with CHAR/BYTE semantics
        "VARCHAR" | "VARCHAR2" | "CHAR" | "NCHAR" | "NVARCHAR" | "NVARCHAR2" | "RAW"
        | "BINARY" | "VARBINARY" => {
            if let Some(len) = column.length.filter(|l| *l > 0) {
                if let Some(cs) = column.char_semantics.as_deref().map(str::to_uppercase) {
                    // DM8 CHAR_USED: 'C' = CHAR semantics, 'B' = BYTE semantics
                    if cs == "C" || cs.contains("CHAR") {
                        data_type = format!("{}({} CHAR)", data_type, len);
                    } else if cs == "B" || cs.contains("BYTE") {
                        data_type = format!("{}({} BYTE)", data_type, len);
                    } else {
                        data_type = format!("{}({})", data_type, len);
                    }
                } else {
                    data_type = format!("{}({})", data_type, len);
                }
            }
        }
        // Numeric types with precision and scale
        "NUMBER" | "DECIMAL" | "NUMERIC" => {
            // Only use precision/scale, never fall back to length (which is byte size)
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                if let Some(scale) = column.scale.filter(|s| *s > 0) {
                    data_type = format!("{}({},{})", data_type, prec, scale);
                } else if column.scale == Some(0) {
                    // Explicit scale of 0 means integer
                    data_type = format!("{}({},0)", data_type, prec);
                } else {
                    data_type = format!("{}({})", data_type, prec);
                }
            }
            // If no precision, leave as NUMBER without parentheses
        }
        // Float types with precision
        "FLOAT" | "DOUBLE" | "REAL" => {
            if let Some(prec) = column.precision.filter(|p| *p > 0) {
                data_type = format!("{}({})", data_type, prec);
            }
        }
        // Timestamp types with fractional seconds precision
        "TIMESTAMP" => {
            // scale field often holds fractional seconds precision for TIMESTAMP
            if let Some(fsp) = column.scale.filter(|s| *s >= 0 && *s <= 9) {
                if fsp != 6 {
                    // 6 is default, only specify if different
                    data_type = format!("TIMESTAMP({})", fsp);
                }
            }
        }
        // These types don't need length/precision in DDL
        "DATE" | "BLOB" | "CLOB" | "NCLOB" | "TEXT" | "LONG" | "LONGVARBINARY"
        | "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "BIT" | "BOOLEAN" => {
            // Keep as-is without modifications
        }
        _ => {
            // For TIMESTAMP WITH TIME ZONE, TIMESTAMP WITH LOCAL TIME ZONE, etc.
            // These complex type names should be preserved as-is
        }
    }

    data_type
}

fn format_default(column: &Column, raw: &str) -> String {
    let dt = column.data_type.trim().to_uppercase();
    let expr = raw.trim();
    let expr_upper = expr.to_uppercase();

    // === Pass-through patterns: expressions that should never be modified ===

    // NULL keyword
    if expr_upper == "NULL" {
        return "NULL".to_string();
    }

    // Already quoted string literal: 'value' - but for DATE/TIMESTAMP types,
    // we should wrap with TO_DATE/TO_TIMESTAMP to avoid NLS dependency
    if expr.starts_with('\'') && expr.ends_with('\'') && expr.len() >= 2 {
        let inner = &expr[1..expr.len() - 1];
        // For DATE/TIMESTAMP types with quoted date-like values, wrap explicitly
        if dt == "DATE" && is_date_literal(inner) {
            let format_str = if inner.contains(':') {
                "YYYY-MM-DD HH24:MI:SS"
            } else {
                "YYYY-MM-DD"
            };
            return format!("TO_DATE('{}','{}')", escape_single_quotes(inner), format_str);
        }
        if dt.starts_with("TIMESTAMP") && (is_date_literal(inner) || is_timestamp_literal(inner)) {
            let normalized = normalize_iso_timestamp(inner);
            let format_str = build_timestamp_format(&normalized, dt.contains("TIME ZONE"));
            if dt.contains("TIME ZONE") && has_timezone(&normalized) {
                return format!("TO_TIMESTAMP_TZ('{}','{}')", escape_single_quotes(&normalized), format_str);
            }
            return format!("TO_TIMESTAMP('{}','{}')", escape_single_quotes(&normalized), format_str);
        }
        return expr.to_string();
    }

    // National character string literal: N'value'
    if (expr_upper.starts_with("N'")) && expr.ends_with('\'') {
        return expr.to_string();
    }

    // Hex literal: X'0A0B' or 0x0A0B
    if (expr_upper.starts_with("X'") && expr.ends_with('\''))
        || expr_upper.starts_with("0X")
    {
        return expr.to_string();
    }

    // DATE/TIMESTAMP/INTERVAL literal syntax
    if expr_upper.starts_with("DATE ")
        || expr_upper.starts_with("TIMESTAMP ")
        || expr_upper.starts_with("INTERVAL ")
    {
        return expr.to_string();
    }

    // Function calls or complex expressions with parentheses
    if expr.contains('(') {
        return expr.to_string();
    }

    // String concatenation operator
    if expr.contains("||") {
        return expr.to_string();
    }

    // CASE expressions
    if expr_upper.starts_with("CASE ") || expr_upper.contains(" CASE ") {
        return expr.to_string();
    }

    // NEXT VALUE FOR sequence
    if expr_upper.starts_with("NEXT VALUE FOR") || expr_upper.contains(".NEXTVAL") {
        return expr.to_string();
    }

    // SQL keywords that should never be quoted (including space variants)
    const SQL_KEYWORDS: &[&str] = &[
        "SYSDATE",
        "SYSTIMESTAMP",
        "CURRENT_DATE",
        "CURRENT_TIME",
        "CURRENT_TIMESTAMP",
        "LOCALTIMESTAMP",
        "LOCALTIME",
        "USER",
        "CURRENT_USER",
        "CURRENT USER",
        "SESSION_USER",
        "SESSION USER",
        "CURRENT_SCHEMA",
        "CURRENT SCHEMA",
        "CURRENT_ROLE",
        "CURRENT ROLE",
        "DBTIMEZONE",
        "SESSIONTIMEZONE",
        "TRUE",
        "FALSE",
    ];

    // Check if expression is or starts with a SQL keyword
    for kw in SQL_KEYWORDS {
        if expr_upper == *kw {
            return expr.to_string();
        }
        // Keyword followed by operator or space (e.g., "CURRENT_DATE + 1")
        if expr_upper.starts_with(kw) {
            let rest = &expr_upper[kw.len()..];
            if rest.is_empty()
                || rest.starts_with(' ')
                || rest.starts_with('+')
                || rest.starts_with('-')
                || rest.starts_with('*')
                || rest.starts_with('/')
            {
                return expr.to_string();
            }
        }
    }

    // Arithmetic expressions with operators (but need to distinguish from date literals)
    let looks_like_date_literal = is_date_literal(expr);

    if !looks_like_date_literal {
        // Check for arithmetic operators
        for (i, c) in expr.char_indices() {
            if c == '*' || c == '/' {
                return expr.to_string();
            }
            // Plus could be addition or timezone offset
            if c == '+' && i > 0 {
                // If it looks like timezone offset (+08:00), don't treat as expression
                let rest = &expr[i..];
                if !rest.starts_with("+0") && !rest.starts_with("+1") {
                    return expr.to_string();
                }
            }
            // Minus is tricky: could be subtraction or negative number or date separator
            if c == '-' && i > 0 {
                let prev_char = expr.chars().nth(i - 1);
                // If preceded by space or closing paren, it's likely subtraction
                if matches!(prev_char, Some(' ') | Some(')')) {
                    return expr.to_string();
                }
                // If preceded by a letter (like "SYSDATE-1"), it's subtraction
                if prev_char.map_or(false, |c| c.is_ascii_alphabetic()) {
                    return expr.to_string();
                }
            }
        }
    }

    // === Type-specific formatting for literal values ===

    // For string types: only quote if it looks like a plain literal value
    if is_string_type(&dt) {
        // If it looks like an expression or keyword, pass through
        if looks_like_expression(expr) {
            return expr.to_string();
        }
        return format!("'{}'", escape_single_quotes(expr));
    }

    // For numeric types: check if it's a valid number (including scientific notation)
    if is_numeric_type(&dt) {
        if is_numeric_literal(expr) {
            return expr.to_string();
        }
        // Not a simple number, might be an expression
        return expr.to_string();
    }

    // DATE type: wrap with TO_DATE if it looks like a date literal
    if dt == "DATE" {
        if looks_like_date_literal {
            let format_str = if expr.contains(':') {
                "YYYY-MM-DD HH24:MI:SS"
            } else {
                "YYYY-MM-DD"
            };
            return format!("TO_DATE('{}','{}')", escape_single_quotes(expr), format_str);
        }
        // Not a date literal, pass through as expression
        return expr.to_string();
    }

    // TIMESTAMP types
    if dt.starts_with("TIMESTAMP") {
        if looks_like_date_literal || is_timestamp_literal(expr) {
            let normalized = normalize_iso_timestamp(expr);
            let format_str = build_timestamp_format(&normalized, dt.contains("TIME ZONE"));
            // For TIMESTAMP WITH TIME ZONE, use TO_TIMESTAMP_TZ if timezone present
            if dt.contains("TIME ZONE") && has_timezone(&normalized) {
                return format!(
                    "TO_TIMESTAMP_TZ('{}','{}')",
                    escape_single_quotes(&normalized),
                    format_str
                );
            }
            return format!(
                "TO_TIMESTAMP('{}','{}')",
                escape_single_quotes(&normalized),
                format_str
            );
        }
        return expr.to_string();
    }

    // Binary types
    if is_binary_type(&dt) {
        if expr_upper.starts_with("HEXTORAW") || expr_upper.starts_with("X'") {
            return expr.to_string();
        }
        // Only wrap if it looks like hex data
        if expr.chars().all(|c| c.is_ascii_hexdigit()) {
            return format!("HEXTORAW('{}')", expr);
        }
        return expr.to_string();
    }

    // Default: pass through as-is (safer than guessing)
    expr.to_string()
}

/// Normalize ISO 8601 timestamp to DM8 compatible format
/// - Replace 'T' with space
/// - Replace 'Z' with '+00:00'
fn normalize_iso_timestamp(expr: &str) -> String {
    let mut result = expr.replace('T', " ");
    if result.ends_with('Z') {
        result = format!("{}+00:00", &result[..result.len() - 1]);
    }
    result
}

/// Build appropriate timestamp format string based on the value
fn build_timestamp_format(expr: &str, with_timezone: bool) -> String {
    let mut format = String::from("YYYY-MM-DD HH24:MI:SS");

    // Check for fractional seconds (digits after the last colon's seconds)
    if let Some(dot_pos) = expr.rfind('.') {
        // Make sure the dot is after the time part, not in the date
        if expr[..dot_pos].contains(':') {
            format.push_str(".FF");
        }
    }

    // Check for timezone
    if with_timezone && has_timezone(expr) {
        format.push_str(" TZH:TZM");
    }

    format
}

/// Check if expression has timezone information
fn has_timezone(expr: &str) -> bool {
    // Look for +HH:MM or -HH:MM at the end (but not date separators)
    if let Some(pos) = expr.rfind('+') {
        let rest = &expr[pos..];
        // Timezone pattern: +HH:MM or +HHMM
        return rest.len() >= 5 && rest[1..].chars().next().map_or(false, |c| c.is_ascii_digit());
    }
    if let Some(pos) = expr.rfind('-') {
        // Make sure it's not a date separator (position should be after time part)
        if expr[..pos].contains(':') {
            let rest = &expr[pos..];
            return rest.len() >= 5 && rest[1..].chars().next().map_or(false, |c| c.is_ascii_digit());
        }
    }
    false
}

/// Check if the data type is a string type
fn is_string_type(dt: &str) -> bool {
    dt == "CHAR"
        || dt == "NCHAR"
        || dt == "VARCHAR"
        || dt == "VARCHAR2"
        || dt == "NVARCHAR"
        || dt == "NVARCHAR2"
        || dt == "TEXT"
        || dt == "CLOB"
        || dt == "NCLOB"
        || dt == "LONG"
        || dt == "LONG VARCHAR"
        || dt.starts_with("CHAR(")
        || dt.starts_with("VARCHAR(")
        || dt.starts_with("VARCHAR2(")
        || dt.starts_with("NCHAR(")
        || dt.starts_with("NVARCHAR(")
        || dt.starts_with("NVARCHAR2(")
}

/// Check if the data type is a numeric type
fn is_numeric_type(dt: &str) -> bool {
    dt == "NUMBER"
        || dt == "INTEGER"
        || dt == "INT"
        || dt == "SMALLINT"
        || dt == "TINYINT"
        || dt == "BIGINT"
        || dt == "DECIMAL"
        || dt == "NUMERIC"
        || dt == "FLOAT"
        || dt == "DOUBLE"
        || dt == "DOUBLE PRECISION"
        || dt == "REAL"
        || dt == "BYTE"
        || dt.starts_with("NUMBER(")
        || dt.starts_with("DECIMAL(")
        || dt.starts_with("NUMERIC(")
        || dt.starts_with("FLOAT(")
}

/// Check if the data type is a binary type
fn is_binary_type(dt: &str) -> bool {
    dt == "RAW"
        || dt == "BINARY"
        || dt == "VARBINARY"
        || dt == "BLOB"
        || dt == "LONGVARBINARY"
        || dt.starts_with("RAW(")
        || dt.starts_with("BINARY(")
        || dt.starts_with("VARBINARY(")
}

/// Check if expression looks like a numeric literal (including scientific notation)
fn is_numeric_literal(expr: &str) -> bool {
    if expr.is_empty() {
        return false;
    }

    let mut chars = expr.chars().peekable();

    // Optional leading sign
    if matches!(chars.peek(), Some('+') | Some('-')) {
        chars.next();
    }

    let mut has_digit = false;
    let mut has_dot = false;
    let mut has_exp = false;

    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            has_digit = true;
        } else if c == '.' && !has_dot && !has_exp {
            has_dot = true;
        } else if (c == 'e' || c == 'E') && has_digit && !has_exp {
            has_exp = true;
            // Optional sign after exponent
            if matches!(chars.peek(), Some('+') | Some('-')) {
                chars.next();
            }
            has_digit = false; // Need digits after exponent
        } else {
            return false;
        }
    }

    has_digit
}

/// Check if expression looks like a date literal (YYYY-MM-DD format)
fn is_date_literal(expr: &str) -> bool {
    let parts: Vec<&str> = expr
        .split(|c| c == '-' || c == ' ' || c == ':' || c == '.' || c == 'T')
        .collect();

    if parts.len() < 3 {
        return false;
    }

    // First part should be 4-digit year
    if parts[0].len() != 4 || !parts[0].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    // Second part should be 1-2 digit month
    if parts[1].is_empty()
        || parts[1].len() > 2
        || !parts[1].chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }

    // Third part should be 1-2 digit day
    if parts[2].is_empty()
        || parts[2].len() > 2
        || !parts[2].chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }

    true
}

/// Check if expression looks like a timestamp literal (with time component)
fn is_timestamp_literal(expr: &str) -> bool {
    // Must have date part
    if !is_date_literal(expr) {
        return false;
    }

    // Should have time separator
    expr.contains(':') || expr.contains('T')
}

/// Check if expression looks like a SQL expression (not a plain literal)
fn looks_like_expression(expr: &str) -> bool {
    let upper = expr.to_uppercase();

    // Contains SQL operators or keywords
    upper.contains("||")
        || upper.contains(" AND ")
        || upper.contains(" OR ")
        || upper.contains(" CASE ")
        || upper.starts_with("CASE ")
        || upper.contains(".NEXTVAL")
        || upper.contains(".CURRVAL")
        || expr.contains('(')
        || expr.contains(')')
}

fn quote_identifier(identifier: &str) -> String {
    identifier
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "''")
}

fn extract_when_clause(body: &str) -> (String, String) {
    let lines: Vec<&str> = body.lines().collect();
    let mut when_clause = String::new();
    let mut body_lines = Vec::new();
    let mut in_when = false;
    let mut paren_depth = 0;

    for line in lines {
        let trimmed = line.trim();
        let upper = trimmed.to_uppercase();

        // Match WHEN followed by optional whitespace and opening parenthesis
        if upper.starts_with("WHEN") && !in_when {
            let after_when = &trimmed[4..].trim_start();
            if after_when.starts_with('(') {
                in_when = true;
                paren_depth = 0;

                // Process the rest of the line
                for ch in after_when.chars() {
                    if ch == '(' {
                        paren_depth += 1;
                        if paren_depth > 1 {
                            // Include nested parentheses in the clause
                            when_clause.push(ch);
                        }
                    } else if ch == ')' {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            in_when = false;
                            break;
                        }
                        when_clause.push(ch);
                    } else if paren_depth > 0 {
                        when_clause.push(ch);
                    }
                }
                continue;
            }
        }

        if in_when {
            // Continue collecting WHEN clause with proper parenthesis tracking
            for ch in trimmed.chars() {
                if ch == '(' {
                    paren_depth += 1;
                    when_clause.push(ch);
                } else if ch == ')' {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        in_when = false;
                        break;
                    }
                    when_clause.push(ch);
                } else {
                    when_clause.push(ch);
                }
            }
            if in_when {
                when_clause.push(' ');
            }
        } else {
            body_lines.push(line);
        }
    }

    (when_clause.trim().to_string(), body_lines.join("\n"))
}

#[cfg(test)]
mod format_default_tests {
    use super::format_default;
    use crate::models::Column;

    fn column_with_type(data_type: &str) -> Column {
        Column {
            name: "col".to_string(),
            data_type: data_type.to_string(),
            length: None,
            precision: None,
            scale: None,
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
    fn format_default_keeps_user_keyword_for_string_types() {
        let column = column_with_type("VARCHAR");
        assert_eq!(format_default(&column, "USER"), "USER");
    }

    #[test]
    fn format_default_keeps_current_date_expression() {
        let column = column_with_type("DATE");
        assert_eq!(
            format_default(&column, "CURRENT_DATE + 1"),
            "CURRENT_DATE + 1"
        );
    }

    #[test]
    fn format_default_keeps_locals_timestamp_keyword() {
        let column = column_with_type("TIMESTAMP");
        assert_eq!(format_default(&column, "LOCALTIMESTAMP"), "LOCALTIMESTAMP");
    }

    #[test]
    fn format_default_keeps_date_literal_expression() {
        let column = column_with_type("DATE");
        assert_eq!(
            format_default(&column, "DATE '2024-01-01'"),
            "DATE '2024-01-01'"
        );
    }

    #[test]
    fn format_default_keeps_n_quoted_string_literal() {
        let column = column_with_type("VARCHAR");
        assert_eq!(format_default(&column, "N'abc'"), "N'abc'");
    }

    #[test]
    fn format_default_keeps_hex_literal_for_raw() {
        let column = column_with_type("RAW");
        assert_eq!(format_default(&column, "X'0A0B'"), "X'0A0B'");
    }

    #[test]
    fn format_default_wraps_date_only_literal_with_to_date() {
        let column = column_with_type("DATE");
        assert_eq!(
            format_default(&column, "2024-01-01"),
            "TO_DATE('2024-01-01','YYYY-MM-DD')"
        );
    }

    #[test]
    fn format_default_wraps_timestamp_literal_without_fraction() {
        let column = column_with_type("TIMESTAMP");
        assert_eq!(
            format_default(&column, "2024-01-01 12:34:56"),
            "TO_TIMESTAMP('2024-01-01 12:34:56','YYYY-MM-DD HH24:MI:SS')"
        );
    }

    #[test]
    fn format_default_wraps_timestamp_literal_with_fraction() {
        let column = column_with_type("TIMESTAMP");
        assert_eq!(
            format_default(&column, "2024-01-01 12:34:56.123"),
            "TO_TIMESTAMP('2024-01-01 12:34:56.123','YYYY-MM-DD HH24:MI:SS.FF')"
        );
    }
}

fn normalize_trigger_body(body: &str) -> String {
    let mut lines = Vec::new();
    let mut cumulative_paren_depth = 0;

    // First pass: identify lines that are part of SELECT...INTO statements
    let all_lines: Vec<&str> = body.lines().collect();
    let mut is_select_into_line = vec![false; all_lines.len()];

    for (i, line) in all_lines.iter().enumerate() {
        let upper = line.trim().to_uppercase();
        if upper.starts_with("SELECT ") {
            // Check if there's an INTO in the following lines before a semicolon
            let mut found_into = false;
            let mut into_idx = i;
            for j in (i + 1)..all_lines.len() {
                let next_upper = all_lines[j].trim().to_uppercase();
                if next_upper.starts_with("INTO ") {
                    found_into = true;
                    into_idx = j;
                    break;
                }
                if next_upper.ends_with(';') || next_upper.starts_with("SELECT ") {
                    break;
                }
            }

            if found_into {
                // Find the end of the statement (after FROM clause or subquery)
                let mut end_idx = into_idx;
                let mut depth = 0;
                for j in (into_idx + 1)..all_lines.len() {
                    let next_line = all_lines[j].trim();
                    let next_upper = next_line.to_uppercase();

                    // Track parenthesis depth
                    depth += next_line.matches('(').count() as i32;
                    depth -= next_line.matches(')').count() as i32;

                    // If we're at depth 0 and hit a line that could end the statement
                    if depth == 0 && (next_upper.ends_with(';')
                        || next_upper.starts_with("SELECT ")
                        || next_upper.starts_with("INSERT ")
                        || next_upper.starts_with("UPDATE ")
                        || next_upper.starts_with("DELETE ")
                        || next_upper.contains(":NEW.")
                        || next_upper.contains(":OLD.")
                        || next_upper.contains(":=")
                        || next_upper.starts_with("END")) {
                        break;
                    }

                    end_idx = j;
                }

                // Mark all lines from SELECT to end of statement
                for k in i..=end_idx {
                    is_select_into_line[k] = true;
                }
            }
        }
    }

    // Second pass: add semicolons where needed
    for (idx, line) in all_lines.iter().enumerate() {
        let trimmed = line.trim_end();
        let upper = trimmed.trim_start().to_uppercase();
        let mut new_line = trimmed.to_string();

        // Skip empty lines
        if upper.is_empty() {
            lines.push(new_line);
            continue;
        }

        // Track cumulative parenthesis depth across lines
        let open_parens = trimmed.matches('(').count();
        let close_parens = trimmed.matches(')').count();
        let prev_depth = cumulative_paren_depth;
        cumulative_paren_depth += open_parens as i32 - close_parens as i32;

        // Check if this is the last line of a SELECT...INTO statement
        let is_last_select_into_line = is_select_into_line[idx]
            && (idx + 1 >= all_lines.len() || !is_select_into_line[idx + 1]);

        // Don't add semicolon if:
        // - Line already ends with semicolon
        // - We're inside unclosed parentheses (either before or after this line)
        // - Line is part of a SELECT...INTO statement (except the last line)
        // - Line is a control structure keyword
        // - Line is a block delimiter
        let needs_semicolon = !upper.ends_with(';')
            && prev_depth == 0
            && cumulative_paren_depth == 0
            && (!is_select_into_line[idx] || is_last_select_into_line)
            && !upper.starts_with("CREATE ")
            && !upper.starts_with("DECLARE")
            && !upper.starts_with("WHEN ")
            && !upper.starts_with("IF ")
            && !upper.starts_with("ELSIF ")
            && !upper.starts_with("ELSE")
            && !upper.starts_with("FOR ")
            && !upper.starts_with("WHILE ")
            && !upper.starts_with("LOOP")
            && !upper.starts_with("BEGIN")
            && !upper.starts_with("END")
            && !upper.starts_with("EXCEPTION")
            && !upper.starts_with("THEN")
            && (upper.starts_with("SELECT ")
                || upper.starts_with("INSERT ")
                || upper.starts_with("UPDATE ")
                || upper.starts_with("DELETE ")
                || upper.starts_with("INTO ")
                || upper.starts_with("NULL")
                || upper.starts_with("RAISE")
                || upper.contains(":NEW.")
                || upper.contains(":OLD.")
                || upper.contains(":=")
                || is_last_select_into_line);

        if needs_semicolon {
            new_line.push(';');
        }

        lines.push(new_line);
    }

    // Ensure END has semicolon
    if let Some(last) = lines.last_mut() {
        let upper = last.trim().to_uppercase();
        if upper == "END" && !last.ends_with(';') {
            last.push(';');
        }
    }

    lines.join("\n")
}

fn normalize_trigger_references(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len() + 8);
    let mut i = 0;

    while i < bytes.len() {
        if i + 4 <= bytes.len() {
            let b0 = bytes[i].to_ascii_uppercase();
            let b1 = bytes[i + 1].to_ascii_uppercase();
            let b2 = bytes[i + 2].to_ascii_uppercase();
            let b3 = bytes[i + 3];

            let is_new = b0 == b'N' && b1 == b'E' && b2 == b'W' && b3 == b'.';
            let is_old = b0 == b'O' && b1 == b'L' && b2 == b'D' && b3 == b'.';

            if is_new || is_old {
                let prev = if i == 0 { None } else { Some(bytes[i - 1]) };
                let prev_is_word = prev.map_or(false, |c| c.is_ascii_alphanumeric() || c == b'_');
                let prev_is_colon = prev == Some(b':');
                if !prev_is_word && !prev_is_colon {
                    out.push_str(if is_new { ":NEW." } else { ":OLD." });
                    i += 4;
                    continue;
                }
            }
        }

        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{generate_foreign_keys, generate_indexes, generate_triggers, TriggerTerminator};
    use crate::models::{CheckConstraint, ForeignKey, Index, TableDetails, TriggerDefinition, UniqueConstraint};

    fn base_table_details(name: &str, indexes: Vec<Index>) -> TableDetails {
        TableDetails {
            name: name.to_string(),
            comment: None,
            columns: Vec::new(),
            primary_keys: Vec::new(),
            indexes,
            unique_constraints: Vec::<UniqueConstraint>::new(),
            foreign_keys: Vec::<ForeignKey>::new(),
            check_constraints: Vec::<CheckConstraint>::new(),
            triggers: Vec::<TriggerDefinition>::new(),
        }
    }

    #[test]
    fn generate_indexes_does_not_qualify_index_name_with_schema() {
        let table = base_table_details(
            "PLATFORM_V3.QRTZ_BLOB_TRIGGERS",
            vec![Index {
                name: "INDEX33561145".to_string(),
                columns: vec![
                    "SCHED_NAME".to_string(),
                    "TRIGGER_NAME".to_string(),
                    "TRIGGER_GROUP".to_string(),
                ],
                unique: false,
            }],
        );

        let statements = generate_indexes(&table);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];
        assert!(stmt.contains("CREATE INDEX \"IDX_QRTZ_BLOB_TRIGGERS_SCHED_NAME_TRIGGER_NAME_TRIGGER_GROUP\""));
        assert!(!stmt.contains("\"PLATFORM_V3\".\"IDX_QRTZ_BLOB_TRIGGERS_SCHED_NAME_TRIGGER_NAME_TRIGGER_GROUP\""));
    }

    #[test]
    fn generate_indexes_skips_non_unique_index_on_pk_columns() {
        let mut table = base_table_details(
            "PLATFORM.QRTZ_SIMPLE_TRIGGERS",
            vec![Index {
                name: "INDEX33561156".to_string(),
                columns: vec![
                    "SCHED_NAME".to_string(),
                    "TRIGGER_NAME".to_string(),
                    "TRIGGER_GROUP".to_string(),
                ],
                unique: false,
            }],
        );
        table.primary_keys = vec![
            "SCHED_NAME".to_string(),
            "TRIGGER_NAME".to_string(),
            "TRIGGER_GROUP".to_string(),
        ];

        let statements = generate_indexes(&table);
        assert_eq!(statements.len(), 0, "Should skip index that covers same columns as PK");
    }

    #[test]
    fn generate_indexes_skips_duplicate_column_list() {
        let table = base_table_details(
            "PLATFORM_V3.DUP_INDEX",
            vec![
                Index {
                    name: "IDX_ONE".to_string(),
                    columns: vec!["A".to_string(), "B".to_string()],
                    unique: false,
                },
                Index {
                    name: "IDX_TWO".to_string(),
                    columns: vec!["A".to_string(), "B".to_string()],
                    unique: false,
                },
            ],
        );

        let statements = generate_indexes(&table);
        assert_eq!(statements.len(), 1, "Should skip duplicate index columns");
    }

    #[test]
    fn generate_indexes_skips_index_matching_unique_constraint_columns() {
        let mut table = base_table_details(
            "PLATFORM_V3.UNIQ_TEST",
            vec![Index {
                name: "IDX_UNIQ".to_string(),
                columns: vec!["CODE".to_string(), "TYPE".to_string()],
                unique: false,
            }],
        );
        table.unique_constraints = vec![UniqueConstraint {
            name: "UK_UNIQ_TEST".to_string(),
            columns: vec!["CODE".to_string(), "TYPE".to_string()],
        }];

        let statements = generate_indexes(&table);
        assert_eq!(statements.len(), 0, "Should skip index that matches unique constraint columns");
    }

    #[test]
    fn generate_foreign_keys_omits_no_action_rule() {
        let mut table = base_table_details("PLATFORM_V3.QRTZ_TRIGGERS", Vec::new());
        table.foreign_keys = vec![ForeignKey {
            name: "FK_TEST".to_string(),
            columns: vec!["SCHED_NAME".to_string()],
            referenced_table: "PLATFORM_V3.QRTZ_JOB_DETAILS".to_string(),
            referenced_columns: vec!["SCHED_NAME".to_string()],
            delete_rule: Some("NO ACTION".to_string()),
            update_rule: Some("NO ACTION".to_string()),
        }];

        let statements = generate_foreign_keys(&table);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0].to_uppercase();
        assert!(!stmt.contains("ON DELETE NO ACTION"));
        assert!(!stmt.contains("ON UPDATE NO ACTION"));
    }

    #[test]
    fn generate_triggers_uses_full_body_when_body_contains_create() {
        let body = "CREATE OR REPLACE TRIGGER TRG_BPM_CATEGORY_ID\nBEFORE INSERT ON BPM_CATEGORY\nBEGIN\nNULL;\nEND;";
        let triggers = vec![TriggerDefinition {
            name: "TRG_BPM_CATEGORY_ID".to_string(),
            table_name: "BPM_CATEGORY".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: true,
            body: body.to_string(),
        }];

        let statements = generate_triggers("PLATFORM_V3", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0].to_uppercase();
        let count = stmt.matches("CREATE OR REPLACE TRIGGER").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn normalize_trigger_body_adds_missing_semicolons() {
        let body = "CREATE OR REPLACE TRIGGER TRG_TEST\nBEFORE INSERT ON T\nFOR EACH ROW\nBEGIN\nSELECT SEQ.NEXTVAL INTO :NEW.ID FROM DUAL\n:NEW.UPDATE_TIME := SYSDATE\nEND";
        let normalized = super::normalize_trigger_body(body);
        assert!(normalized.contains("FROM DUAL;"));
        assert!(normalized.contains("SYSDATE;"));
        assert!(normalized.trim_end().ends_with(';'));
    }

    #[test]
    fn normalize_trigger_body_handles_multiline_select() {
        // This is a simplified test - the function may not handle all edge cases perfectly,
        // but it should handle the common case of SELECT...INTO with FROM clause
        let body = "BEGIN\nSELECT SEQ.NEXTVAL INTO :NEW.ID FROM DUAL\n:NEW.UPDATE_TIME := SYSDATE\nEND";
        let normalized = super::normalize_trigger_body(body);

        // Should add semicolons to complete statements
        assert!(normalized.contains("FROM DUAL;"), "Single-line SELECT...INTO...FROM should have semicolon");
        assert!(normalized.contains("SYSDATE;"), "Assignment should have semicolon");
        assert!(normalized.trim_end().ends_with(';'), "END should have semicolon");
    }

    #[test]
    fn extract_when_clause_separates_when_from_body() {
        let body = "WHEN (NEW.ID IS NULL)\nBEGIN\nSELECT SEQ.NEXTVAL INTO :NEW.ID FROM DUAL;\nEND";
        let (when_clause, body_without_when) = super::extract_when_clause(body);
        assert_eq!(when_clause, "NEW.ID IS NULL");
        assert!(body_without_when.contains("BEGIN"));
        assert!(!body_without_when.to_uppercase().contains("WHEN"));
    }

    #[test]
    fn extract_when_clause_handles_nested_parentheses() {
        let body = "WHEN (FUNC(NEW.ID, NEW.NAME) IS NULL)\nBEGIN\nNULL;\nEND";
        let (when_clause, body_without_when) = super::extract_when_clause(body);
        assert_eq!(when_clause, "FUNC(NEW.ID, NEW.NAME) IS NULL");
        assert!(body_without_when.contains("BEGIN"));
    }

    #[test]
    fn extract_when_clause_handles_multiline_when() {
        let body = "WHEN (\n  NEW.ID IS NULL\n  AND NEW.STATUS = 'ACTIVE'\n)\nBEGIN\nNULL;\nEND";
        let (when_clause, body_without_when) = super::extract_when_clause(body);
        assert!(when_clause.contains("NEW.ID IS NULL"));
        assert!(when_clause.contains("AND NEW.STATUS = 'ACTIVE'"));
        assert!(body_without_when.contains("BEGIN"));
    }

    #[test]
    fn generate_triggers_places_when_after_for_each_row() {
        let triggers = vec![TriggerDefinition {
            name: "TRG_TEST_ID".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: true,
            body: "WHEN (NEW.ID IS NULL)\nBEGIN\nSELECT SEQ.NEXTVAL INTO :NEW.ID FROM DUAL;\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];

        // Verify structure: FOR EACH ROW comes before WHEN
        let for_each_row_pos = stmt.find("FOR EACH ROW").expect("Should contain FOR EACH ROW");
        let when_pos = stmt.find("WHEN (").expect("Should contain WHEN clause");
        assert!(for_each_row_pos < when_pos, "FOR EACH ROW should come before WHEN");

        // Verify WHEN clause content
        assert!(stmt.contains("WHEN (:NEW.ID IS NULL)"));

        // Verify referencing clause
        assert!(stmt.contains("REFERENCING OLD AS OLD NEW AS NEW"));

        // Verify trigger terminator
        assert!(stmt.trim_end().ends_with(';'), "Trigger should end with ';'");
    }

    #[test]
    fn generate_triggers_handles_declare_block() {
        let triggers = vec![TriggerDefinition {
            name: "TRG_WITH_VAR".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: true,
            body: "DECLARE\n  v_count NUMBER;\nBEGIN\n  SELECT COUNT(*) INTO v_count FROM DUAL;\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];

        // Should not wrap DECLARE block with BEGIN/END
        assert!(stmt.contains("DECLARE"));
        let declare_count = stmt.matches("DECLARE").count();
        assert_eq!(declare_count, 1, "Should have exactly one DECLARE keyword, got: {}", stmt);

        // Should not have double BEGIN
        let begin_count = stmt.matches("BEGIN").count();
        assert_eq!(begin_count, 1, "Should have exactly one BEGIN keyword, got: {}", stmt);
    }

    #[test]
    fn generate_triggers_skips_when_for_statement_level_trigger() {
        let triggers = vec![TriggerDefinition {
            name: "TRG_STATEMENT_LEVEL".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "AFTER".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: false,
            body: "WHEN (1=1)\nBEGIN\nNULL;\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];

        // Should not extract WHEN for statement-level trigger
        assert!(!stmt.contains("FOR EACH ROW"));
        // WHEN should remain in body or be ignored
        // (In this case, it stays in body since we don't extract it)
    }

    #[test]
    fn generate_triggers_normalizes_new_references_in_body() {
        let triggers = vec![TriggerDefinition {
            name: "TRG_NEW_REF".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["UPDATE".to_string()],
            each_row: true,
            body: "BEGIN\nNEW.UPDATE_TIME := OLD.UPDATE_TIME\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];
        assert!(stmt.contains(":NEW.UPDATE_TIME"));
        assert!(stmt.contains(":OLD.UPDATE_TIME"));
    }

    #[test]
    fn generate_triggers_datagrip_has_no_slash_terminator() {
        let triggers = vec![TriggerDefinition {
            name: "TRG_TEST_ID".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: true,
            body: "BEGIN\n:NEW.ID := 1;\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];
        assert!(stmt.trim_end().ends_with(';'));
        assert!(!stmt.contains("\n/"));
    }

    #[test]
    fn generate_triggers_script_adds_slash_terminator() {
        let triggers = vec![TriggerDefinition {
            name: "TRG_TEST_ID".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: true,
            body: "BEGIN\n:NEW.ID := 1;\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::Script);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];
        assert!(stmt.contains("\n/"), "Expected script mode to include '/' terminator");
        assert!(stmt.trim_end().ends_with('/'));
    }

    #[test]
    fn generate_triggers_datagrip_script_uses_script_format() {
        // DataGripScript 模式下，触发器使用 Script 格式（END; + /）
        // 因为触发器会被输出到单独的文件中
        let triggers = vec![TriggerDefinition {
            name: "TRG_TEST_ID".to_string(),
            table_name: "TEST_TABLE".to_string(),
            timing: "BEFORE".to_string(),
            events: vec!["INSERT".to_string()],
            each_row: true,
            body: "BEGIN\n:NEW.ID := 1;\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGripScript);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];
        // DataGripScript 现在使用 Script 格式
        assert!(stmt.contains("\n/"), "Expected script mode to include '/' terminator");
        assert!(stmt.trim_end().ends_with('/'));
    }
}
