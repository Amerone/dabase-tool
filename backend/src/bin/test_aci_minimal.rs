// Minimal ACI FFI test: priming hypothesis
// Test A: ONLY configured target (expected FAIL - no priming)
// Test B: localhost first, then configured target in fresh env (expected SUCCESS - primed)

#[cfg(windows)]
use std::ffi::{c_void, CString};

#[path = "shentong_diag_env.rs"]
mod shentong_diag_env;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

fn main() {
    println!("=== Minimal ACI priming test ===");
    let aci_dir = find_aci_dir();
    if let Some(ref d) = aci_dir {
        println!("TNS_ADMIN = {}", d);
        std::env::set_var("TNS_ADMIN", d);
    }
    #[cfg(windows)]
    run(aci_dir);
    #[cfg(not(windows))]
    println!("Windows only");
}

fn find_aci_dir() -> Option<String> {
    for dir in &["drivers/shentong/windows", "../drivers/shentong/windows"] {
        if std::path::Path::new(dir).join("aci.dll").exists() {
            if let Ok(abs) = std::fs::canonicalize(dir) {
                let s = abs.to_string_lossy().into_owned();
                return Some(s.trim_start_matches(r"\\?\").to_string());
            }
        }
    }
    None
}

#[cfg(windows)]
fn run(aci_dir: Option<String>) {
    let dll_path = match aci_dir {
        Some(ref d) => format!("{}\\aci.dll\0", d),
        None => "aci.dll\0".to_string(),
    };
    let module = unsafe { LoadLibraryA(dll_path.as_ptr()) };
    if module.is_null() {
        eprintln!("LoadLibraryA failed!");
        return;
    }
    println!("DLL: {:p}", module);

    macro_rules! sym {
        ($name:expr, $ty:ty) => {
            unsafe {
                let cname = CString::new($name).unwrap();
                std::mem::transmute::<*mut c_void, $ty>(GetProcAddress(
                    module,
                    cname.as_ptr() as *const u8,
                ))
            }
        };
    }

    type FnEnvNls = unsafe extern "C" fn(
        *mut *mut c_void,
        u32,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        *mut *mut c_void,
        u16,
        u16,
    ) -> i32;
    type FnAlloc =
        unsafe extern "C" fn(*mut c_void, *mut *mut c_void, u32, usize, *mut *mut c_void) -> i32;
    type FnAttach = unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, i32, u32) -> i32;
    type FnAttrSet =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32, u32, *mut c_void) -> i32;
    type FnSessBegin = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32) -> i32;
    type FnErrGet =
        unsafe extern "C" fn(*mut c_void, u32, *const u8, *mut i32, *mut u8, u32, u32) -> i32;

    let env_nls: FnEnvNls = sym!("ACIEnvNlsCreate", FnEnvNls);
    let alloc: FnAlloc = sym!("ACIHandleAlloc", FnAlloc);
    let attach: FnAttach = sym!("ACIServerAttach", FnAttach);
    let attr_set: FnAttrSet = sym!("ACIAttrSet", FnAttrSet);
    let sess_begin: FnSessBegin = sym!("ACISessionBegin", FnSessBegin);
    let err_get: FnErrGet = sym!("ACIErrorGet", FnErrGet);

    let try_connect = |cs: &str, label: &str| -> bool {
        let mut env: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env,
                2,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                871,
                871,
            )
        };
        if rc != 0 || env.is_null() {
            println!("  [{}] EnvNlsCreate FAIL", label);
            return false;
        }
        let mut err: *mut c_void = std::ptr::null_mut();
        let mut svc: *mut c_void = std::ptr::null_mut();
        let mut srv: *mut c_void = std::ptr::null_mut();
        unsafe {
            alloc(env, &mut err, 2, 0, std::ptr::null_mut());
            alloc(env, &mut svc, 3, 0, std::ptr::null_mut());
            alloc(env, &mut srv, 8, 0, std::ptr::null_mut());
        }
        let b = cs.as_bytes();
        let rc2 = unsafe { attach(srv, err, b.as_ptr(), b.len() as i32, 0) };
        if rc2 == 0 {
            println!("  [{}] attach OK → trying session...", label);
            // Full session test like test_aci_ffi
            let mut ses: *mut c_void = std::ptr::null_mut();
            let user = shentong_diag_env::user("sysdba");
            let password = shentong_diag_env::password();
            let u = user.as_bytes();
            let p = password.as_bytes();
            unsafe {
                attr_set(svc, 3, srv, 0, 6, err);
                alloc(env, &mut ses, 9, 0, std::ptr::null_mut());
                attr_set(ses, 9, u.as_ptr() as *mut c_void, u.len() as u32, 22, err);
                attr_set(ses, 9, p.as_ptr() as *mut c_void, p.len() as u32, 23, err);
                attr_set(svc, 3, ses, 0, 7, err);
            }
            let rc3 = unsafe { sess_begin(svc, err, ses, 1, 0) };
            println!("  [{}] SessionBegin: rc={}", label, rc3);
            rc3 == 0
        } else {
            let mut buf = vec![0u8; 256];
            let mut code: i32 = 0;
            unsafe {
                err_get(
                    err,
                    1,
                    std::ptr::null(),
                    &mut code,
                    buf.as_mut_ptr(),
                    256,
                    2,
                );
            }
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            println!(
                "  [{}] attach FAIL code={} '{}'",
                label,
                code,
                String::from_utf8_lossy(&buf[..end])
            );
            false
        }
    };

    let configured_connect = shentong_diag_env::default_connect();

    println!("\n--- Test A: ONLY configured target (no priming) ---");
    let a = try_connect(&configured_connect, "A");
    println!("  Test A: {}", if a { "SUCCESS" } else { "FAIL" });

    println!("\n--- Test B: localhost first (prime), then configured target ---");
    try_connect("localhost:2003/osrdb", "B-prime");
    let b = try_connect(&configured_connect, "B-real");
    println!(
        "  Test B: {}",
        if b {
            "SUCCESS (priming works!)"
        } else {
            "FAIL (priming not enough)"
        }
    );

    println!(
        "\nConclusion: {}",
        if b && !a {
            "PRIMING CONFIRMED: hostname attempt needed before IP connect"
        } else if a {
            "No priming needed"
        } else {
            "Both fail - different root cause"
        }
    );
}
