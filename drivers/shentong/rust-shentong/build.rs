extern crate cc;

use std::path;

fn main() {
    if !path::Path::new("odpi/include/dpi.h").exists() {
        println!("The odpi submodule isn't initialized. Run the following commands.");
        println!("  git submodule init");
        println!("  git submodule update");
        std::process::exit(1);
    }

    let mut build = cc::Build::new();
    build
        .file("odpi/embed/dpi.c")
        .include("odpi/include")
        .flag_if_supported("-Wno-unused-parameter");

    // Only define LINUX on non-Windows targets. On Windows the `_WIN32` macro
    // is already set by MSVC, and the LINUX block contains GCC-specific code
    // (`__attribute__`, `__GNUC__`) that MSVC cannot compile.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        build.flag_if_supported("-DLINUX");
    }

    build.compile("libodpic.a");
}
