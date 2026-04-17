#![allow(dead_code)]

pub fn user(default_user: &str) -> String {
    std::env::var("SHENTONG_DIAG_USER").unwrap_or_else(|_| default_user.to_string())
}

pub fn password() -> String {
    std::env::var("SHENTONG_DIAG_PASSWORD")
        .or_else(|_| std::env::var("SHENTONG_PASSWORD"))
        .unwrap_or_else(|_| {
            eprintln!("Set SHENTONG_DIAG_PASSWORD or SHENTONG_PASSWORD to run this diagnostic");
            std::process::exit(2);
        })
}

pub fn connect(default_connect: &str) -> String {
    std::env::var("SHENTONG_DIAG_CONNECT").unwrap_or_else(|_| default_connect.to_string())
}

pub fn default_connect() -> String {
    connect("127.0.0.1:2003/osrdb")
}
