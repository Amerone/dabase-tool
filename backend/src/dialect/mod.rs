pub mod mysql_renderer;

use std::io::Write;

use anyhow::{anyhow, Result};

use crate::domain::canonical::{CanonicalRow, CanonicalTable, CanonicalValue, LogicalType};
use crate::models::{
    CapabilityLevel, CapabilityProfile, DbType, ExportObjectKind, ObjectCapability,
};

pub trait DialectRenderer: Send + Sync {
    fn dialect_kind(&self) -> DbType;
    fn capabilities(&self) -> CapabilityProfile;

    /// Quote a single identifier for the target dialect.
    /// Default: double quotes (SQL standard). MySQL overrides to backticks.
    fn quote_identifier(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    /// Quote a dot-separated qualified identifier (e.g. "schema.table").
    fn quote_qualified_identifier(&self, name: &str) -> String {
        name.split('.')
            .map(|part| self.quote_identifier(part))
            .collect::<Vec<_>>()
            .join(".")
    }

    fn render_table_ddl(&self, _table: &CanonicalTable) -> Result<String> {
        Err(anyhow!(
            "Dialect {:?} table DDL rendering is not implemented",
            self.dialect_kind()
        ))
    }

    fn render_insert_batch(
        &self,
        _table: &CanonicalTable,
        _rows: &[CanonicalRow],
    ) -> Result<String> {
        Err(anyhow!(
            "Dialect {:?} insert rendering is not implemented",
            self.dialect_kind()
        ))
    }

    /// Stream INSERT statements directly to a writer, avoiding intermediate String allocation.
    /// Default implementation delegates to `render_insert_batch`.
    fn write_insert_batch(
        &self,
        writer: &mut dyn Write,
        table: &CanonicalTable,
        rows: &[CanonicalRow],
    ) -> Result<()> {
        let sql = self.render_insert_batch(table, rows)?;
        writeln!(writer, "{}", sql)?;
        Ok(())
    }

    /// Maximum inline BLOB size in bytes. BLOBs exceeding this are handled via
    /// `write_lob_insert` (e.g. DBMS_LOB.APPEND blocks). Default: no limit.
    fn max_inline_blob_bytes(&self) -> usize {
        usize::MAX
    }

    /// Write a single row that contains one or more large BLOB values exceeding
    /// `max_inline_blob_bytes`. `large_blobs` maps (column_index, raw_bytes).
    /// `values` contains all column values; large-BLOB entries are placeholders.
    ///
    /// Default implementation falls back to `render_insert_batch` (ignoring the
    /// large-BLOB split), which is correct for dialects without BLOB size limits.
    fn write_lob_insert(
        &self,
        writer: &mut dyn Write,
        table: &CanonicalTable,
        values: &[CanonicalValue],
        _large_blobs: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        self.write_insert_batch(
            writer,
            table,
            &[CanonicalRow {
                values: values.to_vec(),
            }],
        )
    }
}

#[derive(Debug, Default)]
pub struct Dm8DialectRenderer;

#[derive(Debug, Default)]
pub struct KingbaseDialectRenderer;

#[derive(Debug, Default)]
pub struct ShentongDialectRenderer;

impl DialectRenderer for Dm8DialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Dm8
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "DM8 方言渲染器已在旧版导出模块中实现",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "DM8 数据渲染器已在旧版导出模块中实现",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (ExportObjectKind::Indexes, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Full,
                "",
            ),
            (ExportObjectKind::ForeignKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Full,
                "",
            ),
            (ExportObjectKind::Triggers, CapabilityLevel::Full, ""),
            (ExportObjectKind::Sequences, CapabilityLevel::Full, ""),
        ])
    }

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                dm8_quote_ident(&col.name),
                dm8_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| dm8_quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            dm8_quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        if rows.is_empty() {
            return Ok(String::new());
        }

        let columns = table
            .columns
            .iter()
            .map(|col| dm8_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        let mut statements = Vec::with_capacity(rows.len());
        for row in rows {
            let values = row
                .values
                .iter()
                .map(dm8_format_value)
                .collect::<Vec<_>>()
                .join(", ");
            statements.push(format!(
                "INSERT INTO {} ({}) VALUES ({});",
                dm8_quote_ident(&table.name),
                columns,
                values
            ));
        }

        Ok(statements.join("\n"))
    }
}

