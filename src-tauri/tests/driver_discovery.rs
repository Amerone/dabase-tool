#[cfg(target_os = "linux")]
mod tests {
    use dm8_export_tauri::driver::parse_odbcinst_for_dm8;

    #[test]
    fn parses_dm8_section_driver_value() {
        let ini = r#"
[DM8 ODBC DRIVER]
Description = DM8 Driver
Driver = /opt/dm/libdodbc.so
"#;
        let parsed = parse_odbcinst_for_dm8(ini);
        assert_eq!(
            parsed.unwrap().display().to_string(),
            "/opt/dm/libdodbc.so"
        );
    }
}
