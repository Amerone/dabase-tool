/// Test: does creating TWO env handles (like ODPI) break ACIServerAttach?
/// This mimics ODPI's call sequence using raw FFI to isolate the issue.
///
/// ODPI creates:
///   1. Global env (charset=873/UTF8, mode=DPI_OCI_THREADED=1)  -- dpiGlobal__extendedInitialize
///   2. Connection env (charset=852/GBK → we test 871, mode=ACI_OBJECT=2)
///
/// Direct FFI creates only ONE env and succeeds. Does the global env break things?

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
        eprintln!("  GetProcAddress({}) failed", name);
        None
    } else {
        Some(ptr)
    }
}

fn main() {
    println!("=== Two-env ACI FFI test ===");
    println!("Tests if creating a 'global' env before the connection env breaks ACIServerAttach");

    #[cfg(windows)]
    {
        // Find aci.dll dir and set TNS_ADMIN
        let aci_dir = find_aci_dir();
        if let Some(ref d) = aci_dir {
            println!("Setting TNS_ADMIN = {}", d);
            std::env::set_var("TNS_ADMIN", d);
            // Also add aci dir to PATH so aci.dll can lazy-load its protocol DLLs (osntcp.dll etc.)
            let cur_path = std::env::var("PATH").unwrap_or_default();
            if !cur_path.split(';').any(|p| p.eq_ignore_ascii_case(d)) {
                println!("Adding aci dir to PATH");
                std::env::set_var("PATH", format!("{};{}", d, cur_path));
            }
        }

        test_two_envs(aci_dir);
    }

    #[cfg(not(windows))]
    println!("Windows only");
}

