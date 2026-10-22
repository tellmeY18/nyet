#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("usage error: {0}")]
    Usage(String),
    #[error("invalid option path: {0}")]
    InvalidPath(String),
    #[error("invalid host name: {0}")]
    InvalidHost(String),
    #[error("unknown option: {0}")]
    UnknownOption(String),
    #[error("invalid value: {0}")]
    InvalidValue(String),
    #[error("nix is not available: {0}")]
    NixUnavailable(String),
    #[error("nix evaluation failed: {0}")]
    Nix(String),
    #[error("invalid nix output: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
