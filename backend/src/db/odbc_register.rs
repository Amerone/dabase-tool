/// DM8 ODBC driver name used in Windows registry and connection strings.
pub const DM8_DRIVER_NAME: &str = "DM8 ODBC Driver";

/// Ensures the DM8 ODBC driver is registered in the Windows registry.
///
/// On Windows, the ODBC Driver Manager requires drivers to be registered under
/// `HKLM\SOFTWARE\ODBC\ODBCINST.INI` before they can be referenced by name in
/// connection strings. This function writes those entries at startup, so the
/// standalone binary works without a separate installer.
///
/// Tries HKLM first (system-wide, requires admin). Falls back to HKCU
/// (current user, no admin required) if HKLM write is denied.
#[cfg(windows)]
pub fn ensure_dm8_driver_registered(driver_dll: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    use winreg::enums::*;
    use winreg::RegKey;

    // Check if already registered under HKLM or HKCU to avoid redundant writes.
    for hive in &[HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let hive_key = RegKey::predef(*hive);
        if let Ok(drivers_key) =
            hive_key.open_subkey("SOFTWARE\\ODBC\\ODBCINST.INI\\ODBC Drivers")
        {
            let existing: Result<String, _> = drivers_key.get_value(DM8_DRIVER_NAME);
            if existing.is_ok() {
                tracing::debug!("DM8 ODBC driver already registered ({})", hive_name(*hive));
                return Ok(());
            }
        }
    }

    // Try to register under HKLM (system-wide).
    if try_register(HKEY_LOCAL_MACHINE, driver_dll).is_ok() {
        tracing::info!(
            "DM8 ODBC driver registered under HKLM: {}",
            driver_dll
        );
        return Ok(());
    }

    // Fall back to HKCU (current user, no admin required).
    try_register(HKEY_CURRENT_USER, driver_dll)
        .with_context(|| format!("Failed to register DM8 ODBC driver at '{}'", driver_dll))?;
    tracing::info!(
        "DM8 ODBC driver registered under HKCU (no admin): {}",
        driver_dll
    );
    Ok(())
}

#[cfg(windows)]
fn try_register(hive: winreg::HKEY, driver_dll: &str) -> anyhow::Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hive_key = RegKey::predef(hive);

    // ODBCINST.INI\ODBC Drivers  →  "DM8 ODBC Driver" = "Installed"
    let (drivers_key, _) = hive_key
        .create_subkey("SOFTWARE\\ODBC\\ODBCINST.INI\\ODBC Drivers")
        .map_err(|e| anyhow::anyhow!("create ODBC Drivers key: {}", e))?;
    drivers_key
        .set_value(DM8_DRIVER_NAME, &"Installed".to_string())
        .map_err(|e| anyhow::anyhow!("set ODBC Drivers value: {}", e))?;

    // ODBCINST.INI\DM8 ODBC Driver  →  Driver/Setup = dll path
    let (driver_key, _) = hive_key
        .create_subkey(format!(
            "SOFTWARE\\ODBC\\ODBCINST.INI\\{}",
            DM8_DRIVER_NAME
        ))
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

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn ensure_dm8_driver_registered(_driver_dll: &str) -> anyhow::Result<()> {
    Ok(())
}
