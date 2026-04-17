/// ODBC driver names used in connection strings and Windows registry.
pub const DM8_DRIVER_NAME: &str = "Amarone DM8 ODBC Driver";
pub const DM8_SYSTEM_DRIVER_NAME: &str = "DM8 ODBC Driver";
pub const KINGBASE_DRIVER_NAME: &str = "Amarone KingbaseES 9 ODBC Driver ANSI";
pub const SHENTONG_DRIVER_NAME: &str = "OSCAR ODBC DRIVER";
pub const POSTGRESQL_DRIVER_NAME: &str = "Amarone PostgreSQL Unicode";

/// Bundled driver candidate paths for Windows.
/// Bundled drivers are preferred (self-contained app); local DM8 installation
/// is a fallback for machines where the bundled driver cannot be used.
#[cfg(windows)]
pub const DM8_DRIVER_CANDIDATES: &[&str] = &[
    // Bundled drivers (preferred for standalone app)
    "drivers/dm8/windows/dodbc.dll",
    "drivers/dm8/windows/libdodbc.dll",
    "../drivers/dm8/windows/dodbc.dll",
    "../drivers/dm8/windows/libdodbc.dll",
    // Local DM8 installation fallback
    "C:/dmdbms/bin/dodbc.dll",
    "D:/dmdbms/bin/dodbc.dll",
    "E:/dmdbms/bin/dodbc.dll",
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

/// Ensures the given ODBC driver is registered in HKLM so the Windows ODBC
/// Driver Manager can find it.
///
/// # Windows ODBC Driver Manager behaviour
///
/// The Windows ODBC Driver Manager (`odbc32.dll`) **only** reads driver
/// registrations from HKLM.  HKCU registrations and absolute DLL paths in
/// the DRIVER= connection-string keyword are both silently ignored, returning
/// IM002.  Therefore we must write to HKLM.
///
/// Strategy:
///   1. If the driver is already in HKLM with a valid absolute path → done.
///   2. Try writing to HKLM directly (succeeds when running as admin).
///   3. If that fails (permission denied), spawn an elevated PowerShell
///      process via `Start-Process -Verb RunAs` (triggers UAC once) and
///      re-check HKLM afterwards.
///   4. If elevation is unavailable or declined, log a warning and fall back
///      to HKCU (driver won't work but at least we don't crash).
#[cfg(windows)]
pub fn ensure_odbc_driver_registered(driver_name: &str, driver_dll: &str) -> anyhow::Result<()> {
    use winreg::enums::*;

    // --- Step 1: check if already in HKLM with an absolute path ---
    if is_registered_in_hklm(driver_name) {
        tracing::debug!("ODBC driver '{}' already registered in HKLM", driver_name);
        return Ok(());
    }

    // --- Step 2: try direct HKLM write (works when admin) ---
    if try_register(HKEY_LOCAL_MACHINE, driver_name, driver_dll).is_ok() {
        tracing::info!(
            "ODBC driver '{}' registered in HKLM: {}",
            driver_name,
            driver_dll
        );
        return Ok(());
    }

    // --- Step 3: elevate via UAC and register in HKLM ---
    tracing::info!(
        "HKLM write denied for '{}'; requesting UAC elevation to register ODBC driver",
        driver_name
    );
    if register_hklm_elevated(driver_name, driver_dll).is_ok() {
        if is_registered_in_hklm(driver_name) {
            tracing::info!(
                "ODBC driver '{}' successfully registered in HKLM via elevation",
                driver_name
            );
            return Ok(());
        }
        tracing::warn!(
            "Elevation completed but '{}' still not visible in HKLM",
            driver_name
        );
    } else {
        tracing::warn!(
            "UAC elevation for ODBC driver '{}' was declined or failed",
            driver_name
        );
    }

    // --- Step 4: HKCU fallback (driver won't work with Windows ODBC DM but
    //     we record the registration so the path is at least stored) ---
    let _ = try_register(HKEY_CURRENT_USER, driver_name, driver_dll);
    tracing::warn!(
        "ODBC driver '{}' registered only in HKCU. \
         The Windows ODBC Driver Manager requires HKLM registration; \
         connections will fail unless the backend is run as Administrator \
         at least once to complete HKLM registration.",
        driver_name
    );

    Ok(())
}

/// Returns true if the driver is registered in HKLM with an absolute Driver path.
#[cfg(windows)]
pub(crate) fn is_registered_in_hklm(driver_name: &str) -> bool {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    // Check ODBC Drivers list
    let drivers_key = match hklm.open_subkey("SOFTWARE\\ODBC\\ODBCINST.INI\\ODBC Drivers") {
        Ok(k) => k,
        Err(_) => return false,
    };
    let listed: Result<String, _> = drivers_key.get_value(driver_name);
    if listed.is_err() {
        return false;
    }
    // Check Driver path is absolute
    let driver_key_path = format!("SOFTWARE\\ODBC\\ODBCINST.INI\\{}", driver_name);
    if let Ok(dk) = hklm.open_subkey(&driver_key_path) {
        let path: Result<String, _> = dk.get_value("Driver");
        return path
            .map(|p| p.contains(":\\") || p.starts_with("\\\\"))
            .unwrap_or(false);
    }
    false
}

/// Spawn an elevated process (UAC prompt) to write HKLM registry entries for
/// the ODBC driver.  Uses a temporary .bat file to avoid PowerShell quoting
/// issues with nested invocations.
#[cfg(windows)]
fn register_hklm_elevated(driver_name: &str, driver_dll: &str) -> anyhow::Result<()> {
    use std::io::Write as _;

    // Write a temp batch file with the three reg add commands.
    let tmp_bat =
        std::env::temp_dir().join(format!("_odbc_register_hklm_{}.bat", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp_bat)
            .map_err(|e| anyhow::anyhow!("create temp bat: {}", e))?;
        writeln!(f, "@echo off")?;
        writeln!(
            f,
            "reg add \"HKLM\\SOFTWARE\\ODBC\\ODBCINST.INI\\ODBC Drivers\" /v \"{}\" /d Installed /f",
            driver_name
        )?;
        writeln!(
            f,
            "reg add \"HKLM\\SOFTWARE\\ODBC\\ODBCINST.INI\\{}\" /v Driver /d \"{}\" /f",
            driver_name, driver_dll
        )?;
        writeln!(
            f,
            "reg add \"HKLM\\SOFTWARE\\ODBC\\ODBCINST.INI\\{}\" /v Setup /d \"{}\" /f",
            driver_name, driver_dll
        )?;
    }

    let bat_path = tmp_bat.display().to_string();

    // Start-Process -Verb RunAs triggers UAC; -Wait blocks until the bat exits.
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Start-Process '{}' -Verb RunAs -Wait", bat_path),
        ])
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to spawn elevation process: {}", e))?;

    let _ = std::fs::remove_file(&tmp_bat);

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Elevated batch exited with status: {:?}",
            status.code()
        ))
    }
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

#[cfg(not(windows))]
pub fn ensure_dm8_driver_registered(_driver_dll: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn ensure_odbc_driver_registered(_driver_name: &str, _driver_dll: &str) -> anyhow::Result<()> {
    Ok(())
}
