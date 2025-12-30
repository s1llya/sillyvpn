use crate::storage::AppStateStore;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub fn init_logger(store: &AppStateStore) -> io::Result<()> {
  if let Some(parent) = store.log_path().parent() {
    fs::create_dir_all(parent)?;
  }
  Ok(())
}

pub fn append_log(path: &Path, message: &str) -> io::Result<()> {
  let timestamp = OffsetDateTime::now_local()
    .unwrap_or_else(|_| OffsetDateTime::now_utc())
    .format(&Rfc3339)
    .unwrap_or_else(|_| "unknown-time".to_string());

  let mut file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(path)?;
  writeln!(file, "{} | {}", timestamp, message)?;
  Ok(())
}
