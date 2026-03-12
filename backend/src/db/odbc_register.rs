/// ODBC driver names used in connection strings and Windows registry.
pub const DM8_DRIVER_NAME: &str = "DM8 ODBC Driver";
pub const KINGBASE_DRIVER_NAME: &str = "KingbaseES 9 ODBC Driver ANSI";
pub const SHENTONG_DRIVER_NAME: &str = "OSCAR ODBC DRIVER";
pub const POSTGRESQL_DRIVER_NAME: &str = "PostgreSQL Unicode";

/// Bundled driver candidate paths for Windows.
#[cfg(windows)]
pub const DM8_DRIVER_CANDIDATES: &[&str] = &[
    "drivers/dm8/windows/dodbc.dll",
    "drivers/dm8/windows/libdodbc.dll",
    "../drivers/dm8/windows/dodbc.dll",
    "../drivers/dm8/windows/libdodbc.dll",
];

#[cfg(windows)]
pub const KINGBASE_DRIVER_CANDIDATES: &[&str] = &[
    "drivers/kingbase/windows/kdbodbcw.dll",
    "drivers/kingbase/windows/kdbodbc.dll",
    "drivers/kingbase/X64_Windows/odbc/x64_ANSI_Release/kdbodbc30a.dll",
    "../drivers/kingbase/windows/kdbodbcw.dll",
    "../drivers/kingbase/windows/kdbodbc.dll",
    "../drivers/kingbase/X64_Windows/odbc/x64_ANSI_Release/kdbodbc30a.dll",
];

#[cfg(windows)]
pub const SHENTONG_DRIVER_CANDIDATES: &[&str] = &[
    "drivers/shentong/windows/oscarodbcw.dll",
    "drivers/shentong/windows/oscarodbc.dll",
    "../drivers/shentong/windows/oscarodbcw.dll",
    "../drivers/shentong/windows/oscarodbc.dll",
];

#[cfg(windows)]
pub const POSTGRESQL_DRIVER_CANDIDATES: &[&str] = &[
    "drivers/postgresql/windows/psqlodbc35w.dll",
    "../drivers/postgresql/windows/psqlodbc35w.dll",
    "drivers/postgresql/windows/psqlodbc30a.dll",
    "../drivers/postgresql/windows/psqlodbc30a.dll",
];

/// Backward-compatible wrapper for DM8 registration.
#[cfg(windows)]
pub fn ensure_dm8_driver_registered(driver_dll: &str) -> anyhow::Result<()> {
    ensure_odbc_driver_registered(DM8_DRIVER_NAME, driver_dll)
}

/// Ensures the given ODBC driver is registered in Windows registry.
///
/// The Driver Manager requires entries under:
/// - HKLM/HKCU\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers
/// - HKLM/HKCU\SOFTWARE\ODBC\ODBCINST.INI\<driver_name>
#[cfg(windows)]
pub fn ensure_odbc_driver_registered(driver_name: &str, driver_dll: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    use winreg::enums::*;
    use winreg::RegKey;

    // Check if an existing registration has an absolute path already.
    // If registered with a relative path, re-register with the absolute path we were given.
    for hive in &[HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let hive_key = RegKey::predef(*hive);
        if let Ok(drivers_key) = hive_key.open_subkey("SOFTWARE\\ODBC\\ODBCINST.INI\\ODBC Drivers")
        {
            let existing: Result<String, _> = drivers_key.get_value(driver_name);
            if existing.is_ok() {
                // Driver entry exists — verify the actual Driver path is absolute
                let driver_key_path = format!("SOFTWARE\\ODBC\\ODBCINST.INI\\{}", driver_name);
                if let Ok(driver_key) = hive_key.open_subkey(&driver_key_path) {
                    let current_path: Result<String, _> = driver_key.get_value("Driver");
                    let needs_update = current_path
                        .as_ref()
                        .map(|p| {
                            // Relative path: doesn't start with a drive letter (e.g. C:\) or UNC (\\)
                            !p.starts_with("\\\\") && !p.contains(":\\")
                        })
                        .unwrap_or(true);

                    if needs_update {
                        tracing::info!(
                            "ODBC driver '{}' registered with relative path ('{:?}'), re-registering with absolute path: {}",
                            driver_name,
                            current_path.as_deref().unwrap_or("<unreadable>"),
                            driver_dll
                        );
                        let _ = try_register(*hive, driver_name, driver_dll);
                        return Ok(());
                    }
                }

                tracing::debug!(
                    "ODBC driver '{}' already registered ({})",
                    driver_name,
                    hive_name(*hive)
                );
                return Ok(());
            }
        }
    }

    if try_register(HKEY_LOCAL_MACHINE, driver_name, driver_dll).is_ok() {
        tracing::info!(
            "ODBC driver '{}' registered under HKLM: {}",
            driver_name,
            driver_dll
        );
        return Ok(());
    }

    try_register(HKEY_CURRENT_USER, driver_name, driver_dll).with_context(|| {
        format!(
            "Failed to register ODBC driver '{}' at '{}'",
            driver_name, driver_dll
        )
    })?;

    tracing::info!(
        "ODBC driver '{}' registered under HKCU (no admin): {}",
        driver_name,
        driver_dll
    );

    Ok(())
}

#[cfg(windows)]
fn try_register(hive: winreg::HKEY, driver_name: &str, driver_dll: &str) -> anyhow::Result<()> {
    use winreg::RegKey;

    let hive_key = RegKey::predef(hive);

    let (drivers_key, _) = hive_key
        .create_subkey("SOFTWARE\\ODBC\\ODBCINST.INI\\ODBC Drivers")
        .map_err(|e| anyhow::anyhow!("create ODBC Drivers key: {}", e))?;
    drivers_key
        .set_value(driver_name, &"Installed".to_string())
        .map_err(|e| anyhow::anyhow!("set ODBC Drivers value: {}", e))?;

    let (driver_key, _) = hive_key
        .create_subkey(format!("SOFTWARE\\ODBC\\ODBCINST.INI\\{}", driver_name))
        .map_err(|e| anyhow::anyhow!("create driver key: {}", e))?;
    driver_key
        .set_value("Driver", &driver_dll.to_string())
        .map_err(|e| anyhow::anyhow!("set Driver value: {}", e))?;
    driver_key
        .set_value("Setup", &driver_dll.to_string())
        .map_err(|e| anyhow::anyhow!("set Setup value: {}", e))?;

    Ok(())
}

#[cfg(windows)]
fn hive_name(hive: winreg::HKEY) -> &'static str {
    use winreg::enums::*;
    if hive == HKEY_LOCAL_MACHINE {
        "HKLM"
    } else {
        "HKCU"
    }
}

#[cfg(not(windows))]
pub fn ensure_dm8_driver_registered(_driver_dll: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn ensure_odbc_driver_registered(_driver_name: &str, _driver_dll: &str) -> anyhow::Result<()> {
    Ok(())
}
