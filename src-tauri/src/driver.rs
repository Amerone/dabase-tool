use anyhow::{anyhow, Context, Result};
use std::env;
#[cfg(target_os = "linux")]
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum DriverSource {
    Bundled,
    Env,
    System,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedDriver {
    pub database: &'static str,
    pub driver_path: PathBuf,
    pub search_dir: PathBuf,
    pub source: DriverSource,
    pub required: bool,
    env_var: Option<&'static str>,
    configure_tns_admin: bool,
}

#[derive(Debug, Clone)]
pub struct DriverSetup {
    pub primary: ResolvedDriver,
    pub drivers: Vec<ResolvedDriver>,
}

#[derive(Clone, Copy)]
struct DriverSpec {
    database: &'static str,
    env_var: Option<&'static str>,
    bundled_rel_paths: &'static [&'static str],
    expected_filenames: &'static [&'static str],
    required: bool,
    configure_tns_admin: bool,
    allow_system_lookup: bool,
}

const DM8_WINDOWS_RELS: &[&str] = &["drivers/dm8/windows/dodbc.dll"];
const DM8_LINUX_RELS: &[&str] = &["drivers/dm8/libdodbc.so"];
const DM8_FILES: &[&str] = &["dodbc.dll", "libdodbc.so"];
const DM8_APP_DRIVER_NAME: &str = "Amarone DM8 ODBC Driver";
const DM8_SYSTEM_DRIVER_NAME: &str = "DM8 ODBC Driver";

const KINGBASE_WINDOWS_RELS: &[&str] =
    &["drivers/kingbase/X64_Windows/odbc/x64_ANSI_Release/kdbodbc30a.dll"];
const KINGBASE_LINUX_RELS: &[&str] = &["drivers/kingbase/X64_Linux/odbc/kdbodbcw.so"];
const KINGBASE_FILES: &[&str] = &["kdbodbc30a.dll", "kdbodbcw.so"];

const POSTGRESQL_WINDOWS_RELS: &[&str] = &["drivers/postgresql/windows/psqlodbc35w.dll"];
const POSTGRESQL_FILES: &[&str] = &["psqlodbc35w.dll", "psqlodbc30a.dll"];

const SHENTONG_WINDOWS_RELS: &[&str] = &["drivers/shentong/windows/aci.dll"];
const SHENTONG_FILES: &[&str] = &["aci.dll"];

/// Discover packaged database drivers and set environment variables before the backend starts.
pub fn discover_and_apply(app: &tauri::AppHandle) -> Result<DriverSetup> {
    let mut primary = None;
    let mut drivers = Vec::new();

    for spec in driver_specs() {
        match discover_driver(app, spec) {
            Some(driver) => {
                apply_env(&driver)?;
                if spec.required {
                    primary = Some(driver.clone());
                }
                drivers.push(driver);
            }
            None if spec.required => {
                return Err(anyhow!(
                    "No required {} driver found. Checked bundled resources, environment variables, and system driver registration.",
                    spec.database
                ));
            }
            None => {}
        }
    }

    let primary = primary.ok_or_else(|| anyhow!("No required database driver was resolved"))?;
    Ok(DriverSetup { primary, drivers })
}

fn driver_specs() -> Vec<DriverSpec> {
    let dm8_rels = if cfg!(target_os = "windows") {
        DM8_WINDOWS_RELS
    } else {
        DM8_LINUX_RELS
    };

    let kingbase_rels = if cfg!(target_os = "windows") {
        KINGBASE_WINDOWS_RELS
    } else {
        KINGBASE_LINUX_RELS
    };

    let mut specs = vec![
        DriverSpec {
            database: "DM8",
            env_var: Some("DM8_DRIVER_PATH"),
            bundled_rel_paths: dm8_rels,
            expected_filenames: DM8_FILES,
            required: true,
            configure_tns_admin: false,
            allow_system_lookup: true,
        },
        DriverSpec {
            database: "Kingbase ODBC",
            env_var: Some("KINGBASE_ODBC_DRIVER_PATH"),
            bundled_rel_paths: kingbase_rels,
            expected_filenames: KINGBASE_FILES,
            required: false,
            configure_tns_admin: false,
            allow_system_lookup: false,
        },
    ];

    if cfg!(target_os = "windows") {
        specs.push(DriverSpec {
            database: "PostgreSQL ODBC",
            env_var: Some("POSTGRESQL_ODBC_DRIVER_PATH"),
            bundled_rel_paths: POSTGRESQL_WINDOWS_RELS,
            expected_filenames: POSTGRESQL_FILES,
            required: false,
            configure_tns_admin: false,
            allow_system_lookup: false,
        });
        specs.push(DriverSpec {
            database: "ShenTong ACI",
            env_var: None,
            bundled_rel_paths: SHENTONG_WINDOWS_RELS,
            expected_filenames: SHENTONG_FILES,
            required: false,
            configure_tns_admin: true,
            allow_system_lookup: false,
        });
    }

    specs
}

fn discover_driver(app: &tauri::AppHandle, spec: DriverSpec) -> Option<ResolvedDriver> {
    bundled_driver(app, spec)
        .or_else(|| env_driver(spec))
        .or_else(|| system_driver(spec))
}

fn bundled_driver(app: &tauri::AppHandle, spec: DriverSpec) -> Option<ResolvedDriver> {
    for rel_path in spec.bundled_rel_paths {
        for path in bundled_candidates(app, rel_path) {
            if path.exists() && filename_allowed(&path, spec.expected_filenames) {
                let search_dir = path.parent()?.to_path_buf();
                return Some(ResolvedDriver {
                    database: spec.database,
                    driver_path: path,
                    search_dir,
                    source: DriverSource::Bundled,
                    required: spec.required,
                    env_var: spec.env_var,
                    configure_tns_admin: spec.configure_tns_admin,
                });
            }
        }
    }
    None
}

