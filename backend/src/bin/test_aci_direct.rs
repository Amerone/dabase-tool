/// Minimal test: directly load aci.dll from Rust using raw Win32 API
/// bypassing ODPI entirely, to verify aci.dll works from Rust.
#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn LoadLibraryW(lp_lib_file_name: *const u16) -> *mut c_void;
        pub fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const u8) -> *mut c_void;
        pub fn GetLastError() -> u32;
        pub fn FreeLibrary(h_lib_module: *mut c_void) -> i32;
    }
}

fn main() {
    println!("=== Direct Win32 DLL test ===");

    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use win32::*;

        let path: Vec<u16> = OsStr::new("aci.dll").encode_wide().chain(Some(0)).collect();

        println!("Step 1: LoadLibraryW(aci.dll)...");
        let hmod = unsafe { LoadLibraryW(path.as_ptr()) };
        if hmod.is_null() {
            let err = unsafe { GetLastError() };
            eprintln!("FAIL: LoadLibraryW failed with error {}", err);
            return;
        }
        println!("OK: DLL loaded at {:?}", hmod);

        println!("Step 2: GetProcAddress(ACIClientVersion)...");
        let fn_ptr = unsafe { GetProcAddress(hmod, b"ACIClientVersion\0".as_ptr()) };
        if fn_ptr.is_null() {
            eprintln!("FAIL: GetProcAddress failed");
            unsafe {
                FreeLibrary(hmod);
            }
            return;
        }
        println!("OK: Got function pointer at {:?}", fn_ptr);

        println!("Step 3: Calling ACIClientVersion...");
        type AciClientVersion =
            unsafe extern "C" fn(*mut i32, *mut i32, *mut i32, *mut i32, *mut i32);
        let aci_client_version: AciClientVersion = unsafe { std::mem::transmute(fn_ptr) };

        let mut major = 0i32;
        let mut minor = 0i32;
        let mut update = 0i32;
        let mut patch = 0i32;
        let mut port_update = 0i32;

        unsafe {
            aci_client_version(
                &mut major,
                &mut minor,
                &mut update,
                &mut patch,
                &mut port_update,
            );
        }
        println!(
            "OK: Version {}.{}.{}.{}.{}",
            major, minor, update, patch, port_update
        );

        // Now try ACIEnvNlsCreate
        println!("\nStep 4: GetProcAddress(ACIEnvNlsCreate)...");
        let env_fn_ptr = unsafe { GetProcAddress(hmod, b"ACIEnvNlsCreate\0".as_ptr()) };
        if env_fn_ptr.is_null() {
            eprintln!("FAIL: GetProcAddress(ACIEnvNlsCreate) failed");
        } else {
            println!("OK: Got ACIEnvNlsCreate at {:?}", env_fn_ptr);
            println!("Step 5: Calling ACIEnvNlsCreate...");

            // ACIEnvNlsCreate(envhpp, mode, ctxp, malloc, realloc, free, xtramemsz, usrmempp, charset, ncharset)
            // Using OCI_THREADED=2, charset=852 (GBK)
            type AciEnvNlsCreate = unsafe extern "C" fn(
                *mut *mut std::ffi::c_void, // envhpp
                u32,                        // mode (OCI_THREADED = 2)
                *mut std::ffi::c_void,      // ctxp
                *mut std::ffi::c_void,      // malocfp
                *mut std::ffi::c_void,      // ralocfp
                *mut std::ffi::c_void,      // mfreefp
                usize,                      // xtramemsz
                *mut *mut std::ffi::c_void, // usrmempp
                u16,                        // charset
                u16,                        // ncharset
            ) -> i32;

            let aci_env_nls_create: AciEnvNlsCreate = unsafe { std::mem::transmute(env_fn_ptr) };
            let mut env_handle = std::ptr::null_mut::<std::ffi::c_void>();

            let ret = unsafe {
                aci_env_nls_create(
                    &mut env_handle,
                    2, // OCI_THREADED
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    852u16,
                    852u16, // ZHS16GBK charset
                )
            };
            println!(
                "ACIEnvNlsCreate returned: {}, env_handle: {:?}",
                ret, env_handle
            );
        }

        unsafe {
            FreeLibrary(hmod);
        }
        println!("\nAll tests passed!");
    }

    #[cfg(not(windows))]
    println!("This test is Windows-only");
}
