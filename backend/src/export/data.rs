use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use odbc_api::{Connection, Cursor, buffers::TextRowSet};

use crate::db::schema::get_table_details;

pub fn export_table_data(
    connection: &Connection<'_>,
    schema: &str,
    table: &str,
    writer: &mut impl Write,
    batch_size: usize,
) -> Result<()> {
    let schema_upper = schema.to_uppercase();
    let table_upper = table.to_uppercase();
    let qualified_table = format!("{}.{}", schema_upper, table_upper);

    let table_details = get_table_details(connection, &schema_upper, &table_upper)
        .with_context(|| format!("Failed to get table details for {}", qualified_table))?;

    let query = format!("SELECT * FROM {}", qualified_table);

    let mut cursor = match connection.execute(&query, ())? {
        Some(cursor) => cursor,
        None => {
            tracing::info!("No data to export for table {}", qualified_table);
            return Ok(());
        }
    };

    let mut batch = Vec::new();
    let mut row_count = 0;
    let mut buffers = TextRowSet::for_cursor(batch_size, &mut cursor, Some(8192))?;
    let mut row_set_cursor = cursor.bind_buffer(&mut buffers)?;

    while let Some(batch_result) = row_set_cursor.fetch()? {
        for row_index in 0..batch_result.num_rows() {
            let mut values = Vec::new();

            for (col_index, column) in table_details.columns.iter().enumerate() {
                let value = batch_result.at_as_str(col_index, row_index)?;

                let formatted_value = match value {
                    None => "NULL".to_string(),
                    Some(v) => {
                        if is_numeric_type(&column.data_type) {
                            v.to_string()
                        } else {
                            format!("'{}'", escape_single_quotes(v))
                        }
                    }
                };

                values.push(formatted_value);
            }

            batch.push(format!("({})", values.join(", ")));
            row_count += 1;

            if batch.len() >= batch_size {
                write_batch(writer, &qualified_table, &batch)?;
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        write_batch(writer, &qualified_table, &batch)?;
    }

    tracing::info!("Exported {} rows from {}", row_count, qualified_table);
    Ok(())
}

pub fn export_schema_data(
    connection: &Connection<'_>,
    schema: &str,
    tables: &[String],
    output_path: &Path,
    batch_size: usize,
) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                output_path.display()
            )
        })?;
    }

    let file = File::create(output_path).with_context(|| {
        format!("Failed to create data export file at {}", output_path.display())
    })?;
    let mut writer = BufWriter::new(file);

    for (i, table_name) in tables.iter().enumerate() {
        if i > 0 {
            writeln!(writer)?;
        }

        writeln!(writer, "-- Data for table: {}.{}", schema.to_uppercase(), table_name.to_uppercase())?;

        export_table_data(connection, schema, table_name, &mut writer, batch_size)
            .with_context(|| format!("Failed to export data for table '{}'", table_name))?;
    }

    writer.flush().context("Failed to flush data export to disk")?;
    Ok(())
}

fn write_batch(writer: &mut impl Write, table: &str, batch: &[String]) -> Result<()> {
    writeln!(
        writer,
        "INSERT INTO {} VALUES\n{};",
        table,
        batch.join(",\n")
    )?;
    Ok(())
}

fn is_numeric_type(data_type: &str) -> bool {
    let upper = data_type.to_uppercase();
    matches!(
        upper.as_str(),
        "NUMBER" | "INTEGER" | "INT" | "SMALLINT" | "BIGINT" | "DECIMAL" | "NUMERIC" | "FLOAT" | "DOUBLE" | "REAL"
    )
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "''")
}