fn bundled_candidates(app: &tauri::AppHandle, rel_path: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(path) = app
        .path()
        .resolve(rel_path, tauri::path::BaseDirectory::Resource)
    {
        candidates.push(path);
    }

    if let Ok(pwd) = env::current_dir() {
        candidates.push(pwd.join(rel_path));
        candidates.push(pwd.join("..").join(rel_path));
    }

    candidates
}

fn env_driver(spec: DriverSpec) -> Option<ResolvedDriver> {
    let env_var = spec.env_var?;
    let raw = env::var(env_var).ok()?;
    let path = PathBuf::from(raw.trim());
    if !path.exists() || !filename_allowed(&path, spec.expected_filenames) {
        return None;
    }

    let search_dir = path.parent()?.to_path_buf();
    Some(ResolvedDriver {
        database: spec.database,
        driver_path: path,
        search_dir,
        source: DriverSource::Env,
        required: spec.required,
        env_var: spec.env_var,
        configure_tns_admin: spec.configure_tns_admin,
    })
}

fn system_driver(spec: DriverSpec) -> Option<ResolvedDriver> {
    if !spec.allow_system_lookup || spec.database != "DM8" {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        linux_system_dm8_driver(spec)
    }
    #[cfg(target_os = "windows")]
    {
        windows_system_dm8_driver(spec)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

fn filename_allowed(path: &Path, expected_filenames: &[&str]) -> bool {
    if expected_filenames.is_empty() {
        return true;
    }

    let Some(filename) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };

    expected_filenames
        .iter()
        .any(|expected| filename.eq_ignore_ascii_case(expected))
}

#[cfg(target_os = "linux")]
fn linux_system_dm8_driver(spec: DriverSpec) -> Option<ResolvedDriver> {
    let candidates = ["/etc/odbcinst.ini", "~/.odbcinst.ini"];

    for candidate in candidates {
        let expanded = if candidate.starts_with('~') {
            dirs::home_dir().map(|home| home.join(candidate.trim_start_matches("~/")))
        } else {
            Some(PathBuf::from(candidate))
        };

        if let Some(path) = expanded {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Some(found) = parse_odbcinst_for_dm8(&content) {
                    if found.exists() && filename_allowed(&found, spec.expected_filenames) {
                        let search_dir = found.parent()?.to_path_buf();
                        return Some(ResolvedDriver {
                            database: spec.database,
                            driver_path: found,
                            search_dir,
                            source: DriverSource::System,
                            required: spec.required,
                            env_var: spec.env_var,
                            configure_tns_admin: spec.configure_tns_admin,
                        });
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(crate) fn parse_odbcinst_for_dm8(content: &str) -> Option<PathBuf> {
    let mut current_section: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = Some(trimmed.trim_matches(&['[', ']'][..]).to_ascii_lowercase());
            continue;
        }

        if current_section.as_deref() == Some("dm8 odbc driver") {
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                if key.starts_with("driver") {
                    return Some(PathBuf::from(value.trim()));
                }
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn windows_system_dm8_driver(spec: DriverSpec) -> Option<ResolvedDriver> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    // Only check HKLM. Windows ODBC Driver Manager ignores HKCU for driver definitions.
    for driver_name in [DM8_APP_DRIVER_NAME, DM8_SYSTEM_DRIVER_NAME] {
        let reg_path = format!("SOFTWARE\\ODBC\\ODBCINST.INI\\{}", driver_name);
        if let Ok(key) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(reg_path, KEY_READ)
        {
            if let Ok(value) = key.get_value::<String, _>("Driver") {
                let driver_path = PathBuf::from(value.trim());
                if driver_path.exists() && filename_allowed(&driver_path, spec.expected_filenames) {
                    let search_dir = driver_path.parent()?.to_path_buf();
                    return Some(ResolvedDriver {
                        database: spec.database,
                        driver_path,
                        search_dir,
                        source: DriverSource::System,
                        required: spec.required,
                        env_var: spec.env_var,
                        configure_tns_admin: spec.configure_tns_admin,
                    });
                }
            }
        }
    }
    None
}

/// Set environment variables so the backend can find packaged database clients.
///
/// SAFETY: `env::set_var` must be called before spawning additional threads. In Tauri 2.x the
/// `setup` hook runs on the main thread before the event loop, so this is safe as long as no prior
/// async work has been spawned.
fn apply_env(driver: &ResolvedDriver) -> Result<()> {
    if let Some(env_var) = driver.env_var {
        env::set_var(env_var, &driver.driver_path);
    }

    if cfg!(target_os = "windows") {
        prepend_path("PATH", &driver.search_dir)?;
    } else {
        prepend_path("LD_LIBRARY_PATH", &driver.search_dir)?;
    }

    if driver.configure_tns_admin && env::var("TNS_ADMIN").unwrap_or_default().is_empty() {
        env::set_var("TNS_ADMIN", &driver.search_dir);
    }

    Ok(())
}

fn prepend_path(var: &str, dir: &Path) -> Result<()> {
    let mut paths: Vec<PathBuf> = env::var_os(var)
        .map(|val| env::split_paths(&val).collect())
        .unwrap_or_default();

    if !paths.iter().any(|p| p == dir) {
        paths.insert(0, dir.to_path_buf());
    }
    let joined = env::join_paths(paths).context("failed to join paths")?;
    env::set_var(var, &joined);
    Ok(())
}
