/// Debug: compare connect string pointer in for-loop vs direct byte literal
#[cfg(windows)]
use std::ffi::{c_void, CString};
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
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
fn do_attach(
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
    alloc: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, u32, usize, *mut *mut c_void) -> i32,
    attach: unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, i32, u32) -> i32,
    cs_ptr: *const u8,
    cs_len: usize,
    label: &str,
) {
    println!("  [{}] cs_ptr={:p} len={}", label, cs_ptr, cs_len);
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
            871u16,
            871u16,
        )
    };
    println!("  [{}] EnvNlsCreate rc={} env={:p}", label, rc, env);
    let mut err: *mut c_void = std::ptr::null_mut();
    let mut svc: *mut c_void = std::ptr::null_mut();
    let mut srv: *mut c_void = std::ptr::null_mut();
    unsafe {
        alloc(env, &mut err, 2, 0, std::ptr::null_mut());
        alloc(env, &mut svc, 3, 0, std::ptr::null_mut());
        alloc(env, &mut srv, 8, 0, std::ptr::null_mut());
    }
    let rc2 = unsafe { attach(srv, err, cs_ptr, cs_len as i32, 0) };
    println!("  [{}] attach: rc={}", label, rc2);
}

#[cfg(windows)]
fn run(aci_dir: Option<String>) {
    let dll_path = match aci_dir {
        Some(ref d) => format!("{}\\aci.dll\0", d),
        None => "aci.dll\0".to_string(),
    };
    let module = unsafe { LoadLibraryA(dll_path.as_ptr()) };
    if module.is_null() {
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

    println!("=== PRIMED FAIL: localhost (should fail, check if it breaks subsequent calls) ===");
    {
        let cs = b"localhost:2003/osrdb";
        do_attach(env_nls, alloc, attach, cs.as_ptr(), cs.len(), "prime-fail");
    }

    println!("=== for-loop AFTER failed call ===");
    for connect_str in &["192.168.3.34:2003/osrdb"] {
        let cs = connect_str.as_bytes();
        do_attach(
            env_nls,
            alloc,
            attach,
            cs.as_ptr(),
            cs.len(),
            "for-loop-after",
        );
    }

    println!("=== byte literal AFTER failed call ===");
    {
        let cs = b"192.168.3.34:2003/osrdb";
        do_attach(
            env_nls,
            alloc,
            attach,
            cs.as_ptr(),
            cs.len(),
            "byte-lit-after",
        );
    }
}
#[cfg(not(windows))]
fn run(_: Option<String>) {}
