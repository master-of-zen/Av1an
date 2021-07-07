use chrono::Utc;
use once_cell::sync::OnceCell;

use std::{
  fs::File,
  io::{Error, Write},
};
static LOG_HANDLE: OnceCell<File> = OnceCell::new();

pub fn set_log(file: &str) -> Result<(), Error> {
  Ok(LOG_HANDLE.set(File::create(file).unwrap()).unwrap())
}

pub fn log(msg: &str) {
  if let Some(mut file) = LOG_HANDLE.get() {
    file
      .write_all(format!("[{}] {}\n", Utc::now().to_rfc2822(), msg).as_bytes())
      .unwrap();
  }
}
