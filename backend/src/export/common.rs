use std::any::Any;

use odbc_api::{
    parameter::{VarBinaryArray, VarCharArray},
    CursorRow,
};

use crate::models::{Sequence, TableDetails};

const ODBC_STREAM_CHUNK_SIZE: usize = 8 * 1024;

/// Transform an identifier string according to the requested case mode.
pub fn transform_case(name: &str, case: &str) -> String {
    match case {
        "lower" => name.to_lowercase(),
        "upper" => name.to_uppercase(),
        _ => name.to_string(), // "preserve" or any unknown value
    }
}

/// Apply identifier case transformation to all names in a `TableDetails`.
/// Does NOT touch data values, SQL keywords, trigger bodies, or check constraint conditions.
pub fn apply_identifier_case(details: &mut TableDetails, case: &str) {
    if case == "preserve" {
        return;
    }
    details.name = transform_case(&details.name, case);
    for col in &mut details.columns {
        col.name = transform_case(&col.name, case);
    }
    for pk in &mut details.primary_keys {
        *pk = transform_case(pk, case);
    }
    for idx in &mut details.indexes {
        idx.name = transform_case(&idx.name, case);
        for col in &mut idx.columns {
            *col = transform_case(col, case);
        }
    }
    for uc in &mut details.unique_constraints {
        uc.name = transform_case(&uc.name, case);
        for col in &mut uc.columns {
            *col = transform_case(col, case);
        }
    }
    for cc in &mut details.check_constraints {
        cc.name = transform_case(&cc.name, case);
        // condition text is NOT transformed
    }
    for fk in &mut details.foreign_keys {
        fk.name = transform_case(&fk.name, case);
        for col in &mut fk.columns {
            *col = transform_case(col, case);
        }
        fk.referenced_table = transform_case(&fk.referenced_table, case);
        for col in &mut fk.referenced_columns {
            *col = transform_case(col, case);
        }
    }
    for trig in &mut details.triggers {
        trig.name = transform_case(&trig.name, case);
        trig.table_name = transform_case(&trig.table_name, case);
        // body is NOT transformed
    }
}

/// Apply identifier case transformation to a `Sequence`.
pub fn apply_sequence_case(seq: &mut Sequence, case: &str) {
    if case != "preserve" {
        seq.name = transform_case(&seq.name, case);
    }
}

/// Decode DM8 text bytes, falling back to GB18030 when the driver does not emit UTF-8.
pub fn decode_dm8_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => encoding_rs::GB18030.decode(bytes).0.into_owned(),
    }
}

/// Read a potentially large text field using fixed-size `SQLGetData` chunks.
///
/// This avoids `CursorRow::get_text`, whose adaptive buffer path can panic when a buggy ODBC
/// driver reports inconsistent remaining lengths across chunks.
pub fn read_text_streaming(
    row: &mut CursorRow<'_>,
    col_or_param_num: u16,
    out: &mut Vec<u8>,
) -> Result<bool, odbc_api::Error> {
    out.clear();

    let mut chunk = VarCharArray::<ODBC_STREAM_CHUNK_SIZE>::NULL;
    row.get_data(col_or_param_num, &mut chunk)?;

    let Some(bytes) = chunk.as_bytes() else {
        return Ok(false);
    };
    out.extend_from_slice(bytes);

    while !chunk.is_complete() {
        row.get_data(col_or_param_num, &mut chunk)?;
        if let Some(bytes) = chunk.as_bytes() {
            out.extend_from_slice(bytes);
        }
    }

    Ok(true)
}

/// Read a potentially large binary field using fixed-size `SQLGetData` chunks.
///
/// This avoids `CursorRow::get_binary`, whose adaptive buffer path can panic when a buggy ODBC
/// driver reports inconsistent remaining lengths across chunks.
pub fn read_binary_streaming(
    row: &mut CursorRow<'_>,
    col_or_param_num: u16,
    out: &mut Vec<u8>,
) -> Result<bool, odbc_api::Error> {
    out.clear();

    let mut chunk = VarBinaryArray::<ODBC_STREAM_CHUNK_SIZE>::NULL;
    row.get_data(col_or_param_num, &mut chunk)?;

    let Some(bytes) = chunk.as_bytes() else {
        return Ok(false);
    };
    out.extend_from_slice(bytes);

    while !chunk.is_complete() {
        row.get_data(col_or_param_num, &mut chunk)?;
        if let Some(bytes) = chunk.as_bytes() {
            out.extend_from_slice(bytes);
        }
    }

    Ok(true)
}

