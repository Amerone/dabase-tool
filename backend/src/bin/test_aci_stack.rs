/// ACI priming: confirm that keeping localhost handles ALIVE on stack is the key
/// vs returning false from a closure (stack frame popped)

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
    let ptr = unsafe { GetProcAddress(module, cname.as_ptr() as *const u8) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

fn main() {
    println!("=== ACI stack-alive test ===");
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

    // --- Test A: localhost attempt then 192.168.3.34, but handles kept ALIVE on stack ---
    println!("\n--- Test A: localhost + 192.168.3.34, handles ALIVE on stack (like for loop) ---");
    {
        // localhost - these variables stay ALIVE in this block
        let mut env1: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env1,
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
        let mut err1: *mut c_void = std::ptr::null_mut();
        let mut svc1: *mut c_void = std::ptr::null_mut();
        let mut srv1: *mut c_void = std::ptr::null_mut();
        if rc == 0 && !env1.is_null() {
            unsafe {
                alloc(env1, &mut err1, 2, 0, std::ptr::null_mut());
                alloc(env1, &mut svc1, 3, 0, std::ptr::null_mut());
                alloc(env1, &mut srv1, 8, 0, std::ptr::null_mut());
            }
            let cs = b"localhost:2003/osrdb";
            let rc2 = unsafe { attach(srv1, err1, cs.as_ptr(), cs.len() as i32, 0) };
            println!("  localhost attach: rc={}", rc2);
            if rc2 != 0 {
                let (c, m) = get_error(err1);
                println!("  Prime error: code={} '{}'", c, m);
            }
        }

        // Now 192.168.3.34 with a NEW env — but env1/err1/svc1/srv1 still ALIVE here!
        let mut env2: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env2,
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
        println!("  EnvNlsCreate(192): rc={}", rc);
        if rc == 0 && !env2.is_null() {
            let mut err2: *mut c_void = std::ptr::null_mut();
            let mut svc2: *mut c_void = std::ptr::null_mut();
            let mut srv2: *mut c_void = std::ptr::null_mut();
            unsafe {
                alloc(env2, &mut err2, 2, 0, std::ptr::null_mut());
                alloc(env2, &mut svc2, 3, 0, std::ptr::null_mut());
                alloc(env2, &mut srv2, 8, 0, std::ptr::null_mut());
            }
            let cs = b"192.168.3.34:2003/osrdb";
            let rc2 = unsafe { attach(srv2, err2, cs.as_ptr(), cs.len() as i32, 0) };
            println!("  192.168.3.34 attach: rc={}", rc2);
            if rc2 == 0 {
                println!("  Test A: SUCCESS (handles alive)");
                // Session
                let mut ses: *mut c_void = std::ptr::null_mut();
                unsafe {
                    attr_set(svc2, 3, srv2, 0, 6, err2);
                    alloc(env2, &mut ses, 9, 0, std::ptr::null_mut());
                    let u = b"sysdba";
                    let p = b"szoscar55";
                    attr_set(ses, 9, u.as_ptr() as *mut c_void, u.len() as u32, 22, err2);
                    attr_set(ses, 9, p.as_ptr() as *mut c_void, p.len() as u32, 23, err2);
                    attr_set(svc2, 3, ses, 0, 7, err2);
                }
                let rc3 = unsafe { sess_begin(svc2, err2, ses, 1, 0) };
                println!("  SessionBegin: rc={}", rc3);
            } else {
                let (c, m) = get_error(err2);
                println!("  Test A FAIL: code={} '{}'", c, m);
            }
        }
        // env1,err1,svc1,srv1 still in scope here — this is the key!
        let _ = (env1, err1, svc1, srv1); // keep alive explicitly
    }

    println!("\nDone.");
}

#[cfg(not(windows))]
fn run(_: Option<String>) {
    println!("Windows only");
}
