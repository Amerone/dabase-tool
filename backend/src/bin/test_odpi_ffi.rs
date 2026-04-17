// Test ODPI initialization directly via FFI, bypassing the rust-shentong wrapper
// Raw FFI to ODPI functions (compiled into our binary via libodpic.a)
extern "C" {
    fn dpiContext_createWithParams(
        major_version: u32,
        minor_version: u32,
        params: *mut std::ffi::c_void,
        context: *mut *mut std::ffi::c_void,
        error_info: *mut u8,
    ) -> i32;
}

fn main() {
    println!("=== ODPI FFI crash test ===");
    println!("Step 1: About to call dpiContext_createWithParams via raw FFI...");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    let mut ctx = std::ptr::null_mut::<std::ffi::c_void>();
    // dpiErrorInfo is ~512 bytes
    let mut err_info = vec![0u8; 512];

    // DPI_MAJOR_VERSION = 5, DPI_MINOR_VERSION = 0
    let rc = unsafe {
        dpiContext_createWithParams(5, 0, std::ptr::null_mut(), &mut ctx, err_info.as_mut_ptr())
    };

    println!("Step 2: dpiContext_createWithParams returned: {}", rc);
    if ctx.is_null() {
        println!("Context is null");
    } else {
        println!("Context: {:?}", ctx);
    }
    println!("Done.");
}
