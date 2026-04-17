// Direct ACI FFI test - bypasses ODPI entirely
// Tests ACIServerAttach and ACISessionBegin directly via Windows LoadLibrary

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

#[cfg(windows)]
fn get_fn(module: *mut c_void, name: &str) -> Option<*mut c_void> {
    let cname = CString::new(name).unwrap();
    let ptr = unsafe { GetProcAddress(module, cname.as_ptr() as *const u8) };
    if ptr.is_null() {
        println!("  GetProcAddress({}) failed", name);
        None
    } else {
        Some(ptr)
    }
}

fn main() {
    println!("=== Direct ACI FFI test ===");

    // Set TNS_ADMIN before loading DLL
    let aci_dir = find_aci_dir();
    if let Some(ref d) = aci_dir {
        println!("Setting TNS_ADMIN = {}", d);
        std::env::set_var("TNS_ADMIN", d);
    }

    #[cfg(windows)]
    test_aci_direct(aci_dir);
}

fn find_aci_dir() -> Option<String> {
    let candidates = ["drivers/shentong/windows", "../drivers/shentong/windows"];
    for dir in &candidates {
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
fn test_aci_direct(aci_dir: Option<String>) {
    // Load aci.dll
    let dll_path = match aci_dir {
        Some(ref d) => format!("{}\\aci.dll\0", d),
        None => "aci.dll\0".to_string(),
    };

    println!("Loading: {}", dll_path.trim_end_matches('\0'));
    let module = unsafe { LoadLibraryA(dll_path.as_ptr()) };
    if module.is_null() {
        println!("LoadLibraryA failed!");
        return;
    }
    println!("DLL loaded: {:p}", module);

    // Get function pointers
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

    let fn_env_nls = get_fn(module, "ACIEnvNlsCreate").unwrap();
    let fn_handle_alloc = get_fn(module, "ACIHandleAlloc").unwrap();
    let fn_server_attach = get_fn(module, "ACIServerAttach").unwrap();
    let fn_attr_set = get_fn(module, "ACIAttrSet").unwrap();
    let fn_session_begin = get_fn(module, "ACISessionBegin").unwrap();
    let fn_error_get = get_fn(module, "ACIErrorGet").unwrap();

    let env_nls: EnvNlsCreate = unsafe { std::mem::transmute(fn_env_nls) };
    let handle_alloc: HandleAlloc = unsafe { std::mem::transmute(fn_handle_alloc) };
    let server_attach: ServerAttach = unsafe { std::mem::transmute(fn_server_attach) };
    let attr_set: AttrSet = unsafe { std::mem::transmute(fn_attr_set) };
    let session_begin: SessionBegin = unsafe { std::mem::transmute(fn_session_begin) };
    let error_get: ErrorGet = unsafe { std::mem::transmute(fn_error_get) };

    // ShenTong ACI constants (from aci.h)
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
        let msg = String::from_utf8_lossy(&buf[..end]).to_string();
        (code, msg)
    };

    let configured_connect = shentong_diag_env::default_connect();
    let connect_strings = ["localhost:2003/osrdb".to_string(), configured_connect];

    for connect_str in &connect_strings {
        println!("\nTrying: {}", connect_str);

        // Create env
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

        // Create error handle
        let mut err_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut err_h, ACI_HTYPE_ERROR, 0, std::ptr::null_mut());
        }

        // Create service context
        let mut svc_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut svc_h, ACI_HTYPE_SVCCTX, 0, std::ptr::null_mut());
        }

        // Create server handle
        let mut srv_h: *mut c_void = std::ptr::null_mut();
        unsafe {
            handle_alloc(env_h, &mut srv_h, ACI_HTYPE_SERVER, 0, std::ptr::null_mut());
        }

        // ACIServerAttach
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

        // Set server on svc
        unsafe {
            attr_set(svc_h, ACI_HTYPE_SVCCTX, srv_h, 0, ACI_ATTR_SERVER, err_h);
        }

        // Create session
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

        // Set user/password
        let user = shentong_diag_env::user("sysdba");
        let password = shentong_diag_env::password();
        let user = user.as_bytes();
        let pwd = password.as_bytes();
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

        // ACISessionBegin
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
fn test_aci_direct(_: Option<String>) {
    println!("Not on Windows");
}
