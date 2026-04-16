// Enable ODPI debug output to trace crash location
extern "C" {
    static mut dpiDebugLevel: u64;
}

fn main() {
    println!("=== ShenTong ACI connection test ===");

    // Enable ODPI LOAD_LIB + ERRORS + FNS debug
    unsafe {
        dpiDebugLevel = 0x40 | 0x08 | 0x01 | 0x02; // LOAD_LIB | ERRORS | FNS | SQL
    }

    // Locate aci.dll directory and set TNS_ADMIN so ACI can find tnsnames.aci.
    // Use the drivers source dir directly — this is what works for direct FFI tests.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let aci_candidates = ["drivers/shentong/windows", "../drivers/shentong/windows"];

    // Set TNS_ADMIN to the ACI drivers source dir (which has tnsnames.aci + sqlnet.aci).
    // This is the same directory used by the successful direct FFI test.
    let mut aci_dir_str: Option<String> = None;
    for dir in &aci_candidates {
        let p = std::path::Path::new(dir).join("aci.dll");
        if p.exists() {
            if let Ok(abs) = std::fs::canonicalize(dir) {
                let s = abs.to_string_lossy().into_owned();
                let s = s.trim_start_matches(r"\\?\").to_string();
                aci_dir_str = Some(s);
                break;
            }
        }
    }

    if let Some(ref aci_str) = aci_dir_str {
        println!("Setting TNS_ADMIN = {} (ACI drivers dir)", aci_str);
        std::env::set_var("TNS_ADMIN", aci_str);
    } else if let Some(ref exe_d) = exe_dir {
        let exe_str = exe_d.to_string_lossy().into_owned();
        println!("Exe dir: {}", exe_str);
        println!("Setting TNS_ADMIN = {} (exe dir fallback)", exe_str);
        std::env::set_var("TNS_ADMIN", &exe_str);
    }

    // Also add the aci drivers dir to PATH
    for dir in &aci_candidates {
        let p = std::path::Path::new(dir).join("aci.dll");
        if p.exists() {
            if let Ok(abs) = std::fs::canonicalize(dir) {
                // Strip \\?\ prefix if present (ACI doesn't like it in PATH)
                let abs_str = abs.to_string_lossy().into_owned();
                let abs_str = abs_str.trim_start_matches(r"\\?\").to_string();
                let cur_path = std::env::var("PATH").unwrap_or_default();
                if !cur_path
                    .split(';')
                    .any(|p| p.eq_ignore_ascii_case(&abs_str))
                {
                    std::env::set_var("PATH", format!("{};{}", abs_str, cur_path));
                }
                break;
            }
        }
    }

    // ShenTong ACI connect string formats to try:
    //   1. Simple connect: host:port/service  (no // prefix)
    //   2. TNS alias (looked up in tnsnames.aci via TNS_ADMIN)
    //   3. Remote machine (bypasses Docker networking)
    //   4. Full Oracle-style connect descriptor (bypasses both EZConnect and TNS lookup)
    let connect_strings = [
        // Simple connect format (ShenTong native - no // prefix)
        ("simple", "localhost:2003/osrdb"),
        ("simple-upper", "localhost:2003/OSRDB"),
        ("127_simple", "127.0.0.1:2003/osrdb"),
        // TNS aliases from tnsnames.aci
        ("tns-alias-lower", "osrdb"),
        ("tns-alias-upper", "OSRDB"),
        // EZConnect with // (Oracle style - may or may not work)
        ("ezconnect", "//localhost:2003/OSRDB"),
        // Remote Windows machine (bypasses Docker)
        ("remote", "192.168.3.34:2003/osrdb"),
        ("remote-tns", "remote_osrdb"),
        // Full Oracle-style connect descriptor (bypasses TNS resolution entirely)
        ("full-desc-remote", "(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)(HOST=192.168.3.34)(PORT=2003))(CONNECT_DATA=(SERVICE_NAME=osrdb)))"),
        ("full-desc-local", "(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)(HOST=localhost)(PORT=2003))(CONNECT_DATA=(SERVICE_NAME=osrdb)))"),
    ];

    for (label, connect_str) in &connect_strings {
        println!("\n[{}] Trying: {}", label, connect_str);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        let result = shentong::Connection::connect("sysdba", "szoscar55", connect_str);
        match result {
            Ok(_) => {
                println!("SUCCESS! Connected to ShenTong with: {}", connect_str);
                return;
            }
            Err(e) => println!("Error: {}", e),
        }
    }
    println!("\nAll connection attempts failed.");
}
