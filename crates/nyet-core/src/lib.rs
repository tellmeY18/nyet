#![forbid(unsafe_code)]

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn engine_name() -> &'static str {
    "nyet"
}