fn find_aci_dir() -> Option<String> {
    for dir in &["drivers/shentong/windows", "../drivers/shentong/windows"] {
        if std::path::Path::new(dir).join("aci.dll").exists() {
            if let Ok(abs) = std::fs::canonicalize(dir) {
                let s = abs.to_string_lossy().into_owned();
                let s = s.trim_start_matches(r"\\?\").to_string();
                return Some(s);
            }
        }
    }
    None
}

#[cfg(windows)]
fn test_two_envs(aci_dir: Option<String>) {
    let dll_path = match aci_dir {
        Some(ref d) => format!("{}\\aci.dll\0", d),
        None => "aci.dll\0".to_string(),
    };

    println!("Loading: {}", dll_path.trim_end_matches('\0'));
    let module = unsafe { LoadLibraryA(dll_path.as_ptr()) };
    if module.is_null() {
        eprintln!("LoadLibraryA failed!");
        return;
    }
    println!("DLL loaded: {:p}", module);

    // ACI constants
    const ACI_HTYPE_ERROR: u32 = 2;
    const ACI_HTYPE_SVCCTX: u32 = 3;
    const ACI_HTYPE_SERVER: u32 = 8;
    const ACI_HTYPE_SESSION: u32 = 9;
    const ACI_ATTR_USERNAME: u32 = 22;
    const ACI_ATTR_PASSWORD: u32 = 23;
    const ACI_ATTR_SERVER: u32 = 6;
    const ACI_ATTR_SESSION: u32 = 7;
    const ACI_CRED_RDBMS: u32 = 1;
    const ACI_DEFAULT: u32 = 0;
    const ACI_OBJECT: u32 = 2;
    const ACI_THREADED: u32 = 1;
    const ACI_CHARSET_UTF8: u16 = 871;
    const ACI_CHARSET_UTF8_ODPI: u16 = 873; // ODPI's UTF-8 charset ID

    type EnvNlsCreate = unsafe extern "C" fn(
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
    type HandleAlloc =
        unsafe extern "C" fn(*mut c_void, *mut *mut c_void, u32, usize, *mut *mut c_void) -> i32;
    type HandleFree = unsafe extern "C" fn(*mut c_void, u32) -> i32;
    type ServerAttach = unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, i32, u32) -> i32;
    type AttrSet =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32, u32, *mut c_void) -> i32;
    type SessionBegin =
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32) -> i32;
    type AttrGet =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut u32, u32, *mut c_void) -> i32;
    type NlsNumericInfoGet = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut i32, u32) -> i32;
    type ErrorGet =
        unsafe extern "C" fn(*mut c_void, u32, *const u8, *mut i32, *mut u8, u32, u32) -> i32;

    let env_nls: EnvNlsCreate =
        unsafe { std::mem::transmute(get_fn(module, "ACIEnvNlsCreate").unwrap()) };
    let handle_alloc: HandleAlloc =
        unsafe { std::mem::transmute(get_fn(module, "ACIHandleAlloc").unwrap()) };
    let handle_free: HandleFree =
        unsafe { std::mem::transmute(get_fn(module, "ACIHandleFree").unwrap()) };
    let server_attach: ServerAttach =
        unsafe { std::mem::transmute(get_fn(module, "ACIServerAttach").unwrap()) };
    let attr_set: AttrSet = unsafe { std::mem::transmute(get_fn(module, "ACIAttrSet").unwrap()) };
    let session_begin: SessionBegin =
        unsafe { std::mem::transmute(get_fn(module, "ACISessionBegin").unwrap()) };
    let attr_get: AttrGet = unsafe { std::mem::transmute(get_fn(module, "ACIAttrGet").unwrap()) };
    let nls_info: NlsNumericInfoGet =
        unsafe { std::mem::transmute(get_fn(module, "ACINlsNumericInfoGet").unwrap()) };
    let error_get: ErrorGet =
        unsafe { std::mem::transmute(get_fn(module, "ACIErrorGet").unwrap()) };

    let get_error = |err_h: *mut c_void| {
        let mut buf = vec![0u8; 512];
        let mut code: i32 = 0;
        unsafe {
            error_get(
                err_h,
                1,
                std::ptr::null(),
                &mut code,
                buf.as_mut_ptr(),
                512,
                ACI_HTYPE_ERROR,
            );
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        (code, String::from_utf8_lossy(&buf[..end]).to_string())
    };

    let connect_str = "192.168.3.34:2003/osrdb";

    // === Test 1: One env (like direct FFI - known to work) ===
    println!("\n--- Test 1: ONE env (charset=871, mode=ACI_OBJECT) ---");
    {
        let mut env_h: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env_h,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!("  EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}", rc, env_h);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
            }
            unsafe {
                handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach: rc={}", rc2);
            if rc2 == 0 {
                println!("  ONE ENV: SUCCESS");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  ONE ENV: FAIL code={} msg={}", code, msg);
            }
            unsafe {
                handle_free(env_h, 1);
            } // 1 = ACI_HTYPE_ENV
        }
    }

    // === Test 2: Global env FIRST (charset=873, mode=THREADED), then connection env ===
    println!("\n--- Test 2: GLOBAL env (873, THREADED) + connection env (871, ACI_OBJECT) ---");
    {
        // Create global env like ODPI does
        let mut global_env: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut global_env,
                ACI_THREADED,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8_ODPI,
                ACI_CHARSET_UTF8_ODPI,
            )
        };
        println!(
            "  Global EnvNlsCreate(873, THREADED): rc={} env={:p}",
            rc, global_env
        );
        if rc != 0 || global_env.is_null() {
            println!("  Global env creation failed!");
        }
        // Allocate global error handle (like ODPI does)
        let mut global_err: *mut c_void = std::ptr::null_mut();
        if !global_env.is_null() {
            unsafe {
                handle_alloc(
                    global_env,
                    &mut global_err,
                    ACI_HTYPE_ERROR,
                    0,
                    std::ptr::null_mut(),
                );
            }
            println!("  Global error handle: {:p}", global_err);
        }

        // Now create connection env
        let mut conn_env: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut conn_env,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!(
            "  Conn EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}",
            rc, conn_env
        );
        if rc == 0 && !conn_env.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(
                    conn_env,
                    &mut err_h,
                    ACI_HTYPE_ERROR,
                    0,
                    std::ptr::null_mut(),
                );
            }

            // Extra calls like ODPI does (ACIAttrGet for charset, ACINlsNumericInfoGet)
            let mut charset_id: u16 = 0;
            let rc_attr = unsafe {
                attr_get(
                    conn_env,
                    1, /*HTYPE_ENV*/
                    &mut charset_id as *mut u16 as *mut c_void,
                    std::ptr::null_mut(),
                    31, /*OCI_ATTR_CHARSET_ID*/
                    err_h,
                )
            };
            println!(
                "  ACIAttrGet(CHARSET_ID): rc={} charsetId={}",
                rc_attr, charset_id
            );
            let mut max_bytes: i32 = 0;
            let rc_nls = unsafe {
                nls_info(
                    conn_env,
                    err_h,
                    &mut max_bytes,
                    91, /*NLS_CHARSET_MAXBYTESZ*/
                )
            };
            println!(
                "  ACINlsNumericInfoGet: rc={} maxBytes={}",
                rc_nls, max_bytes
            );

            unsafe {
                handle_alloc(
                    conn_env,
                    &mut srv_h,
                    ACI_HTYPE_SERVER,
                    0,
                    std::ptr::null_mut(),
                );
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach: rc={}", rc2);
            if rc2 == 0 {
                println!("  TWO ENVS (THREADED first): SUCCESS");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  TWO ENVS: FAIL code={} msg={}", code, msg);
            }
        }
    }

    // === Test 3: Global env FIRST (charset=873, mode=THREADED|OBJECT), then connection env ===
    println!(
        "\n--- Test 3: GLOBAL env (873, THREADED|OBJECT) + connection env (871, ACI_OBJECT) ---"
    );
    {
        let mut global_env: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut global_env,
                ACI_THREADED | ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8_ODPI,
                ACI_CHARSET_UTF8_ODPI,
            )
        };
        println!(
            "  Global EnvNlsCreate(873, THREADED|OBJECT): rc={} env={:p}",
            rc, global_env
        );

        let mut conn_env: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut conn_env,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!(
            "  Conn EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}",
            rc, conn_env
        );
        if rc == 0 && !conn_env.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(
                    conn_env,
                    &mut err_h,
                    ACI_HTYPE_ERROR,
                    0,
                    std::ptr::null_mut(),
                );
            }
            unsafe {
                handle_alloc(
                    conn_env,
                    &mut srv_h,
                    ACI_HTYPE_SERVER,
                    0,
                    std::ptr::null_mut(),
                );
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach: rc={}", rc2);
            if rc2 == 0 {
                println!("  SUCCESS");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  FAIL code={} msg={}", code, msg);
            }
        }
    }

    // === Test 4: Mirror test_aci_ffi exactly — allocate SVCCTX before SERVER, no extra attr calls ===
    println!(
        "\n--- Test 4: ONE env, allocate SVCCTX BEFORE SERVER (mirrors working test_aci_ffi) ---"
    );
    {
        let mut env_h: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env_h,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!("  EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}", rc, env_h);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
            }
            unsafe {
                handle_alloc(env_h, &mut svc_h, ACI_HTYPE_SVCCTX, 0, std::ptr::null_mut());
            } // SVCCTX first!
            unsafe {
                handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach: rc={}", rc2);
            if rc2 == 0 {
                println!("  Test4 (SVCCTX before SERVER, no extra calls): SUCCESS");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  Test4 FAIL code={} msg={}", code, msg);
            }
        }
    }

    // === Test 5: ONE env, allocate SVCCTX BEFORE SERVER, WITH extra ACIAttrGet/NlsInfo calls ===
    println!("\n--- Test 5: ONE env, SVCCTX before SERVER + ACIAttrGet + ACINlsNumericInfoGet ---");
    {
        let mut env_h: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env_h,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!("  EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}", rc, env_h);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
            }
            unsafe {
                handle_alloc(env_h, &mut svc_h, ACI_HTYPE_SVCCTX, 0, std::ptr::null_mut());
            }
            // Extra calls like ODPI does
            let mut charset_id: u16 = 0;
            let rc_attr = unsafe {
                attr_get(
                    env_h,
                    1, /*HTYPE_ENV*/
                    &mut charset_id as *mut u16 as *mut c_void,
                    std::ptr::null_mut(),
                    31, /*ACI_ATTR_CHARSET_ID*/
                    err_h,
                )
            };
            println!(
                "  ACIAttrGet(CHARSET_ID): rc={} charsetId={}",
                rc_attr, charset_id
            );
            let mut max_bytes: i32 = 0;
            let rc_nls = unsafe { nls_info(env_h, err_h, &mut max_bytes, 91) };
            println!(
                "  ACINlsNumericInfoGet: rc={} maxBytes={}",
                rc_nls, max_bytes
            );
            unsafe {
                handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach: rc={}", rc2);
            if rc2 == 0 {
                println!("  Test5 (SVCCTX first + extra calls): SUCCESS");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  Test5 FAIL code={} msg={}", code, msg);
            }
        }
    }

    // === Test 6: ONE env, NO SVCCTX, no extra calls (minimal) ===
    println!("\n--- Test 6: ONE env, NO SVCCTX, no extra calls (minimal) ---");
    {
        let mut env_h: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env_h,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!("  EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}", rc, env_h);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
            }
            unsafe {
                handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach: rc={}", rc2);
            if rc2 == 0 {
                println!("  Test6 (no SVCCTX, no extra): SUCCESS");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  Test6 FAIL code={} msg={}", code, msg);
            }
        }
    }

    // === Test 7: "Priming" hypothesis — try localhost FIRST (fail), then 192.168.3.34 in NEW env ===
    // In test_aci_ffi.rs the successful 192.168.3.34 attempt comes AFTER a failed localhost attempt.
    // Does the failed attempt "prime" ACI's internal state?
    println!("\n--- Test 7: Prime with localhost attempt, then try 192.168.3.34 in fresh env ---");
    {
        // Priming attempt: localhost (expected to fail)
        let prime_str = b"localhost:2003/osrdb";
        let mut env_prime: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env_prime,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        if rc == 0 && !env_prime.is_null() {
            let mut err_prime: *mut c_void = std::ptr::null_mut();
            let mut svc_prime: *mut c_void = std::ptr::null_mut();
            let mut srv_prime: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(
                    env_prime,
                    &mut err_prime,
                    ACI_HTYPE_ERROR,
                    0,
                    std::ptr::null_mut(),
                );
            }
            unsafe {
                handle_alloc(
                    env_prime,
                    &mut svc_prime,
                    ACI_HTYPE_SVCCTX,
                    0,
                    std::ptr::null_mut(),
                );
            }
            unsafe {
                handle_alloc(
                    env_prime,
                    &mut srv_prime,
                    ACI_HTYPE_SERVER,
                    0,
                    std::ptr::null_mut(),
                );
            }
            let rc2 = unsafe {
                server_attach(
                    srv_prime,
                    err_prime,
                    prime_str.as_ptr(),
                    prime_str.len() as i32,
                    ACI_DEFAULT,
                )
            };
            println!(
                "  Prime localhost ACIServerAttach: rc={} (expected -1)",
                rc2
            );
            // Call ACIErrorGet like test_aci_ffi does after a failure
            if rc2 != 0 {
                let (code, msg) = get_error(err_prime);
                println!("  Prime error: code={} msg={}", code, msg);
            }
            // NOTE: do NOT free — same as test_aci_ffi loop (handles leak)
        }

        // Now try 192.168.3.34 in a brand new env (exactly like test_aci_ffi.rs 2nd iteration)
        let mut env_h: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            env_nls(
                &mut env_h,
                ACI_OBJECT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                ACI_CHARSET_UTF8,
                ACI_CHARSET_UTF8,
            )
        };
        println!("  EnvNlsCreate(871, ACI_OBJECT): rc={} env={:p}", rc, env_h);
        if rc == 0 && !env_h.is_null() {
            let mut err_h: *mut c_void = std::ptr::null_mut();
            let mut svc_h: *mut c_void = std::ptr::null_mut();
            let mut srv_h: *mut c_void = std::ptr::null_mut();
            unsafe {
                handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
            }
            unsafe {
                handle_alloc(env_h, &mut svc_h, ACI_HTYPE_SVCCTX, 0, std::ptr::null_mut());
            }
            unsafe {
                handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
            }
            let cs = connect_str.as_bytes();
            let rc2 =
                unsafe { server_attach(srv_h, err_h, cs.as_ptr(), cs.len() as i32, ACI_DEFAULT) };
            println!("  ACIServerAttach(192.168.3.34): rc={}", rc2);
            if rc2 == 0 {
                println!("  Test7 PRIMING SUCCESS: localhost failure enabled 192.168.3.34!");
            } else {
                let (code, msg) = get_error(err_h);
                println!("  Test7 FAIL code={} msg={}", code, msg);
            }
        }
    }

    println!("\nDone.");
}
