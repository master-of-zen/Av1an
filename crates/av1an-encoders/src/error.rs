use std::{fmt::Display, process::ExitStatus};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0} does not support {1:?}")]
    UnsupportedFormat(String, String),

    #[error(
        "Encoder crashed: {exit_status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    )]
    EncoderCrash {
        exit_status: ExitStatus,
        stdout:      String,
        stderr:      String,
    },

    #[error("Invalid encoder parameters: {0}")]
    InvalidParameters(String),

    #[error("Encoder not found: {0}")]
    EncoderNotFound(String),
}

#[derive(Debug, Clone)]
pub struct StringOrBytes {
    pub inner: Vec<u8>,
}

impl Display for StringOrBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(s) = std::str::from_utf8(&self.inner) {
            write!(f, "{}", s)
        } else {
            write!(f, "{:?}", self.inner)
        }
    }
}
