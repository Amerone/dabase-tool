/// Isolation: does a pointer to .rodata on the stack matter?
/// Test: create a &str reference on the stack before calling ACIServerAttach

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
    let aci_dir = find_aci_dir();
    if let Some(ref d) = aci_dir {
        std::env::set_var("TNS_ADMIN", d);
    }
    #[cfg(windows)]
    test(aci_dir);
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
fn attempt(
    label: &str,
    env_nls: unsafe extern "C" fn(
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
    ) -> i32,
    handle_alloc: unsafe extern "C" fn(
        *mut c_void,
        *mut *mut c_void,
        u32,
        usize,
        *mut *mut c_void,
    ) -> i32,
    server_attach: unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, i32, u32) -> i32,
    cs_ptr: *const u8,
    cs_len: usize,
) {
    let mut env_h: *mut c_void = std::ptr::null_mut();
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
            871u16,
            871u16,
        )
    };
    if rc != 0 || env_h.is_null() {
        println!("  [{}] EnvNlsCreate FAILED rc={}", label, rc);
        return;
    }
    let mut err_h: *mut c_void = std::ptr::null_mut();
    let mut svc_h: *mut c_void = std::ptr::null_mut();
    let mut srv_h: *mut c_void = std::ptr::null_mut();
    unsafe {
        handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
        handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
        handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
    }
    let rc = unsafe { server_attach(srv_h, err_h, cs_ptr, cs_len as i32, 0) };
    println!("  [{}] ACIServerAttach: rc={}", label, rc);
}

#[cfg(windows)]
fn test(aci_dir: Option<String>) {
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
    let handle_alloc: FnAlloc =
        unsafe { std::mem::transmute(get_fn(module, "ACIHandleAlloc").unwrap()) };
    let server_attach: FnAttach =
        unsafe { std::mem::transmute(get_fn(module, "ACIServerAttach").unwrap()) };
    let _attr_set: FnAttrSet =
        unsafe { std::mem::transmute(get_fn(module, "ACIAttrSet").unwrap()) };
    let _session_begin: FnSessBegin =
        unsafe { std::mem::transmute(get_fn(module, "ACISessionBegin").unwrap()) };
    let _error_get: FnErrGet =
        unsafe { std::mem::transmute(get_fn(module, "ACIErrorGet").unwrap()) };

    let cs = b"192.168.3.34:2003/osrdb";

    // Test A: plain block (baseline FAIL)
    println!("\n--- Test A: plain block (baseline FAIL expected) ---");
    {
        let mut env_h: *mut c_void = std::ptr::null_mut();
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
                871u16,
                871u16,
            )
        };
        println!("  EnvNlsCreate: rc={}", rc);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
            }
            let rc = unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
            println!("  ACIServerAttach: rc={}", rc);
        }
    }

    // Test B: with a &str array reference on the stack (like slice iter)
    println!("\n--- Test B: &str array ref on stack before call ---");
    {
        // This creates a slice reference on the stack (pointer to .rodata)
        let _strs: &[&str] = &["192.168.3.34:2003/osrdb"];
        let mut env_h: *mut c_void = std::ptr::null_mut();
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
                871u16,
                871u16,
            )
        };
        println!("  EnvNlsCreate: rc={}", rc);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
            }
            let _ = _strs; // keep alive
            let rc = unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
            println!("  ACIServerAttach: rc={}", rc);
        }
    }

    // Test C: attempt() helper with str slice from for-loop element
    println!("\n--- Test C: for x in &[str], pass x.as_bytes() to attempt() ---");
    for connect_str in &["192.168.3.34:2003/osrdb"] {
        let bytes = connect_str.as_bytes();
        attempt(
            "C",
            env_nls,
            handle_alloc,
            server_attach,
            bytes.as_ptr(),
            bytes.len(),
        );
    }

    // Test D: attempt() helper with plain byte literal
    println!("\n--- Test D: attempt() with byte literal directly ---");
    attempt(
        "D",
        env_nls,
        handle_alloc,
        server_attach,
        cs.as_ptr(),
        cs.len(),
    );

    // Test E: for-loop (known working) - confirm
    println!("\n--- Test E: for-slice (confirm working) ---");
    for connect_str in &["192.168.3.34:2003/osrdb"] {
        println!("  Trying: {}", connect_str);
        let mut env_h: *mut c_void = std::ptr::null_mut();
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
                871u16,
                871u16,
            )
        };
        println!("  EnvNlsCreate: rc={}", rc);
        if rc != 0 || env_h.is_null() {
            continue;
        }
        let mut err_h: *mut c_void = std::ptr::null_mut();
        let mut svc_h: *mut c_void = std::ptr::null_mut();
        let mut srv_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
            handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
            handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
        }
        let cs_bytes = connect_str.as_bytes();
        let rc =
            unsafe { server_attach(srv_h, err_h, cs_bytes.as_ptr(), cs_bytes.len() as i32, 0) };
        println!("  ACIServerAttach: rc={}", rc);
    }

    println!("\nDone.");
}

#[cfg(not(windows))]
fn test(_: Option<String>) {
    println!("Windows only");
}
