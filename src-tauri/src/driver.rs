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
    pub driver_path: PathBuf,
    pub search_dir: PathBuf,
    pub source: DriverSource,
}

/// Discover an available DM8 ODBC driver and set environment variables for loading it.
pub fn discover_and_apply(app: &tauri::AppHandle) -> Result<ResolvedDriver> {
    let driver = discover_driver(app)?;
    apply_env(&driver)?;
    Ok(driver)
}

fn driver_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "dodbc.dll"
    } else {
        "libdodbc.so"
    }
}

/// Platform-specific subdirectory under `drivers/dm8/` holding driver files.
/// Windows DLLs live in `drivers/dm8/windows/`; Linux SOs live directly in `drivers/dm8/`.
fn driver_subdir() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else {
        ""
    }
}

/// Relative path to the driver file, e.g.:
///   Windows → `drivers/dm8/windows/dodbc.dll`
///   Linux   → `drivers/dm8/libdodbc.so`
fn driver_rel_path() -> String {
    let sub = driver_subdir();
    if sub.is_empty() {
        format!("drivers/dm8/{}", driver_filename())
    } else {
        format!("drivers/dm8/{}/{}", sub, driver_filename())
    }
}

fn discover_driver(app: &tauri::AppHandle) -> Result<ResolvedDriver> {
    // 1) Bundled resource (works in dev and packaged)
    if let Some(resolved) = bundled_driver(app) {
        return Ok(resolved);
    }

    // 2) User-specified env
    if let Some(path) = env_driver() {
        return Ok(path);
    }

    // 3) System-installed driver
    if let Some(path) = system_driver() {
        return Ok(path);
    }

    Err(anyhow!(
        "No DM8 ODBC driver found. Checked bundled resources, DM8_DRIVER_PATH, and system ODBC registry/ini."
    ))
}

fn bundled_driver(app: &tauri::AppHandle) -> Option<ResolvedDriver> {
    let rel = driver_rel_path();

    // Packaged or dev mode via Tauri v2 path resolver
    if let Ok(path) = app
        .path()
        .resolve(&rel, tauri::path::BaseDirectory::Resource)
    {
        if path.exists() {
            let search_dir = path.parent()?.to_path_buf();
            return Some(ResolvedDriver {
                driver_path: path,
                search_dir,
                source: DriverSource::Bundled,
            });
        }
    }

    // Dev fallback: cwd is typically `src-tauri/` when running via `cargo run`
    let dev_path = std::env::current_dir()
        .ok()
        .map(|pwd| pwd.join("..").join(&rel));
    if let Some(path) = dev_path {
        if path.exists() {
            let search_dir = path.parent()?.to_path_buf();
            return Some(ResolvedDriver {
                driver_path: path,
                search_dir,
                source: DriverSource::Bundled,
            });
        }
    }
    None
}

fn env_driver() -> Option<ResolvedDriver> {
    let filename = driver_filename();
    if let Ok(raw) = env::var("DM8_DRIVER_PATH") {
        let path = PathBuf::from(raw.trim());
        if path.exists() {
            let search_dir = path.parent()?.to_path_buf();
            if path.file_name()?.to_string_lossy() == filename {
                return Some(ResolvedDriver {
                    driver_path: path,
                    search_dir,
                    source: DriverSource::Env,
                });
            }
        }
    }
    None
}

fn system_driver() -> Option<ResolvedDriver> {
    #[cfg(target_os = "linux")]
    {
        linux_system_driver()
    }
    #[cfg(target_os = "windows")]
    {
        windows_system_driver()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_system_driver() -> Option<ResolvedDriver> {
    let filename = driver_filename();
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
                    if found.exists() && found.file_name()?.to_string_lossy() == filename {
                        let search_dir = found.parent()?.to_path_buf();
                        return Some(ResolvedDriver {
                            driver_path: found,
                            search_dir,
                            source: DriverSource::System,
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
            current_section =
                Some(trimmed.trim_matches(&['[', ']'][..]).to_ascii_lowercase());
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
fn windows_system_driver() -> Option<ResolvedDriver> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let filename = driver_filename();
    // Only check HKLM — Windows ODBC Driver Manager ignores HKCU for driver definitions
    let reg_path = "SOFTWARE\\ODBC\\ODBCINST.INI\\DM8 ODBC Driver";
    if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(reg_path, KEY_READ)
    {
        if let Ok(value) = key.get_value::<String, _>("Driver") {
            let driver_path = PathBuf::from(value.trim());
            if driver_path.exists() && driver_path.file_name()?.to_string_lossy() == filename {
                let search_dir = driver_path.parent()?.to_path_buf();
                return Some(ResolvedDriver {
                    driver_path,
                    search_dir,
                    source: DriverSource::System,
                });
            }
        }
    }
    None
}

/// Set environment variables so the ODBC layer can find the DM8 driver.
///
/// SAFETY: `env::set_var` must be called before spawning additional threads.
/// In Tauri 2.x the `setup` hook runs on the main thread before the event loop,
/// so this is safe as long as no prior async work has been spawned.
fn apply_env(driver: &ResolvedDriver) -> Result<()> {
    env::set_var("DM8_DRIVER_PATH", &driver.driver_path);

    if cfg!(target_os = "windows") {
        prepend_path("PATH", &driver.search_dir)?;
    } else {
        prepend_path("LD_LIBRARY_PATH", &driver.search_dir)?;
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
