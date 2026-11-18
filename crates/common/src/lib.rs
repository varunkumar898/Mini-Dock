#![forbid(unsafe_code)]

//! `alphen-common`: Common errors, types, and logging initialization for `alphen-box`.

use thiserror::Error;

/// Common error type for `alphen-box` operations.
#[derive(Debug, Error)]
pub enum AlphenError {
    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Missing or incompatible system requirement (cgroups v2, namespaces, overlayfs, seccomp).
    #[error("System check failed: {0}")]
    SystemRequirement(String),

    /// Image or distribution spec error.
    #[error("Image error: {0}")]
    Image(String),

    /// Container lifecycle or execution error.
    #[error("Runtime error: {0}")]
    Runtime(String),

    /// Network or netlink error.
    #[error("Network error: {0}")]
    Network(String),

    /// Configuration or argument error.
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Convenience result alias for `alphen-box`.
pub type Result<T> = std::result::Result<T, AlphenError>;

/// Initialize logging with `env_logger`. Defaults to info if unset.
pub fn init_logger() {
    let mut builder = env_logger::Builder::new();
    if std::env::var("ALPHEN_LOG").is_ok() {
        builder.parse_env("ALPHEN_LOG");
    } else {
        builder.filter_level(log::LevelFilter::Info);
    }
    // Keep log format minimal and fast
    builder.format_timestamp(None);
    builder.format_target(false);
    let _ = builder.try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AlphenError::Config("invalid parameter".to_string());
        assert_eq!(format!("{err}"), "Configuration error: invalid parameter");
    }
}
