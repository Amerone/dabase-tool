/// ACI clone test: exact copy of test_aci_ffi loop, but WITHOUT the path setup
/// to isolate if path/env matters vs code structure

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
    println!("=== ACI clone test ===");
    let aci_dir = find_aci_dir();
    if let Some(ref d) = aci_dir {
        println!("TNS_ADMIN = {}", d);
        std::env::set_var("TNS_ADMIN", d);
    }
    #[cfg(windows)]
    test(aci_dir);
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
    type ServerAttach = unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, i32, u32) -> i32;
    type AttrSet =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32, u32, *mut c_void) -> i32;
    type SessionBegin =
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32) -> i32;
    type ErrorGet =
        unsafe extern "C" fn(*mut c_void, u32, *const u8, *mut i32, *mut u8, u32, u32) -> i32;

    let env_nls: EnvNlsCreate =
        unsafe { std::mem::transmute(get_fn(module, "ACIEnvNlsCreate").unwrap()) };
    let handle_alloc: HandleAlloc =
        unsafe { std::mem::transmute(get_fn(module, "ACIHandleAlloc").unwrap()) };
    let server_attach: ServerAttach =
        unsafe { std::mem::transmute(get_fn(module, "ACIServerAttach").unwrap()) };
    let attr_set: AttrSet = unsafe { std::mem::transmute(get_fn(module, "ACIAttrSet").unwrap()) };
    let session_begin: SessionBegin =
        unsafe { std::mem::transmute(get_fn(module, "ACISessionBegin").unwrap()) };
    let error_get: ErrorGet =
        unsafe { std::mem::transmute(get_fn(module, "ACIErrorGet").unwrap()) };

    const ACI_HTYPE_ERROR: u32 = 2;
    const ACI_HTYPE_SVCCTX: u32 = 3;
    const ACI_HTYPE_SERVER: u32 = 8;
    const ACI_HTYPE_SESSION: u32 = 9;
    const ACI_ATTR_SERVER: u32 = 6;
    const ACI_ATTR_SESSION: u32 = 7;
    const ACI_ATTR_USERNAME: u32 = 22;
    const ACI_ATTR_PASSWORD: u32 = 23;
    const ACI_CRED_RDBMS: u32 = 1;
    const ACI_DEFAULT: u32 = 0;
    const ACI_OBJECT: u32 = 2;
    const ACI_CHARSET_UTF8: u16 = 871;

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

    // EXACT SAME LOOP as test_aci_ffi.rs
    // Change slice to test different combinations:
    // ["localhost:2003/osrdb", "192.168.3.34:2003/osrdb"] → known to work
    // ["192.168.3.34:2003/osrdb"] → does for loop alone help?
    // ["localhost:2003/osrdb", "192.168.3.34:2003/osrdb", "192.168.3.34:2003/osrdb"] → 3 attempts
    println!("Testing with only 192.168.3.34 in the for loop...");
    for connect_str in &["192.168.3.34:2003/osrdb"] {
        println!("\nTrying: {}", connect_str);

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
        println!("  EnvNlsCreate: rc={} env={:p}", rc, env_h);
        if rc != 0 || env_h.is_null() {
            continue;
        }

        let mut err_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
        }

        let mut svc_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut svc_h, ACI_HTYPE_SVCCTX, 0, std::ptr::null_mut());
        }

        let mut srv_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
        }

        let cs_bytes = connect_str.as_bytes();
        let rc = unsafe {
            server_attach(
                srv_h,
                err_h,
                cs_bytes.as_ptr(),
                cs_bytes.len() as i32,
                ACI_DEFAULT,
            )
        };
        println!("  ACIServerAttach: rc={}", rc);
        if rc != 0 {
            let (code, msg) = get_error(err_h);
            println!("  Error: code={} msg={}", code, msg);
            continue;
        }

        unsafe {
            attr_set(svc_h, ACI_HTYPE_SVCCTX, srv_h, 0, ACI_ATTR_SERVER, err_h);
        }

        let mut ses_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(
                env_h,
                &mut ses_h,
                ACI_HTYPE_SESSION,
                0,
                std::ptr::null_mut(),
            );
        }

        let user = b"sysdba";
        let pwd = b"szoscar55";
        unsafe {
            attr_set(
                ses_h,
                ACI_HTYPE_SESSION,
                user.as_ptr() as *mut c_void,
                user.len() as u32,
                ACI_ATTR_USERNAME,
                err_h,
            );
            attr_set(
                ses_h,
                ACI_HTYPE_SESSION,
                pwd.as_ptr() as *mut c_void,
                pwd.len() as u32,
                ACI_ATTR_PASSWORD,
                err_h,
            );
            attr_set(svc_h, ACI_HTYPE_SVCCTX, ses_h, 0, ACI_ATTR_SESSION, err_h);
        }

        let rc = unsafe { session_begin(svc_h, err_h, ses_h, ACI_CRED_RDBMS, ACI_DEFAULT) };
        println!("  ACISessionBegin: rc={}", rc);
        if rc == 0 {
            println!("  === SUCCESS ===");
        } else {
            let (code, msg) = get_error(err_h);
            println!("  Error: code={} msg={}", code, msg);
        }
    }
}

#[cfg(not(windows))]
fn test(_: Option<String>) {
    println!("Windows only");
}
