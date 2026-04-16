/// Test: does splitting ACI calls across different stack frames cause failure?
/// ODPI splits: dpiOci__envNlsCreate (frame1) → dpiOci__handleAlloc (frame2) → dpiOci__serverAttach (frame3)
/// Direct FFI does all in one frame → works
///
/// This test compares:
/// A) All calls in one function (like do_attach) → expect SUCCESS
/// B) Calls split across separate functions → expect FAIL?

#[cfg(windows)]
use std::ffi::{c_void, CString};

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
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
type FnAttrSet = unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32, u32, *mut c_void) -> i32;
type FnSessBegin = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32) -> i32;
type FnErrGet =
    unsafe extern "C" fn(*mut c_void, u32, *const u8, *mut i32, *mut u8, u32, u32) -> i32;

// --- ODPI-style: each call in its own function (like dpiOci__XXX wrappers) ---
#[cfg(windows)]
fn odpi_env_create(f: FnEnvNls, out: *mut *mut c_void) -> i32 {
    unsafe {
        f(
            out,
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
    }
}

// --- LIKE test_aci_ptr.rs do_attach: all ACI calls in ONE function ---
#[cfg(windows)]
fn do_attach_all(
    env_nls: FnEnvNls,
    alloc: FnAlloc,
    attach: FnAttach,
    cs_ptr: *const u8,
    cs_len: usize,
    label: &str,
) -> i32 {
    let mut env: *mut c_void = std::ptr::null_mut();
    let mut err: *mut c_void = std::ptr::null_mut();
    let mut svc: *mut c_void = std::ptr::null_mut();
    let mut srv: *mut c_void = std::ptr::null_mut();
    unsafe {
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
    unsafe {
        alloc(env, &mut err, 2, 0, std::ptr::null_mut());
    }
    unsafe {
        alloc(env, &mut svc, 3, 0, std::ptr::null_mut());
    }
    unsafe {
        alloc(env, &mut srv, 8, 0, std::ptr::null_mut());
    }
    let rc = unsafe { attach(srv, err, cs_ptr, cs_len as i32, 0) };
    println!("  [{}] attach: rc={}", label, rc);
    rc
}
#[cfg(windows)]
fn odpi_handle_alloc(f: FnAlloc, env: *mut c_void, out: *mut *mut c_void, htype: u32) -> i32 {
    unsafe { f(env, out, htype, 0, std::ptr::null_mut()) }
}
#[cfg(windows)]
fn odpi_server_attach(f: FnAttach, srv: *mut c_void, err: *mut c_void, cs: &[u8]) -> i32 {
    unsafe { f(srv, err, cs.as_ptr(), cs.len() as i32, 0) }
}

fn main() {
    let aci_dir = find_aci_dir();
    if let Some(ref d) = aci_dir {
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

    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let c = CString::new($name).unwrap();
            unsafe {
                std::mem::transmute::<*mut c_void, $ty>(GetProcAddress(
                    module,
                    c.as_ptr() as *const u8,
                ))
            }
        }};
    }
    let env_nls: FnEnvNls = sym!("ACIEnvNlsCreate", FnEnvNls);
    let alloc: FnAlloc = sym!("ACIHandleAlloc", FnAlloc);
    let attach: FnAttach = sym!("ACIServerAttach", FnAttach);
    let attr_set: FnAttrSet = sym!("ACIAttrSet", FnAttrSet);
    let sess_begin: FnSessBegin = sym!("ACISessionBegin", FnSessBegin);
    let err_get: FnErrGet = sym!("ACIErrorGet", FnErrGet);

    let print_err = |err: *mut c_void| {
        let mut buf = vec![0u8; 256];
        let mut code = 0i32;
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
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        println!("  code={} '{}'", code, String::from_utf8_lossy(&buf[..n]));
    };

    let cs = b"192.168.3.34:2003/osrdb";

    // === Method 0: do_attach_all localhost priming — SAME FUNCTION as Method C ===
    println!("\n--- Method 0b: do_attach_all localhost priming (same function as Method C) ---");
    {
        let lh = b"localhost:2003/osrdb";
        do_attach_all(
            env_nls,
            alloc,
            attach,
            lh.as_ptr(),
            lh.len(),
            "prime-via-do-attach",
        );
    }

    // === Method 0: original inline priming ===
    println!("\n--- Method 0: do_attach localhost (priming, should fail) ---");
    {
        let mut env: *mut c_void = std::ptr::null_mut();
        let mut err: *mut c_void = std::ptr::null_mut();
        let mut svc: *mut c_void = std::ptr::null_mut();
        let mut srv: *mut c_void = std::ptr::null_mut();
        unsafe {
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
        unsafe {
            alloc(env, &mut err, 2, 0, std::ptr::null_mut());
            alloc(env, &mut svc, 3, 0, std::ptr::null_mut());
            alloc(env, &mut srv, 8, 0, std::ptr::null_mut());
        }
        let lh = b"localhost:2003/osrdb";
        let rc = unsafe { attach(srv, err, lh.as_ptr(), lh.len() as i32, 0) };
        println!("  localhost attach: rc={}", rc);
        if rc != 0 {
            print_err(err);
        }
    }

    // === Method A: All calls in ONE function frame (direct_connect) ===
    println!("\n--- Method A: all in one function frame AFTER priming ---");
    {
        let mut env: *mut c_void = std::ptr::null_mut();
        let mut err: *mut c_void = std::ptr::null_mut();
        let mut svc: *mut c_void = std::ptr::null_mut();
        let mut srv: *mut c_void = std::ptr::null_mut();
        unsafe {
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
        unsafe {
            alloc(env, &mut err, 2, 0, std::ptr::null_mut());
        }
        unsafe {
            alloc(env, &mut svc, 3, 0, std::ptr::null_mut());
        }
        unsafe {
            alloc(env, &mut srv, 8, 0, std::ptr::null_mut());
        }
        let rc = unsafe { attach(srv, err, cs.as_ptr(), cs.len() as i32, 0) };
        println!("  attach: rc={}", rc);
        if rc != 0 {
            print_err(err);
        } else {
            let mut ses: *mut c_void = std::ptr::null_mut();
            unsafe {
                attr_set(svc, 3, srv, 0, 6, err);
                alloc(env, &mut ses, 9, 0, std::ptr::null_mut());
            }
            unsafe {
                let u = b"sysdba";
                let p = b"szoscar55";
                attr_set(ses, 9, u.as_ptr() as *mut c_void, u.len() as u32, 22, err);
                attr_set(ses, 9, p.as_ptr() as *mut c_void, p.len() as u32, 23, err);
                attr_set(svc, 3, ses, 0, 7, err);
            }
            let rc2 = unsafe { sess_begin(svc, err, ses, 1, 0) };
            println!(
                "  SessionBegin: rc={} => {}",
                rc2,
                if rc2 == 0 { "SUCCESS" } else { "FAIL" }
            );
        }
    }

    // === Method B: ODPI-style - each in separate wrapper function ===
    println!("\n--- Method B: ODPI-style, each call in separate function ---");
    {
        let mut env: *mut c_void = std::ptr::null_mut();
        let mut err: *mut c_void = std::ptr::null_mut();
        let mut svc: *mut c_void = std::ptr::null_mut();
        let mut srv: *mut c_void = std::ptr::null_mut();
        odpi_env_create(env_nls, &mut env);
        odpi_handle_alloc(alloc, env, &mut err, 2);
        odpi_handle_alloc(alloc, env, &mut svc, 3);
        odpi_handle_alloc(alloc, env, &mut srv, 8);
        let rc = odpi_server_attach(attach, srv, err, cs);
        println!("  attach: rc={}", rc);
        if rc != 0 {
            print_err(err);
        } else {
            let mut ses: *mut c_void = std::ptr::null_mut();
            unsafe {
                attr_set(svc, 3, srv, 0, 6, err);
                alloc(env, &mut ses, 9, 0, std::ptr::null_mut());
            }
            unsafe {
                let u = b"sysdba";
                let p = b"szoscar55";
                attr_set(ses, 9, u.as_ptr() as *mut c_void, u.len() as u32, 22, err);
                attr_set(ses, 9, p.as_ptr() as *mut c_void, p.len() as u32, 23, err);
                attr_set(svc, 3, ses, 0, 7, err);
            }
            let rc2 = unsafe { sess_begin(svc, err, ses, 1, 0) };
            println!(
                "  SessionBegin: rc={} => {}",
                rc2,
                if rc2 == 0 { "SUCCESS" } else { "FAIL" }
            );
        }
    }

    // === Method C: same as test_aci_ptr.rs do_attach approach ===
    println!("\n--- Method C: call do_attach_all function directly ---");
    {
        let cs = b"192.168.3.34:2003/osrdb";
        do_attach_all(env_nls, alloc, attach, cs.as_ptr(), cs.len(), "method-C");
    }

    println!("\nDone.");
}
#[cfg(not(windows))]
fn run(_: Option<String>) {}
