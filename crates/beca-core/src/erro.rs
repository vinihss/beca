use std::fmt;

/// Shared error type for beca-core.
#[derive(Debug)]
pub enum BecaError {
    Config(String),
    Theme(String),
    Io(std::io::Error),
    Runtime(String),
}

impl fmt::Display for BecaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BecaError::Config(m) => write!(f, "config error: {m}"),
            BecaError::Theme(m) => write!(f, "theme error: {m}"),
            BecaError::Io(e) => write!(f, "io error: {e}"),
            BecaError::Runtime(m) => write!(f, "runtime error: {m}"),
        }
    }
}

impl std::error::Error for BecaError {}

impl From<std::io::Error> for BecaError {
    fn from(e: std::io::Error) -> Self {
        BecaError::Io(e)
    }
}

impl From<String> for BecaError {
    fn from(s: String) -> Self {
        BecaError::Runtime(s)
    }
}
