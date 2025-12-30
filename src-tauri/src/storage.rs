use crate::models::{AppItem, AppStateFile, Tunnel};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::api::path::config_dir;
use thiserror::Error;
use uuid::Uuid;

const APP_DIR: &str = "sillyvpn";
const STATE_FILE: &str = "state.json";

#[derive(Debug, Error)]
pub enum StorageError {
  #[error("missing config directory")]
  MissingConfigDir,
  #[error("io error: {0}")]
  Io(#[from] io::Error),
  #[error("json error: {0}")]
  Json(#[from] serde_json::Error),
  #[error("tunnel not found")]
  TunnelNotFound,
  #[error("app not found")]
  AppNotFound,
}

pub struct AppStateStore {
  state: Mutex<AppStateFile>,
  data_dir: PathBuf,
  log_path: PathBuf,
}

impl AppStateStore {
  pub fn new() -> Self {
    let base = config_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let data_dir = base.join(APP_DIR);
    let log_path = data_dir.join("app.log");
    fs::create_dir_all(&data_dir).ok();

    let state = load_state_file(&data_dir).unwrap_or_default();
    Self {
      state: Mutex::new(state),
      data_dir,
      log_path,
    }
  }

  pub fn data_dir(&self) -> &Path {
    &self.data_dir
  }

  pub fn log_path(&self) -> &Path {
    &self.log_path
  }

  pub fn state_snapshot(&self) -> AppStateFile {
    self.state.lock().expect("lock").clone()
  }

  pub fn save_state(&self, state: &AppStateFile) -> Result<(), StorageError> {
    save_state_file(&self.data_dir, state)?;
    Ok(())
  }

  pub fn import_conf(&self, src: &Path) -> Result<Tunnel, StorageError> {
    let mut state = self.state.lock().expect("lock");
    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.conf", id);
    let dest = self.data_dir.join(&file_name);
    fs::copy(src, &dest)?;
    set_private_permissions(&dest)?;

    let name = src
      .file_stem()
      .and_then(|s| s.to_str())
      .unwrap_or("tunnel")
      .to_string();

    let tunnel = Tunnel {
      id: id.clone(),
      name,
      path: dest.to_string_lossy().to_string(),
    };
    state.tunnels.push(tunnel.clone());
    state.last_tunnel_id = Some(id);
    save_state_file(&self.data_dir, &state)?;
    Ok(tunnel)
  }

  pub fn add_app(&self, path: &Path, label: String) -> Result<AppItem, StorageError> {
    let mut state = self.state.lock().expect("lock");
    let id = Uuid::new_v4().to_string();
    let app = AppItem {
      id: id.clone(),
      label,
      path: path.to_string_lossy().to_string(),
    };
    state.apps.push(app.clone());
    save_state_file(&self.data_dir, &state)?;
    Ok(app)
  }

  pub fn remove_app(&self, app_id: &str) -> Result<(), StorageError> {
    let mut state = self.state.lock().expect("lock");
    let initial = state.apps.len();
    state.apps.retain(|app| app.id != app_id);
    if state.apps.len() == initial {
      return Err(StorageError::AppNotFound);
    }
    save_state_file(&self.data_dir, &state)?;
    Ok(())
  }

  pub fn set_vpn_enabled(&self, enabled: bool) -> Result<(), StorageError> {
    let mut state = self.state.lock().expect("lock");
    state.vpn_enabled = enabled;
    save_state_file(&self.data_dir, &state)?;
    Ok(())
  }

  pub fn set_last_tunnel_id(&self, tunnel_id: &str) -> Result<(), StorageError> {
    let mut state = self.state.lock().expect("lock");
    state.last_tunnel_id = Some(tunnel_id.to_string());
    save_state_file(&self.data_dir, &state)?;
    Ok(())
  }

  pub fn set_last_app_id(&self, app_id: &str) -> Result<(), StorageError> {
    let mut state = self.state.lock().expect("lock");
    state.last_app_id = Some(app_id.to_string());
    save_state_file(&self.data_dir, &state)?;
    Ok(())
  }

  pub fn find_tunnel(&self, id: &str) -> Option<Tunnel> {
    self
      .state
      .lock()
      .expect("lock")
      .tunnels
      .iter()
      .find(|tunnel| tunnel.id == id)
      .cloned()
  }

  pub fn find_app(&self, id: &str) -> Option<AppItem> {
    self
      .state
      .lock()
      .expect("lock")
      .apps
      .iter()
      .find(|app| app.id == id)
      .cloned()
  }
}

fn load_state_file(data_dir: &Path) -> Result<AppStateFile, StorageError> {
  let path = data_dir.join(STATE_FILE);
  if !path.exists() {
    return Ok(AppStateFile::default());
  }
  let mut file = fs::File::open(path)?;
  let mut contents = String::new();
  file.read_to_string(&mut contents)?;
  Ok(serde_json::from_str(&contents)?)
}

fn save_state_file(data_dir: &Path, state: &AppStateFile) -> Result<(), StorageError> {
  fs::create_dir_all(data_dir)?;
  let path = data_dir.join(STATE_FILE);
  let payload = serde_json::to_string_pretty(state)?;
  let mut file = fs::File::create(path)?;
  file.write_all(payload.as_bytes())?;
  Ok(())
}

fn set_private_permissions(path: &Path) -> Result<(), StorageError> {
  let mut perms = fs::metadata(path)?.permissions();
  perms.set_mode(0o600);
  fs::set_permissions(path, perms)?;
  Ok(())
}