impl DialectRenderer for KingbaseDialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Kingbase
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "KingbaseES 渲染器支持完整 DDL 生成（表/列/主键/索引/约束/触发器/序列）",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "KingbaseES 渲染器支持 INSERT 批量生成",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::Full,
                "支持 CREATE INDEX",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Full,
                "支持 ALTER TABLE ADD CONSTRAINT UNIQUE",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::Full,
                "支持 ALTER TABLE ADD CONSTRAINT FOREIGN KEY",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Full,
                "支持 ALTER TABLE ADD CONSTRAINT CHECK",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::Full,
                "支持 CREATE TRIGGER (PostgreSQL 语法)",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::Full,
                "支持 CREATE SEQUENCE (PostgreSQL 语法)",
            ),
        ])
    }

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                kingbase_quote_ident(&col.name),
                kingbase_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| kingbase_quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            kingbase_quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        if rows.is_empty() {
            return Ok(String::new());
        }

        let target_ident = kingbase_quote_ident(&table.name);
        let columns = table
            .columns
            .iter()
            .map(|col| kingbase_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        if kingbase_requires_single_row_insert(table) {
            let statements = rows
                .iter()
                .map(|row| render_kingbase_single_row_insert(&target_ident, &columns, table, row))
                .collect::<Vec<_>>();
            return Ok(statements.join("\n"));
        }

        let values = rows
            .iter()
            .map(|row| {
                let literals = row
                    .values
                    .iter()
                    .enumerate()
                    .map(|(idx, v)| {
                        kingbase_format_value_for_column(v, kingbase_column_logical_type(table, idx))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", literals)
            })
            .collect::<Vec<_>>()
            .join(",\n");

        Ok(format!(
            "INSERT INTO {} ({}) VALUES\n{};",
            target_ident, columns, values
        ))
    }

    fn write_insert_batch(
        &self,
        writer: &mut dyn Write,
        table: &CanonicalTable,
        rows: &[CanonicalRow],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        if !kingbase_requires_single_row_insert(table) {
            let sql = self.render_insert_batch(table, rows)?;
            writeln!(writer, "{}", sql)?;
            return Ok(());
        }

        let target_ident = kingbase_quote_ident(&table.name);
        let columns = table
            .columns
            .iter()
            .map(|col| kingbase_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        for row in rows {
            writeln!(
                writer,
                "{}",
                render_kingbase_single_row_insert(&target_ident, &columns, table, row)
            )?;
        }

        Ok(())
    }

    fn max_inline_blob_bytes(&self) -> usize {
        KINGBASE_INLINE_BYTEA_MAX_BYTES
    }

    fn write_lob_insert(
        &self,
        writer: &mut dyn Write,
        table: &CanonicalTable,
        values: &[CanonicalValue],
        large_blobs: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        let target_ident = kingbase_quote_ident(&table.name);
        let columns = table
            .columns
            .iter()
            .map(|col| kingbase_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");
        let formatted_values = values
            .iter()
            .enumerate()
            .map(|(idx, v)| {
                kingbase_format_value_for_column(v, kingbase_column_logical_type(table, idx))
            })
            .collect::<Vec<_>>();
        let where_predicate = kingbase_primary_key_predicate(table, values);

        write_kingbase_lob_insert_block(
            writer,
            &target_ident,
            &columns,
            &formatted_values,
            large_blobs,
            where_predicate.as_deref(),
        )
    }
}

impl DialectRenderer for ShentongDialectRenderer {
    fn dialect_kind(&self) -> DbType {
        DbType::Shentong
    }

    fn capabilities(&self) -> CapabilityProfile {
        capability_profile(&[
            (
                ExportObjectKind::Ddl,
                CapabilityLevel::Full,
                "神通 OSCAR 渲染器支持完整 DDL 生成（Oracle 兼容语法）",
            ),
            (
                ExportObjectKind::Data,
                CapabilityLevel::Full,
                "神通 OSCAR 渲染器支持 INSERT 语句批量生成（多行 VALUES）",
            ),
            (ExportObjectKind::Columns, CapabilityLevel::Full, ""),
            (ExportObjectKind::PrimaryKeys, CapabilityLevel::Full, ""),
            (
                ExportObjectKind::Indexes,
                CapabilityLevel::Partial,
                "基础索引支持（通过 all_indexes）",
            ),
            (
                ExportObjectKind::UniqueConstraints,
                CapabilityLevel::Partial,
                "唯一约束基础支持",
            ),
            (
                ExportObjectKind::ForeignKeys,
                CapabilityLevel::Full,
                "外键支持（通过 all_constraints + all_cons_columns）",
            ),
            (
                ExportObjectKind::CheckConstraints,
                CapabilityLevel::Partial,
                "神通 OSCAR 检查约束支持（DM8→Shentong 路径透传）",
            ),
            (
                ExportObjectKind::Triggers,
                CapabilityLevel::Partial,
                "神通 OSCAR 触发器支持（DM8→Shentong 路径透传，需注意 NEXTVAL 语法差异）",
            ),
            (
                ExportObjectKind::Sequences,
                CapabilityLevel::Full,
                "神通 OSCAR 序列支持（Oracle 兼容 CREATE SEQUENCE 语法）",
            ),
        ])
    }

    fn render_table_ddl(&self, table: &CanonicalTable) -> Result<String> {
        let mut lines = Vec::new();
        for col in &table.columns {
            let nullability = if col.nullable { "" } else { " NOT NULL" };
            lines.push(format!(
                "  {} {}{}",
                shentong_quote_ident(&col.name),
                shentong_type(&col.logical_type),
                nullability
            ));
        }

        if !table.primary_keys.is_empty() {
            let pk_cols = table
                .primary_keys
                .iter()
                .map(|name| shentong_quote_ident(name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  PRIMARY KEY ({})", pk_cols));
        }

        Ok(format!(
            "CREATE TABLE {} (\n{}\n);",
            shentong_quote_ident(&table.name),
            lines.join(",\n")
        ))
    }

    fn render_insert_batch(&self, table: &CanonicalTable, rows: &[CanonicalRow]) -> Result<String> {
        if rows.is_empty() {
            return Ok(String::new());
        }

        let columns = table
            .columns
            .iter()
            .map(|col| shentong_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");

        // ShenTong limitation: multi-row INSERT with explicit identity/auto-increment
        // values fails with "自增列插入多行时，只有第一行支持指定自增列的值".
        // Fall back to single-row INSERTs when any column is an identity column.
        let has_identity = table.columns.iter().any(|col| col.identity);

        if has_identity {
            let mut statements = Vec::with_capacity(rows.len());
            for row in rows {
                let literals = row
                    .values
                    .iter()
                    .map(shentong_format_value)
                    .collect::<Vec<_>>()
                    .join(", ");
                statements.push(format!(
                    "INSERT INTO {} ({}) VALUES ({});",
                    shentong_quote_ident(&table.name),
                    columns,
                    literals
                ));
            }
            return Ok(statements.join("\n"));
        }

        // Shentong supports multi-row VALUES (max 10000 rows per INSERT).
        // Split into multiple INSERT statements if batch exceeds the limit.
        const SHENTONG_MAX_MULTI_ROW: usize = 10000;
        let mut statements = Vec::new();

        for chunk in rows.chunks(SHENTONG_MAX_MULTI_ROW) {
            let values = chunk
                .iter()
                .map(|row| {
                    let literals = row
                        .values
                        .iter()
                        .map(shentong_format_value)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("({})", literals)
                })
                .collect::<Vec<_>>()
                .join(",\n");

            statements.push(format!(
                "INSERT INTO {} ({}) VALUES\n{};",
                shentong_quote_ident(&table.name),
                columns,
                values
            ));
        }

        Ok(statements.join("\n"))
    }

    fn max_inline_blob_bytes(&self) -> usize {
        SHENTONG_INLINE_BLOB_MAX_BYTES
    }

    fn write_lob_insert(
        &self,
        writer: &mut dyn Write,
        table: &CanonicalTable,
        values: &[CanonicalValue],
        large_blobs: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        let target_ident = shentong_quote_ident(&table.name);
        let columns_str = table
            .columns
            .iter()
            .map(|col| shentong_quote_ident(&col.name))
            .collect::<Vec<_>>()
            .join(", ");
        let formatted_values: Vec<String> = values.iter().map(shentong_format_value).collect();
        let where_predicate = shentong_primary_key_predicate(table, values);
        write_shentong_lob_insert_block(
            writer,
            &target_ident,
            &columns_str,
            &formatted_values,
            large_blobs,
            where_predicate.as_deref(),
        )
    }
}

pub fn renderer_for(db_type: &DbType) -> Box<dyn DialectRenderer> {
    match db_type {
        DbType::Dm8 => Box::new(Dm8DialectRenderer),
        DbType::Mysql => Box::new(mysql_renderer::MySqlDialectRenderer),
        DbType::Kingbase => Box::new(KingbaseDialectRenderer),
        DbType::Shentong => Box::new(ShentongDialectRenderer),
    }
}

pub fn shentong_quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn shentong_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "INTEGER",
        LogicalType::Decimal => "DECIMAL(38,10)",
        LogicalType::Float => "DOUBLE PRECISION",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "CLOB",
        LogicalType::Binary => "BLOB",
        LogicalType::Boolean => "BOOLEAN",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "TIMESTAMP",
        LogicalType::Json => "JSON",
        LogicalType::Unknown => "CLOB",
    }
}

pub fn shentong_format_value(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Null => "NULL".to_string(),
        CanonicalValue::Integer(v) => v.to_string(),
        CanonicalValue::Decimal(v) => v.clone(),
        CanonicalValue::Float(v) if v.is_finite() => v.to_string(),
        CanonicalValue::Float(_) => "NULL".to_string(),
        CanonicalValue::Boolean(v) => {
            if *v {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CanonicalValue::String(v)
        | CanonicalValue::Date(v)
        | CanonicalValue::DateTime(v)
        | CanonicalValue::Json(v) => format!("'{}'", v.replace('\'', "''")),
        CanonicalValue::Binary(v) => {
            if v.is_empty() {
                "NULL".to_string()
            } else {
                // ShenTong: TO_BLOB(HEXTORAW('hex')) — confirmed working by manual test.
                let hex = bytes_to_hex_upper(v);
                format!("TO_BLOB(HEXTORAW('{}'))", hex)
            }
        }
    }
}

/// Maximum bytes for inline TO_BLOB(HEXTORAW('...')) binary literals in Shentong INSERT statements.
/// Standard SQL hex literals have DB-side SQL length limits (a few MB).
/// Use conservative 16383 bytes to stay safe; beyond this use chunked UPDATE concatenation.
pub const SHENTONG_INLINE_BLOB_MAX_BYTES: usize = 16383;

/// Maximum bytes per TO_BLOB(HEXTORAW('...')) chunk for Shentong UPDATE concatenation.
/// Each chunk should stay within SQL parser limits for HEXTORAW().
const SHENTONG_LOB_CHUNK_BYTES: usize = 16000;

fn shentong_primary_key_predicate(
    table: &CanonicalTable,
    values: &[CanonicalValue],
) -> Option<String> {
    if table.primary_keys.is_empty() {
        return None;
    }

    let mut predicates = Vec::with_capacity(table.primary_keys.len());
    for pk in &table.primary_keys {
        let column_index = table
            .columns
            .iter()
            .position(|col| col.name.eq_ignore_ascii_case(pk))?;
        let value = values.get(column_index)?;
        if matches!(value, CanonicalValue::Null) {
            return None;
        }
        predicates.push(format!(
            "{} = {}",
            shentong_quote_ident(pk),
            shentong_format_value(value)
        ));
    }

    Some(predicates.join(" AND "))
}

/// Write INSERT + UPDATE statements for a row with large BLOB values in ShenTong OSCAR.
///
/// Strategy: INSERT the row with the first chunk of each BLOB inline, then append
/// remaining chunks via plain SQL `UPDATE SET col = col || TO_BLOB(HEXTORAW('...'))`.
///
/// ShenTong's PL/SQL (PLOSCAR) does NOT support `BLOB` as a variable type in
/// DECLARE blocks (error: "syntax error at or near BLOB"), so we avoid PL/SQL
/// entirely and use plain SQL UPDATE concatenation instead.
///
/// `large_blobs` maps column index → raw bytes for oversized BLOB columns.
/// `column_values` contains formatted SQL literals for all columns (large BLOBs
/// should be `"NULL"` placeholders that get replaced).
pub fn write_shentong_lob_insert_block(
    writer: &mut (impl std::io::Write + ?Sized),
    target_table_ident: &str,
    columns_str: &str,
    column_values: &[String],
    large_blobs: &[(usize, Vec<u8>)],
    where_predicate: Option<&str>,
) -> Result<()> {
    let col_names: Vec<&str> = columns_str.split(", ").collect();

    // Step 1: INSERT with the first chunk of each large BLOB inline.
    let mut final_values = column_values.to_vec();
    for (col_idx, bytes) in large_blobs.iter() {
        let first_chunk = &bytes[..bytes.len().min(SHENTONG_LOB_CHUNK_BYTES)];
        let hex = bytes_to_hex_upper(first_chunk);
        final_values[*col_idx] = format!("TO_BLOB(HEXTORAW('{}'))", hex);
    }
    writeln!(
        writer,
        "INSERT INTO {} ({}) VALUES ({});",
        target_table_ident,
        columns_str,
        final_values.join(", ")
    )?;

    // Step 2: For each large BLOB with remaining data, append chunks via UPDATE.
    let has_remaining_chunks = large_blobs
        .iter()
        .any(|(_, bytes)| bytes.len() > SHENTONG_LOB_CHUNK_BYTES);
    let where_predicate = if has_remaining_chunks {
        Some(where_predicate.ok_or_else(|| {
            anyhow::anyhow!(
                "Shentong large BLOB export requires a non-null primary key on table {}",
                target_table_ident
            )
        })?)
    } else {
        None
    };

    for (col_idx, bytes) in large_blobs.iter() {
        if bytes.len() <= SHENTONG_LOB_CHUNK_BYTES {
            continue; // fully inserted in step 1
        }
        let blob_col = col_names.get(*col_idx).unwrap_or(&"\"data\"");
        let where_predicate = where_predicate.expect("checked when remaining chunks exist");
        for chunk in bytes[SHENTONG_LOB_CHUNK_BYTES..].chunks(SHENTONG_LOB_CHUNK_BYTES) {
            let hex = bytes_to_hex_upper(chunk);
            writeln!(
                writer,
                "UPDATE {} SET {} = {} || TO_BLOB(HEXTORAW('{}')) WHERE {};",
                target_table_ident, blob_col, blob_col, hex, where_predicate
            )?;
        }
    }

    Ok(())
}

fn kingbase_quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn kingbase_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "BIGINT",
        LogicalType::Decimal => "NUMERIC(38,10)",
        LogicalType::Float => "DOUBLE PRECISION",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "TEXT",
        LogicalType::Binary => "BYTEA",
        LogicalType::Boolean => "BOOLEAN",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "TIMESTAMP",
        LogicalType::Json => "JSONB",
        LogicalType::Unknown => "TEXT",
    }
}

fn kingbase_requires_single_row_insert(table: &CanonicalTable) -> bool {
    table.columns.iter().any(|col| {
        matches!(
            col.logical_type,
            LogicalType::Binary | LogicalType::Text | LogicalType::Json
        )
    })
}

fn render_kingbase_single_row_insert(
    target_table_ident: &str,
    columns: &str,
    table: &CanonicalTable,
    row: &CanonicalRow,
) -> String {
    let literals = row
        .values
        .iter()
        .enumerate()
        .map(|(idx, v)| {
            kingbase_format_value_for_column(v, kingbase_column_logical_type(table, idx))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {} ({}) VALUES ({});",
        target_table_ident, columns, literals
    )
}

fn kingbase_column_logical_type(table: &CanonicalTable, idx: usize) -> Option<&LogicalType> {
    table.columns.get(idx).map(|col| &col.logical_type)
}

/// Format a value for KingBase, taking the target column's logical type into account.
///
/// The only context-sensitive case is `Boolean`: when the target column is itself
/// a Boolean we emit `TRUE`/`FALSE`, but when it is a numeric type (the typical
/// DM8→KingBase mapping for `BIT` is `SMALLINT`) we emit `1`/`0` so KingBase's
/// strict type system does not reject the literal.
fn kingbase_format_value_for_column(
    value: &CanonicalValue,
    target_type: Option<&LogicalType>,
) -> String {
    if let CanonicalValue::Boolean(b) = value {
        let want_numeric = matches!(
            target_type,
            Some(LogicalType::Integer) | Some(LogicalType::Decimal) | Some(LogicalType::Float)
        );
        if want_numeric {
            return if *b { "1".to_string() } else { "0".to_string() };
        }
    }
    kingbase_format_value(value)
}

fn kingbase_format_value(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Null => "NULL".to_string(),
        CanonicalValue::Integer(v) => v.to_string(),
        CanonicalValue::Decimal(v) => v.clone(),
        CanonicalValue::Float(v) if v.is_finite() => v.to_string(),
        CanonicalValue::Float(_) => "NULL".to_string(),
        CanonicalValue::Boolean(v) => {
            if *v {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CanonicalValue::String(v)
        | CanonicalValue::Date(v)
        | CanonicalValue::DateTime(v)
        | CanonicalValue::Json(v) => format!("'{}'", v.replace('\'', "''")),
        CanonicalValue::Binary(v) => {
            let hex = bytes_to_hex_lower(v);
            format!("decoding('{}', 'hex')", hex)
        }
    }
}

pub const KINGBASE_INLINE_BYTEA_MAX_BYTES: usize = 512 * 1024;

const KINGBASE_BYTEA_CHUNK_BYTES: usize = KINGBASE_INLINE_BYTEA_MAX_BYTES;

fn kingbase_primary_key_predicate(
    table: &CanonicalTable,
    values: &[CanonicalValue],
) -> Option<String> {
    if table.primary_keys.is_empty() {
        return None;
    }

    let mut predicates = Vec::with_capacity(table.primary_keys.len());
    for pk in &table.primary_keys {
        let column_index = table
            .columns
            .iter()
            .position(|col| col.name.eq_ignore_ascii_case(pk))?;
        let value = values.get(column_index)?;
        if matches!(value, CanonicalValue::Null) {
            return None;
        }
        predicates.push(format!(
            "{} = {}",
            kingbase_quote_ident(pk),
            kingbase_format_value_for_column(value, Some(&table.columns[column_index].logical_type))
        ));
    }

    Some(predicates.join(" AND "))
}

fn write_kingbase_lob_insert_block(
    writer: &mut (impl Write + ?Sized),
    target_table_ident: &str,
    columns_str: &str,
    column_values: &[String],
    large_blobs: &[(usize, Vec<u8>)],
    where_predicate: Option<&str>,
) -> Result<()> {
    let col_names: Vec<&str> = columns_str.split(", ").collect();
    let mut final_values = column_values.to_vec();

    for (col_idx, bytes) in large_blobs {
        let first_chunk = &bytes[..bytes.len().min(KINGBASE_BYTEA_CHUNK_BYTES)];
        let hex = bytes_to_hex_lower(first_chunk);
        final_values[*col_idx] = format!("decoding('{}', 'hex')", hex);
    }

    writeln!(
        writer,
        "INSERT INTO {} ({}) VALUES ({});",
        target_table_ident,
        columns_str,
        final_values.join(", ")
    )?;

    let has_remaining_chunks = large_blobs
        .iter()
        .any(|(_, bytes)| bytes.len() > KINGBASE_BYTEA_CHUNK_BYTES);
    if !has_remaining_chunks {
        return Ok(());
    }

    let where_predicate = where_predicate.ok_or_else(|| {
        anyhow!(
            "Kingbase large BYTEA export requires a primary key on table {}",
            target_table_ident
        )
    })?;

    for (col_idx, bytes) in large_blobs {
        if bytes.len() <= KINGBASE_BYTEA_CHUNK_BYTES {
            continue;
        }

        let blob_col = col_names.get(*col_idx).unwrap_or(&"\"data\"");
        for chunk in bytes[KINGBASE_BYTEA_CHUNK_BYTES..].chunks(KINGBASE_BYTEA_CHUNK_BYTES) {
            let hex = bytes_to_hex_lower(chunk);
            writeln!(
                writer,
                "UPDATE {} SET {} = {} || decoding('{}', 'hex') WHERE {};",
                target_table_ident, blob_col, blob_col, hex, where_predicate
            )?;
        }
    }

    Ok(())
}

fn capability_profile(defs: &[(ExportObjectKind, CapabilityLevel, &str)]) -> CapabilityProfile {
    CapabilityProfile {
        items: defs
            .iter()
            .map(|(object, level, note)| ObjectCapability {
                object: *object,
                level: *level,
                note: non_empty_note(note),
                reason_code: reason_code_for_level(*level),
            })
            .collect(),
    }
}

fn reason_code_for_level(level: CapabilityLevel) -> Option<String> {
    match level {
        CapabilityLevel::Full => None,
        CapabilityLevel::Partial => Some("partial_support".to_string()),
        CapabilityLevel::None => Some("not_supported".to_string()),
    }
}

fn dm8_quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn dm8_type(logical: &LogicalType) -> &'static str {
    match logical {
        LogicalType::Integer => "BIGINT",
        LogicalType::Decimal => "DECIMAL(38,10)",
        LogicalType::Float => "DOUBLE",
        LogicalType::String => "VARCHAR(255)",
        LogicalType::Text => "CLOB",
        LogicalType::Binary => "BLOB",
        LogicalType::Boolean => "NUMBER(1)",
        LogicalType::Date => "DATE",
        LogicalType::DateTime => "TIMESTAMP",
        LogicalType::Json => "CLOB",
        LogicalType::Unknown => "CLOB",
    }
}

fn dm8_format_value(value: &CanonicalValue) -> String {
    match value {
        CanonicalValue::Null => "NULL".to_string(),
        CanonicalValue::Integer(v) => v.to_string(),
        CanonicalValue::Decimal(v) => v.clone(),
        CanonicalValue::Float(v) if v.is_finite() => v.to_string(),
        CanonicalValue::Float(_) => "NULL".to_string(),
        CanonicalValue::Boolean(v) => {
            if *v {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        CanonicalValue::String(v)
        | CanonicalValue::Date(v)
        | CanonicalValue::DateTime(v)
        | CanonicalValue::Json(v) => format!("'{}'", v.replace('\'', "''")),
        CanonicalValue::Binary(v) => {
            let hex = bytes_to_hex_upper(v);
            format!("HEXTORAW('{}')", hex)
        }
    }
}

fn non_empty_note(note: &str) -> Option<String> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Lookup table for fast byte-to-hex conversion.
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Convert bytes to uppercase hex string with pre-allocated capacity.
/// Much faster than per-byte `format!("{:02X}", b)`.
pub fn bytes_to_hex_upper(bytes: &[u8]) -> String {
    let mut hex = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        hex.push(HEX_UPPER[(b >> 4) as usize]);
        hex.push(HEX_UPPER[(b & 0x0F) as usize]);
    }
    // SAFETY: all bytes are ASCII hex digits
    unsafe { String::from_utf8_unchecked(hex) }
}

/// Convert bytes to lowercase hex string with pre-allocated capacity.
fn bytes_to_hex_lower(bytes: &[u8]) -> String {
    let mut hex = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        hex.push(HEX_LOWER[(b >> 4) as usize]);
        hex.push(HEX_LOWER[(b & 0x0F) as usize]);
    }
    unsafe { String::from_utf8_unchecked(hex) }
}

#[cfg(test)]
mod tests {
    use super::{
        shentong_format_value, write_shentong_lob_insert_block, DialectRenderer,
        Dm8DialectRenderer, KingbaseDialectRenderer, ShentongDialectRenderer,
        KINGBASE_INLINE_BYTEA_MAX_BYTES,
    };
    use crate::domain::canonical::{
        CanonicalColumn, CanonicalRow, CanonicalTable, CanonicalValue, LogicalType,
    };

    #[test]
    fn dm8_renderer_quotes_schema_qualified_name() {
        let renderer = Dm8DialectRenderer;
        let table = CanonicalTable {
            name: "APP.USERS".to_string(),
            columns: vec![CanonicalColumn {
                name: "ID".to_string(),
                logical_type: LogicalType::Integer,
                nullable: false,
                identity: false,
            }],
            primary_keys: vec!["ID".to_string()],
        };

        let ddl = renderer
            .render_table_ddl(&table)
            .expect("schema-qualified DM8 table should render");
        assert!(ddl.contains("CREATE TABLE \"APP\".\"USERS\""));
    }

    #[test]
    fn shentong_format_value_binary_uses_hex_literal() {
        let val = CanonicalValue::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(shentong_format_value(&val), "TO_BLOB(HEXTORAW('DEADBEEF'))");
    }

    #[test]
    fn shentong_format_value_empty_binary_is_null() {
        let val = CanonicalValue::Binary(vec![]);
        assert_eq!(shentong_format_value(&val), "NULL");
    }

    #[test]
    fn kingbase_boolean_renders_numeric_for_integer_column() {
        let renderer = KingbaseDialectRenderer;
        let table = CanonicalTable {
            name: "t".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::Integer,
                    nullable: false,
                    identity: false,
                },
                // DM8 BIT → KingBase SMALLINT (mapped via dm8_to_kingbase). The
                // canonical type stays Boolean because the source column was BIT,
                // but the DDL emitted SMALLINT, so the literal must be 0/1.
                CanonicalColumn {
                    name: "deleted".to_string(),
                    logical_type: LogicalType::Integer,
                    nullable: false,
                    identity: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        };
        let row = CanonicalRow {
            values: vec![CanonicalValue::Integer(1), CanonicalValue::Boolean(false)],
        };

        let sql = renderer.render_insert_batch(&table, &[row]).unwrap();
        assert!(sql.contains(", 0)"), "expected boolean→0, got: {}", sql);
        assert!(!sql.contains("FALSE"), "must not emit FALSE literal: {}", sql);
    }

    #[test]
    fn kingbase_boolean_keeps_true_false_for_boolean_column() {
        let renderer = KingbaseDialectRenderer;
        let table = CanonicalTable {
            name: "t".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::Integer,
                    nullable: false,
                    identity: false,
                },
                CanonicalColumn {
                    name: "active".to_string(),
                    logical_type: LogicalType::Boolean,
                    nullable: false,
                    identity: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        };
        let row = CanonicalRow {
            values: vec![CanonicalValue::Integer(1), CanonicalValue::Boolean(true)],
        };

        let sql = renderer.render_insert_batch(&table, &[row]).unwrap();
        assert!(sql.contains("TRUE"), "expected boolean→TRUE, got: {}", sql);
    }

    #[test]
    fn kingbase_format_value_binary_uses_decoding() {
        let renderer = KingbaseDialectRenderer;
        let table = CanonicalTable {
            name: "t".to_string(),
            columns: vec![CanonicalColumn {
                name: "data".to_string(),
                logical_type: LogicalType::Binary,
                nullable: true,
                identity: false,
            }],
            primary_keys: vec![],
        };
        let row = CanonicalRow {
            values: vec![CanonicalValue::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF])],
        };

        let sql = renderer.render_insert_batch(&table, &[row]).unwrap();
        assert!(sql.contains("decoding('deadbeef', 'hex')"));
        assert!(!sql.contains("::BYTEA"));
    }

    #[test]
    fn kingbase_writer_streams_lob_rows_as_single_row_inserts() {
        let renderer = KingbaseDialectRenderer;
        let table = CanonicalTable {
            name: "t".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::String,
                    nullable: false,
                    identity: false,
                },
                CanonicalColumn {
                    name: "data".to_string(),
                    logical_type: LogicalType::Binary,
                    nullable: true,
                    identity: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        };
        let rows = vec![
            CanonicalRow {
                values: vec![
                    CanonicalValue::String("a".into()),
                    CanonicalValue::Binary(vec![0x01]),
                ],
            },
            CanonicalRow {
                values: vec![
                    CanonicalValue::String("b".into()),
                    CanonicalValue::Binary(vec![0x02]),
                ],
            },
        ];

        let mut buf = Vec::new();
        renderer
            .write_insert_batch(&mut buf, &table, &rows)
            .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        assert_eq!(rendered.matches("INSERT INTO").count(), 2);
        assert!(!rendered.contains("),\n("));
    }

    #[test]
    fn kingbase_lob_insert_large_blob_uses_update_concat() {
        let renderer = KingbaseDialectRenderer;
        let table = CanonicalTable {
            name: "s.t".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::String,
                    nullable: false,
                    identity: false,
                },
                CanonicalColumn {
                    name: "data".to_string(),
                    logical_type: LogicalType::Binary,
                    nullable: true,
                    identity: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        };
        let values = vec![CanonicalValue::String("pk1".into()), CanonicalValue::Null];
        let large_blobs = vec![(1usize, vec![0xAB; KINGBASE_INLINE_BYTEA_MAX_BYTES + 32])];

        let mut buf = Vec::new();
        renderer
            .write_lob_insert(&mut buf, &table, &values, &large_blobs)
            .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        assert!(rendered.contains("INSERT INTO \"s\".\"t\""));
        assert!(rendered.contains("decoding('"));
        assert!(rendered.contains("'hex')"));
        assert!(rendered.contains("UPDATE \"s\".\"t\" SET \"data\" = \"data\" || decoding('"));
        assert!(rendered.contains("WHERE \"id\" = 'pk1'"));
    }

    #[test]
    fn shentong_format_value_boolean_renders_true_false() {
        assert_eq!(
            shentong_format_value(&CanonicalValue::Boolean(true)),
            "TRUE"
        );
        assert_eq!(
            shentong_format_value(&CanonicalValue::Boolean(false)),
            "FALSE"
        );
    }

    #[test]
    fn shentong_renderer_splits_large_batch_at_10000() {
        let renderer = ShentongDialectRenderer;
        let table = CanonicalTable {
            name: "T".to_string(),
            columns: vec![CanonicalColumn {
                name: "ID".to_string(),
                logical_type: LogicalType::Integer,
                nullable: false,
                identity: false,
            }],
            primary_keys: vec![],
        };

        // Create 10001 rows
        let rows: Vec<CanonicalRow> = (0..10001)
            .map(|i| CanonicalRow {
                values: vec![CanonicalValue::Integer(i)],
            })
            .collect();

        let sql = renderer
            .render_insert_batch(&table, &rows)
            .expect("large batch should render");
        // Should have exactly 2 INSERT statements
        let insert_count = sql.matches("INSERT INTO").count();
        assert_eq!(insert_count, 2, "should split into 2 INSERT statements");
    }

    #[test]
    fn shentong_lob_insert_block_uses_dbms_lob() {
        let mut buf = Vec::new();
        let values = vec!["'pk1'".to_string(), "NULL".to_string()];
        let large_blobs = vec![(1usize, vec![0xAB; 100])];

        write_shentong_lob_insert_block(
            &mut buf,
            "\"S\".\"T\"",
            "\"ID\", \"DATA\"",
            &values,
            &large_blobs,
            None,
        )
        .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        // Step 1: INSERT with EMPTY_BLOB()
        assert!(rendered.contains("INSERT INTO \"S\".\"T\""));
        // Step 1: INSERT with first chunk inline (not EMPTY_BLOB)
        assert!(rendered.contains("TO_BLOB(HEXTORAW('"));
        assert!(
            !rendered.contains("EMPTY_BLOB()"),
            "should NOT use EMPTY_BLOB — first chunk is inlined"
        );
        // Step 2: plain SQL UPDATE concatenation (no PL/SQL DECLARE block)
        assert!(
            !rendered.contains("DECLARE"),
            "must NOT use PL/SQL DECLARE block"
        );
        assert!(
            !rendered.contains("DBMS_LOB"),
            "must NOT use DBMS_LOB (not supported in ShenTong anonymous blocks)"
        );
        // No PL/SQL block terminator needed
        assert!(
            !rendered.contains("END;\n/\n"),
            "must NOT have PL/SQL block terminator"
        );
    }

    #[test]
    fn shentong_lob_insert_large_blob_uses_update_concat() {
        let mut buf = Vec::new();
        let values = vec!["'pk1'".to_string(), "NULL".to_string()];
        // 40000 bytes = first 16000 chunk + 16000 chunk + 8000 chunk
        let large_blobs = vec![(1usize, vec![0xAB; 40000])];

        write_shentong_lob_insert_block(
            &mut buf,
            "\"S\".\"T\"",
            "\"ID\", \"DATA\"",
            &values,
            &large_blobs,
            Some("\"ID\" = 'pk1'"),
        )
        .unwrap();

        let rendered = String::from_utf8(buf).unwrap();
        // INSERT has first chunk inline
        assert!(rendered.contains("INSERT INTO \"S\".\"T\""));
        assert!(rendered.contains("TO_BLOB(HEXTORAW('"));
        // Remaining chunks appended via UPDATE concatenation
        let update_count = rendered.matches("UPDATE \"S\".\"T\"").count();
        assert_eq!(
            update_count, 2,
            "should have 2 UPDATE statements for remaining 24000 bytes"
        );
        assert!(rendered.contains("SET \"DATA\" = \"DATA\" || TO_BLOB(HEXTORAW('"));
        assert!(rendered.contains("WHERE \"ID\" = 'pk1'"));
    }

    #[test]
    fn shentong_lob_insert_large_blob_requires_primary_key_predicate() {
        let mut buf = Vec::new();
        let values = vec!["'pk1'".to_string(), "NULL".to_string()];
        let large_blobs = vec![(1usize, vec![0xAB; 40000])];

        let err = write_shentong_lob_insert_block(
            &mut buf,
            "\"S\".\"T\"",
            "\"ID\", \"DATA\"",
            &values,
            &large_blobs,
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("requires a non-null primary key"));
    }

    #[test]
    fn shentong_renderer_uses_single_row_insert_for_identity_table() {
        let renderer = ShentongDialectRenderer;
        let table = CanonicalTable {
            name: "S.infra_codegen_column".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::Integer,
                    nullable: false,
                    identity: true, // AUTO_INCREMENT column
                },
                CanonicalColumn {
                    name: "name".to_string(),
                    logical_type: LogicalType::String,
                    nullable: true,
                    identity: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        };

        let rows = vec![
            CanonicalRow {
                values: vec![
                    CanonicalValue::Integer(1),
                    CanonicalValue::String("a".into()),
                ],
            },
            CanonicalRow {
                values: vec![
                    CanonicalValue::Integer(2),
                    CanonicalValue::String("b".into()),
                ],
            },
            CanonicalRow {
                values: vec![
                    CanonicalValue::Integer(3),
                    CanonicalValue::String("c".into()),
                ],
            },
        ];

        let sql = renderer
            .render_insert_batch(&table, &rows)
            .expect("identity table should render single-row INSERTs");

        // Must produce 3 separate INSERT statements, not one multi-row INSERT
        let insert_count = sql.matches("INSERT INTO").count();
        assert_eq!(
            insert_count, 3,
            "identity table must use single-row INSERTs"
        );

        // Must NOT contain multi-row VALUES separator
        assert!(
            !sql.contains("),\n("),
            "must not use multi-row VALUES syntax"
        );

        // Each INSERT should be self-contained with VALUES (...)
        assert!(sql.contains("VALUES (1, 'a');"));
        assert!(sql.contains("VALUES (2, 'b');"));
        assert!(sql.contains("VALUES (3, 'c');"));
    }

    #[test]
    fn shentong_renderer_uses_multi_row_insert_for_non_identity_table() {
        let renderer = ShentongDialectRenderer;
        let table = CanonicalTable {
            name: "T".to_string(),
            columns: vec![
                CanonicalColumn {
                    name: "id".to_string(),
                    logical_type: LogicalType::Integer,
                    nullable: false,
                    identity: false,
                },
                CanonicalColumn {
                    name: "name".to_string(),
                    logical_type: LogicalType::String,
                    nullable: true,
                    identity: false,
                },
            ],
            primary_keys: vec!["id".to_string()],
        };

        let rows = vec![
            CanonicalRow {
                values: vec![
                    CanonicalValue::Integer(1),
                    CanonicalValue::String("a".into()),
                ],
            },
            CanonicalRow {
                values: vec![
                    CanonicalValue::Integer(2),
                    CanonicalValue::String("b".into()),
                ],
            },
        ];

        let sql = renderer
            .render_insert_batch(&table, &rows)
            .expect("non-identity table should render multi-row INSERT");

        // Should produce ONE multi-row INSERT
        let insert_count = sql.matches("INSERT INTO").count();
        assert_eq!(
            insert_count, 1,
            "non-identity table should use multi-row INSERT"
        );

        // Should contain multi-row VALUES separator
        assert!(sql.contains("),\n("), "should use multi-row VALUES syntax");
    }
}
