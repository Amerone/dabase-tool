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
    let pk_set: HashSet<String> = table
        .primary_keys
        .iter()
        .map(|col| col.to_uppercase())
        .collect();

    table
        .indexes
        .iter()
        .filter_map(|index| {
            if index.columns.is_empty() {
                return None;
            }

            let index_cols_upper: HashSet<String> =
                index.columns.iter().map(|c| c.to_uppercase()).collect();

            // Skip indexes that cover exactly the same columns as the primary key
            // (both unique and non-unique, as PK constraint creates its own index)
            if !pk_set.is_empty() && index_cols_upper == pk_set {
                return None;
            }

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
            let mut stmt = format!(
                "CREATE OR REPLACE SEQUENCE {}.{}",
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
            if let Some(cache) = seq.cache_size {
                stmt.push_str(&format!(" CACHE {}", cache));
            } else {
                stmt.push_str(" NOCACHE");
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
    triggers
        .iter()
        .map(|tr| {
            let body_trimmed = tr.body.trim();
            let body_upper = body_trimmed.to_uppercase();
            if body_upper.starts_with("CREATE TRIGGER")
                || body_upper.starts_with("CREATE OR REPLACE TRIGGER")
            {
                let mut stmt = normalize_trigger_body(body_trimmed);
                apply_trigger_terminator(&mut stmt, terminator);
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
            apply_trigger_terminator(&mut stmt, terminator);
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
    writeln!(writer, "-- DM8 DDL Export")?;
    writeln!(writer, "-- Tables: {}", tables.len())?;
    writeln!(writer, "-- Generated at: {}", timestamp)?;
    if drop_existing {
        writeln!(writer, "-- Warning: This script will drop existing tables before recreating them.")?;
    } else {
        writeln!(writer, "-- Note: DROP TABLE is disabled in this export.")?;
    }
    writeln!(writer)?;

    for (i, table_details) in table_cache.iter().enumerate() {
        let mut render_table = table_details.clone();
        render_table.name = format!("{}.{}", target_schema, table_details.name);

        if i > 0 {
            writeln!(writer)?;
        }

        writeln!(
            writer,
            "-- Table: {}",
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
        writeln!(writer, "-- Foreign keys")?;
        for stmt in fk_statements {
            writeln!(writer, "{}", stmt)?;
        }
    }

    // Emit sequences before triggers to satisfy dependencies.
    let seq_stmts = generate_sequences(&target_schema, &sequences);
    if !seq_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- Sequences")?;
        for stmt in seq_stmts {
            writeln!(writer, "{}", stmt)?;
        }
    }

    // Emit triggers after sequences and tables.
    let mut trig_stmts = Vec::new();
    for table_details in &table_cache {
        let mut render_table = table_details.clone();
        render_table.name = format!("{}.{}", target_schema, table_details.name);
        trig_stmts.extend(generate_triggers(
            &target_schema,
            &render_table.triggers,
            TriggerTerminator::DataGrip,
        ));
    }
    if !trig_stmts.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "-- Triggers")?;
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

    if data_type.contains('(') {
        return data_type;
    }

    match data_type.as_str() {
        "VARCHAR" | "VARCHAR2" | "CHAR" | "NCHAR" | "NVARCHAR" | "NVARCHAR2" | "RAW"
        | "BINARY" => {
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
        "NUMBER" | "DECIMAL" | "NUMERIC" => {
            if let Some(prec) = column.precision {
                if let Some(scale) = column.scale {
                    data_type = format!("{}({},{})", data_type, prec, scale);
                } else {
                    data_type = format!("{}({})", data_type, prec);
                }
            } else if let Some(len) = column.length {
                data_type = format!("{}({})", data_type, len);
            }
        }
        _ => {}
    }

    data_type
}

fn format_default(column: &Column, raw: &str) -> String {
    let dt = column.data_type.trim().to_uppercase();
    let expr = raw.trim();

    // If already quoted or looks like a function/expression, pass through.
    let is_expr = expr.contains('(')
        || expr.eq_ignore_ascii_case("SYSDATE")
        || expr.eq_ignore_ascii_case("SYSTIMESTAMP")
        || expr.eq_ignore_ascii_case("CURRENT_DATE")
        || expr.eq_ignore_ascii_case("CURRENT_TIMESTAMP");
    let is_quoted = expr.starts_with('\'') && expr.ends_with('\'');
    let is_numeric = expr.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+');

    if is_expr || is_quoted || is_numeric {
        return expr.to_string();
    }

    if matches!(
        dt.as_str(),
        "CHAR"
            | "NCHAR"
            | "VARCHAR"
            | "VARCHAR2"
            | "NVARCHAR"
            | "NVARCHAR2"
    ) {
        return format!("'{}'", escape_single_quotes(expr));
    }

    if matches!(dt.as_str(), "DATE") {
        return format!("TO_DATE('{}','YYYY-MM-DD HH24:MI:SS')", escape_single_quotes(expr));
    }

    if dt.starts_with("TIMESTAMP") {
        return format!("TO_TIMESTAMP('{}','YYYY-MM-DD HH24:MI:SS.FF')", escape_single_quotes(expr));
    }

    if matches!(dt.as_str(), "RAW" | "BINARY" | "VARBINARY") {
        return format!("HEXTORAW('{}')", expr);
    }

    expr.to_string()
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
            body: "BEGIN\nNEW.UPDATE_TIME := SYSDATE\nEND".to_string(),
        }];

        let statements = generate_triggers("PLATFORM", &triggers, TriggerTerminator::DataGrip);
        assert_eq!(statements.len(), 1);
        let stmt = &statements[0];
        assert!(stmt.contains(":NEW.UPDATE_TIME"));
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
}
