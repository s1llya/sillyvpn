use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HelperError {
  #[error("helper binary not found")]
  MissingHelper,
  #[error("io error: {0}")]
  Io(#[from] std::io::Error),
  #[error("helper failed: {0}")]
  HelperFailed(String),
}

fn helper_path() -> Result<PathBuf, HelperError> {
  let exe = std::env::current_exe()?;
  let dir = exe
    .parent()
    .ok_or(HelperError::MissingHelper)?
    .to_path_buf();
  let helper = dir.join("sillyvpn-helper");
  if helper.exists() {
    Ok(helper)
  } else {
    Err(HelperError::MissingHelper)
  }
}

fn installed_helper_path() -> PathBuf {
  PathBuf::from("/usr/local/lib/sillyvpn/sillyvpn-helper")
}

fn helper_exec_path() -> Result<PathBuf, HelperError> {
  let installed = installed_helper_path();
  if installed.exists() {
    return Ok(installed);
  }
  install_helper(&installed)?;
  Ok(installed)
}

fn install_helper(dest: &Path) -> Result<(), HelperError> {
  let helper = helper_path()?;
  let temp_dir = std::env::temp_dir().join("sillyvpn-helper-install");
  fs::create_dir_all(&temp_dir)?;
  let temp_path = temp_dir.join("sillyvpn-helper");
  fs::copy(&helper, &temp_path)?;
  let install_bin = if PathBuf::from("/usr/bin/install").exists() {
    "/usr/bin/install"
  } else {
    return Err(HelperError::MissingHelper);
  };
  let output = configure_pkexec(Command::new("pkexec"))
    .arg(install_bin)
    .args(["-m", "755", "-D"])
    .arg(&temp_path)
    .arg(dest)
    .output()?;
  if output.status.success() {
    Ok(())
  } else {
    Err(HelperError::HelperFailed(format!(
      "install failed: {}{}",
      String::from_utf8_lossy(&output.stderr),
      String::from_utf8_lossy(&output.stdout)
    )))
  }
}

pub fn run_helper(args: &[&str]) -> Result<(), HelperError> {
  let helper = helper_exec_path()?;
  let output = configure_pkexec(Command::new("pkexec"))
    .arg(helper)
    .args(args)
    .output()?;
  if output.status.success() {
    Ok(())
  } else {
    Err(HelperError::HelperFailed(format!(
      "{}{}",
      String::from_utf8_lossy(&output.stderr),
      String::from_utf8_lossy(&output.stdout)
    )))
  }
}

pub fn run_helper_with_path(args: &[&str], path: &Path) -> Result<(), HelperError> {
  let helper = helper_exec_path()?;
  let output = configure_pkexec(Command::new("pkexec"))
    .arg(helper)
    .args(args)
    .arg(path)
    .output()?;
  if output.status.success() {
    Ok(())
  } else {
    Err(HelperError::HelperFailed(format!(
      "{}{}",
      String::from_utf8_lossy(&output.stderr),
      String::from_utf8_lossy(&output.stdout)
    )))
  }
}

pub fn run_helper_vec(args: Vec<String>) -> Result<(), HelperError> {
  let helper = helper_exec_path()?;
  let output = configure_pkexec(Command::new("pkexec"))
    .arg(helper)
    .args(args)
    .output()?;
  if output.status.success() {
    Ok(())
  } else {
    Err(HelperError::HelperFailed(format!(
      "{}{}",
      String::from_utf8_lossy(&output.stderr),
      String::from_utf8_lossy(&output.stdout)
    )))
  }
}

fn configure_pkexec(mut cmd: Command) -> Command {
  cmd.arg("--disable-internal-agent");
  for key in [
    "DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "WAYLAND_DISPLAY",
    "LANG",
    "LC_ALL",
  ] {
    if let Ok(value) = std::env::var(key) {
      cmd.env(key, value);
    }
  }
  cmd
}
