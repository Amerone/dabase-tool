/// Test: does any for-loop (range 0..1) fix it, or is it specific to slice for-loops?

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
fn do_connect(
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
    attr_set: unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32, u32, *mut c_void) -> i32,
    session_begin: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32) -> i32,
    error_get: unsafe extern "C" fn(
        *mut c_void,
        u32,
        *const u8,
        *mut i32,
        *mut u8,
        u32,
        u32,
    ) -> i32,
    label: &str,
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
    println!("  [{}] EnvNlsCreate: rc={} env={:p}", label, rc, env_h);
    if rc != 0 || env_h.is_null() {
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

    let cs = b"192.168.3.34:2003/osrdb";
    let rc = unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
    println!("  [{}] ACIServerAttach: rc={}", label, rc);
    if rc != 0 {
        let mut buf = vec![0u8; 256];
        let mut code = 0i32;
        unsafe {
            error_get(
                err_h,
                1,
                std::ptr::null(),
                &mut code,
                buf.as_mut_ptr(),
                256,
                2,
            );
        }
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        println!(
            "  [{}] Error: code={} msg={}",
            label,
            code,
            String::from_utf8_lossy(&buf[..n])
        );
        return;
    }

    unsafe {
        attr_set(svc_h, 3, srv_h, 0, 6, err_h);
    }
    let mut ses_h: *mut c_void = std::ptr::null_mut();
    unsafe {
        handle_alloc(env_h, &mut ses_h, 9, 0, std::ptr::null_mut());
    }
    let user = b"sysdba";
    let pwd = b"szoscar55";
    unsafe {
        attr_set(
            ses_h,
            9,
            user.as_ptr() as *mut c_void,
            user.len() as u32,
            22,
            err_h,
        );
        attr_set(
            ses_h,
            9,
            pwd.as_ptr() as *mut c_void,
            pwd.len() as u32,
            23,
            err_h,
        );
        attr_set(svc_h, 3, ses_h, 0, 7, err_h);
    }
    let rc = unsafe { session_begin(svc_h, err_h, ses_h, 1, 0) };
    println!(
        "  [{}] SessionBegin: rc={} => {}",
        label,
        rc,
        if rc == 0 { "SUCCESS" } else { "FAIL" }
    );
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
    let attr_set: FnAttrSet = unsafe { std::mem::transmute(get_fn(module, "ACIAttrSet").unwrap()) };
    let session_begin: FnSessBegin =
        unsafe { std::mem::transmute(get_fn(module, "ACISessionBegin").unwrap()) };
    let error_get: FnErrGet =
        unsafe { std::mem::transmute(get_fn(module, "ACIErrorGet").unwrap()) };

    // Test A: range for-loop for _ in 0..1
    println!("\n--- Test A: range loop (for _ in 0..1) ---");
    for _ in 0..1 {
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
        println!("  EnvNlsCreate: rc={} env={:p}", rc, env_h);
        if rc != 0 || env_h.is_null() {
            break;
        }
        let mut err_h: *mut c_void = std::ptr::null_mut();
        let mut svc_h: *mut c_void = std::ptr::null_mut();
        let mut srv_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
            handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
            handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
        }
        let cs = b"192.168.3.34:2003/osrdb";
        let rc = unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
        println!("  ACIServerAttach: rc={}", rc);
        if rc != 0 {
            let mut buf = vec![0u8; 256];
            let mut code = 0i32;
            unsafe {
                error_get(
                    err_h,
                    1,
                    std::ptr::null(),
                    &mut code,
                    buf.as_mut_ptr(),
                    256,
                    2,
                );
            }
            let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            println!(
                "  Error: code={} msg={}",
                code,
                String::from_utf8_lossy(&buf[..n])
            );
        } else {
            println!("  SUCCESS branch entered");
        }
    }

    // Test B: while loop (while counter < 1)
    println!("\n--- Test B: while loop ---");
    {
        let mut once = true;
        while once {
            once = false;
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
            println!("  EnvNlsCreate: rc={} env={:p}", rc, env_h);
            if rc != 0 || env_h.is_null() {
                break;
            }
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
            }
            let cs = b"192.168.3.34:2003/osrdb";
            let rc = unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
            println!("  ACIServerAttach: rc={}", rc);
            if rc != 0 {
                let mut buf = vec![0u8; 256];
                let mut code = 0i32;
                unsafe {
                    error_get(
                        err_h,
                        1,
                        std::ptr::null(),
                        &mut code,
                        buf.as_mut_ptr(),
                        256,
                        2,
                    );
                }
                let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                println!(
                    "  Error: code={} msg={}",
                    code,
                    String::from_utf8_lossy(&buf[..n])
                );
            } else {
                println!("  SUCCESS branch entered");
            }
        }
    }

    // Test C: plain block (baseline FAIL)
    println!("\n--- Test C: plain block (baseline) ---");
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
        println!("  EnvNlsCreate: rc={} env={:p}", rc, env_h);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, 2, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut svc_h, 3, 0, std::ptr::null_mut());
                handle_alloc(env_h, &mut srv_h, 8, 0, std::ptr::null_mut());
            }
            let cs = b"192.168.3.34:2003/osrdb";
            let rc = unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, 0) };
            println!("  ACIServerAttach: rc={}", rc);
            if rc != 0 {
                let mut buf = vec![0u8; 256];
                let mut code = 0i32;
                unsafe {
                    error_get(
                        err_h,
                        1,
                        std::ptr::null(),
                        &mut code,
                        buf.as_mut_ptr(),
                        256,
                        2,
                    );
                }
                let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                println!(
                    "  Error: code={} msg={}",
                    code,
                    String::from_utf8_lossy(&buf[..n])
                );
            } else {
                println!("  SUCCESS branch entered");
            }
        }
    }

    // Test D: do_connect function (like do_attach in test_aci_ptr.rs)
    println!("\n--- Test D: via do_connect function ---");
    do_connect(
        env_nls,
        handle_alloc,
        server_attach,
        attr_set,
        session_begin,
        error_get,
        "D",
    );

    println!("\nDone.");
}

#[cfg(not(windows))]
fn test(_: Option<String>) {
    println!("Windows only");
}