pub fn panic_payload_to_string(payload: Box<dyn Any + Send + 'static>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CheckConstraint, Column, ForeignKey, Index, Sequence, TableDetails, TriggerDefinition,
        UniqueConstraint,
    };

    fn make_column(name: &str) -> Column {
        Column {
            name: name.to_string(),
            data_type: "INTEGER".to_string(),
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

    fn make_table_details() -> TableDetails {
        TableDetails {
            name: "MY_TABLE".to_string(),
            comment: Some("A test table".to_string()),
            columns: vec![make_column("COL_A"), make_column("COL_B")],
            primary_keys: vec!["COL_A".to_string()],
            indexes: vec![Index {
                name: "IDX_COL_B".to_string(),
                columns: vec!["COL_B".to_string()],
                unique: false,
            }],
            unique_constraints: vec![UniqueConstraint {
                name: "UQ_COL_A".to_string(),
                columns: vec!["COL_A".to_string()],
            }],
            check_constraints: vec![CheckConstraint {
                name: "CK_POSITIVE".to_string(),
                condition: "COL_A > 0".to_string(),
            }],
            foreign_keys: vec![ForeignKey {
                name: "FK_OTHER".to_string(),
                columns: vec!["COL_B".to_string()],
                referenced_table: "OTHER_TABLE".to_string(),
                referenced_columns: vec!["ID".to_string()],
                delete_rule: None,
                update_rule: None,
            }],
            triggers: vec![TriggerDefinition {
                name: "TRG_INSERT".to_string(),
                table_name: "MY_TABLE".to_string(),
                timing: "BEFORE".to_string(),
                events: vec!["INSERT".to_string()],
                each_row: true,
                body: "BEGIN NULL; END;".to_string(),
            }],
        }
    }

    #[test]
    fn transform_case_lower() {
        assert_eq!(transform_case("MY_TABLE", "lower"), "my_table");
    }

    #[test]
    fn transform_case_upper() {
        assert_eq!(transform_case("my_table", "upper"), "MY_TABLE");
    }

    #[test]
    fn transform_case_preserve() {
        assert_eq!(transform_case("My_Table", "preserve"), "My_Table");
    }

    #[test]
    fn apply_identifier_case_lower() {
        let mut td = make_table_details();
        apply_identifier_case(&mut td, "lower");

        assert_eq!(td.name, "my_table");
        assert_eq!(td.columns[0].name, "col_a");
        assert_eq!(td.columns[1].name, "col_b");
        assert_eq!(td.primary_keys[0], "col_a");
        assert_eq!(td.indexes[0].name, "idx_col_b");
        assert_eq!(td.indexes[0].columns[0], "col_b");
        assert_eq!(td.unique_constraints[0].name, "uq_col_a");
        assert_eq!(td.check_constraints[0].name, "ck_positive");
        // condition is NOT transformed
        assert_eq!(td.check_constraints[0].condition, "COL_A > 0");
        assert_eq!(td.foreign_keys[0].name, "fk_other");
        assert_eq!(td.foreign_keys[0].referenced_table, "other_table");
        assert_eq!(td.foreign_keys[0].referenced_columns[0], "id");
        assert_eq!(td.triggers[0].name, "trg_insert");
        assert_eq!(td.triggers[0].table_name, "my_table");
        // trigger body is NOT transformed
        assert_eq!(td.triggers[0].body, "BEGIN NULL; END;");
    }

    #[test]
    fn apply_identifier_case_upper() {
        let mut td = make_table_details();
        td.name = "my_table".to_string();
        td.columns[0].name = "col_a".to_string();
        apply_identifier_case(&mut td, "upper");

        assert_eq!(td.name, "MY_TABLE");
        assert_eq!(td.columns[0].name, "COL_A");
    }

    #[test]
    fn apply_identifier_case_preserve_is_noop() {
        let td_original = make_table_details();
        let mut td = td_original.clone();
        apply_identifier_case(&mut td, "preserve");

        assert_eq!(td.name, td_original.name);
        assert_eq!(td.columns[0].name, td_original.columns[0].name);
        assert_eq!(td.indexes[0].name, td_original.indexes[0].name);
    }

    #[test]
    fn apply_sequence_case_lower() {
        let mut seq = Sequence {
            name: "SEQ_USER_ID".to_string(),
            increment_by: 1,
            start_with: Some(1),
            min_value: None,
            max_value: None,
            cache_size: Some(20),
            cycle: false,
            order: false,
        };
        apply_sequence_case(&mut seq, "lower");
        assert_eq!(seq.name, "seq_user_id");
    }

    #[test]
    fn apply_sequence_case_preserve() {
        let mut seq = Sequence {
            name: "SEQ_USER_ID".to_string(),
            increment_by: 1,
            start_with: Some(1),
            min_value: None,
            max_value: None,
            cache_size: Some(20),
            cycle: false,
            order: false,
        };
        apply_sequence_case(&mut seq, "preserve");
        assert_eq!(seq.name, "SEQ_USER_ID");
    }
}
