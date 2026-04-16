/// ACI reuse-variable test: use SAME stack variable for both attempts
/// In test_aci_ffi for loop, env_h/err_h/svc_h/srv_h are at the SAME stack
/// addresses across iterations. Test if reusing same variables is the key.

#[cfg(windows)]
use std::ffi::{c_void, CString};

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

#[cfg(windows)]
fn get_fn(module: *mut c_void, name: &str) -> Option<*mut c_void> {
    let cname = CString::new(name).unwrap();
    unsafe {
        let p = GetProcAddress(module, cname.as_ptr() as *const u8);
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }
}

fn main() {
    println!("=== ACI reuse-variable test ===");
    let aci_dir = find_aci_dir();
    if let Some(ref d) = aci_dir {
        println!("TNS_ADMIN = {}", d);
        std::env::set_var("TNS_ADMIN", d);
    }
    #[cfg(windows)]
    run(aci_dir);
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

    let env_nls: FnEnvNls =
        unsafe { std::mem::transmute(get_fn(module, "ACIEnvNlsCreate").unwrap()) };
    let alloc: FnAlloc = unsafe { std::mem::transmute(get_fn(module, "ACIHandleAlloc").unwrap()) };
    let attach: FnAttach =
        unsafe { std::mem::transmute(get_fn(module, "ACIServerAttach").unwrap()) };
    let attr_set: FnAttrSet = unsafe { std::mem::transmute(get_fn(module, "ACIAttrSet").unwrap()) };
    let sess_begin: FnSessBegin =
        unsafe { std::mem::transmute(get_fn(module, "ACISessionBegin").unwrap()) };
    let err_get: FnErrGet = unsafe { std::mem::transmute(get_fn(module, "ACIErrorGet").unwrap()) };

    let get_error = |err_h: *mut c_void| {
        let mut buf = vec![0u8; 256];
        let mut code: i32 = 0;
        unsafe {
            err_get(
                err_h,
                1,
                std::ptr::null(),
                &mut code,
                buf.as_mut_ptr(),
                256,
                2,
            );
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        (code, String::from_utf8_lossy(&buf[..end]).to_string())
    };

    // === Test: REUSE same stack variables for both attempts ===
    // Declare handles ONCE, outside the per-attempt blocks
    let mut env_h: *mut c_void = std::ptr::null_mut();
    let mut err_h: *mut c_void = std::ptr::null_mut();
    let mut svc_h: *mut c_void = std::ptr::null_mut();
    let mut srv_h: *mut c_void = std::ptr::null_mut();

    println!(
        "\n--- Attempt 1: localhost (prime) using env_h@{:p} ---",
        &env_h
    );
    env_h = std::ptr::null_mut();
    let rc = unsafe {
        env_nls(
            &mut env_h,
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
    println!("  EnvNlsCreate: rc={} env_h={:p}", rc, env_h);
    err_h = std::ptr::null_mut();
    svc_h = std::ptr::null_mut();
    srv_h = std::ptr::null_mut();
    if rc == 0 && !env_h.is_null() {
        unsafe {
            alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
            alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
            alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
        }
        let cs = b"localhost:2003/osrdb";
        let rc2 = unsafe { attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
        println!("  localhost attach: rc={}", rc2);
        if rc2 != 0 {
            let (c, m) = get_error(err_h);
            println!("  error: code={} '{}'", c, m);
        }
    }

    println!(
        "\n--- Attempt 2: 192.168.3.34 REUSING same env_h@{:p} ---",
        &env_h
    );
    // OVERWRITE env_h with new value — same STACK ADDRESS, new ACI handle
    env_h = std::ptr::null_mut();
    let rc = unsafe {
        env_nls(
            &mut env_h,
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
    println!("  EnvNlsCreate: rc={} env_h={:p}", rc, env_h);
    err_h = std::ptr::null_mut();
    svc_h = std::ptr::null_mut();
    srv_h = std::ptr::null_mut();
    if rc == 0 && !env_h.is_null() {
        unsafe {
            alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
            alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
            alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
        }
        let cs = b"192.168.3.34:2003/osrdb";
        let rc2 = unsafe { attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
        println!("  192.168.3.34 attach: rc={}", rc2);
        if rc2 == 0 {
            println!("  SAME-VARIABLE REUSE: SUCCESS!");
            let mut ses_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                attr_set(svc_h, 3, srv_h, 0, 6, err_h);
                alloc(env_h, &mut ses_h, 9, 0, std::ptr::null_mut());
                let u = b"sysdba";
                let p = b"szoscar55";
                attr_set(
                    ses_h,
                    9,
                    u.as_ptr() as *mut c_void,
                    u.len() as u32,
                    22,
                    err_h,
                );
                attr_set(
                    ses_h,
                    9,
                    p.as_ptr() as *mut c_void,
                    p.len() as u32,
                    23,
                    err_h,
                );
                attr_set(svc_h, 3, ses_h, 0, 7, err_h);
            }
            let rc3 = unsafe { sess_begin(svc_h, err_h, ses_h, 1, 0) };
            println!("  SessionBegin: rc={}", rc3);
        } else {
            let (c, m) = get_error(err_h);
            println!("  FAIL code={} '{}'", c, m);
        }
    }

    println!("\nDone.");
}

#[cfg(not(windows))]
fn run(_: Option<String>) {
    println!("Windows only");
}
