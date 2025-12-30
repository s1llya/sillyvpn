use crate::helper_call::{run_helper_vec, HelperError};
use crate::logging::append_log;
use crate::models::AppStateFile;
use crate::storage::{AppStateStore, StorageError};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tauri::State;
use std::os::unix::fs::MetadataExt;

#[tauri::command]
pub fn get_state(store: State<'_, AppStateStore>) -> Result<AppStateFile, String> {
  Ok(store.state_snapshot())
}

#[tauri::command]
pub fn get_logs(store: State<'_, AppStateStore>) -> Result<Vec<String>, String> {
  let path = store.log_path();
  let content = std::fs::read_to_string(path).unwrap_or_default();
  let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
  const MAX_LINES: usize = 200;
  if lines.len() > MAX_LINES {
    lines = lines.split_off(lines.len() - MAX_LINES);
  }
  Ok(lines)
}

#[tauri::command]
pub fn clear_logs(store: State<'_, AppStateStore>) -> Result<(), String> {
  std::fs::write(store.log_path(), "").map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn import_conf(path: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  let source = PathBuf::from(path);
  if !source.exists() {
    return Err("Config file not found".into());
  }
  if source.extension().and_then(|s| s.to_str()) != Some("conf") {
    return Err("Only .conf files are supported".into());
  }

  let tunnel = store.import_conf(&source).map_err(map_error)?;
  append_log(store.log_path(), &format!("Imported tunnel {}", tunnel.name))
    .map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn add_app(path: String, label: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  let app_path = PathBuf::from(path);
  if !app_path.exists() {
    return Err("Binary not found".into());
  }
  store
    .add_app(&app_path, label)
    .map_err(map_error)?;
  append_log(store.log_path(), "Added VPN app").map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn remove_app(app_id: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  store.remove_app(&app_id).map_err(map_error)?;
  append_log(store.log_path(), "Removed VPN app").map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn enable_vpn(tunnel_id: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  let tunnel = store
    .find_tunnel(&tunnel_id)
    .ok_or_else(|| "Tunnel not found".to_string())?;
  let ifname = "wg-temp".to_string();

  let args = vec![
    "enable".to_string(),
    "--config".to_string(),
    tunnel.path.clone(),
    "--ifname".to_string(),
    ifname,
  ];
  run_helper_vec(args).map_err(map_helper_error)?;
  store.set_vpn_enabled(true).map_err(map_error)?;
  append_log(store.log_path(), "VPN enabled").map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn disable_vpn(store: State<'_, AppStateStore>) -> Result<(), String> {
  let args = vec!["disable".to_string()];
  run_helper_vec(args).map_err(map_helper_error)?;
  store.set_vpn_enabled(false).map_err(map_error)?;
  append_log(store.log_path(), "VPN disabled").map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn run_app_via_vpn(app_id: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  let app = store
    .find_app(&app_id)
    .ok_or_else(|| "App not found".to_string())?;
  ensure_app_not_running(&app.path)?;
  store
    .set_last_app_id(&app_id)
    .map_err(map_error)?;
  let mut args = vec!["run".to_string(), "--bin".to_string(), app.path.clone()];
  for (key, value) in collect_ui_env() {
    args.push("--env".to_string());
    args.push(format!("{}={}", key, value));
  }
  run_helper_vec(args).map_err(map_helper_error)?;
  append_log(
    store.log_path(),
    &format!("Started app via VPN: {}", app.label),
  )
  .map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn kill_all_apps(store: State<'_, AppStateStore>) -> Result<(), String> {
  let apps = store.state_snapshot().apps;
  let mut total = 0;
  for app in apps {
    total += kill_by_path_in_namespace(&app.path, "sillyvpn-ns")?;
  }
  append_log(
    store.log_path(),
    &format!("Killed {} processes for VPN apps", total),
  )
  .map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn get_running_apps(store: State<'_, AppStateStore>) -> Result<Vec<String>, String> {
  let apps = store.state_snapshot().apps;
  let mut running = Vec::new();
  for app in apps {
    if is_app_running_in_namespace(&app.path, "sillyvpn-ns")? {
      running.push(app.id);
    }
  }
  Ok(running)
}

fn ensure_app_not_running(path: &str) -> Result<(), String> {
  if is_app_running(path)? {
    return Err(
      "Приложение уже запущено. Закройте его полностью и повторите запуск через VPN."
        .to_string(),
    );
  }
  Ok(())
}

fn is_app_running(path: &str) -> Result<bool, String> {
  let target = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
  let target_base = target
    .file_name()
    .and_then(|s| s.to_str())
    .unwrap_or("")
    .to_string();
  let target_base_lower = target_base.to_ascii_lowercase();
  for entry in std::fs::read_dir("/proc").map_err(|e| e.to_string())? {
    let entry = match entry {
      Ok(entry) => entry,
      Err(_) => continue,
    };
    let file_name = entry.file_name();
    let pid = match file_name.to_str() {
      Some(name) => name,
      None => continue,
    };
    if !pid.chars().all(|c| c.is_ascii_digit()) {
      continue;
    }
    if process_matches_path(
      &entry.path(),
      &target,
      &target_base,
      &target_base_lower,
    ) {
      return Ok(true);
    }
  }
  Ok(false)
}

fn is_app_running_in_namespace(path: &str, ns_name: &str) -> Result<bool, String> {
  let ns_inode = match read_netns_inode(ns_name) {
    Ok(Some(inode)) => inode,
    Ok(None) => return Ok(false),
    Err(err) => return Err(err),
  };
  let target = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
  let target_base = target
    .file_name()
    .and_then(|s| s.to_str())
    .unwrap_or("")
    .to_string();
  let target_base_lower = target_base.to_ascii_lowercase();
  for entry in std::fs::read_dir("/proc").map_err(|e| e.to_string())? {
    let entry = match entry {
      Ok(entry) => entry,
      Err(_) => continue,
    };
    let file_name = entry.file_name();
    let pid = match file_name.to_str() {
      Some(name) => name,
      None => continue,
    };
    if !pid.chars().all(|c| c.is_ascii_digit()) {
      continue;
    }
    let proc_path = entry.path();
    if !process_in_namespace(&proc_path, ns_inode) {
      continue;
    }
    if !process_matches_path(
      &proc_path,
      &target,
      &target_base,
      &target_base_lower,
    ) {
      continue;
    }
    return Ok(true);
  }
  Ok(false)
}

fn kill_by_path_in_namespace(path: &str, ns_name: &str) -> Result<u32, String> {
  let ns_inode = match read_netns_inode(ns_name) {
    Ok(Some(inode)) => inode,
    Ok(None) => return Ok(0),
    Err(err) => return Err(err),
  };
  let target = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
  let target_base = target
    .file_name()
    .and_then(|s| s.to_str())
    .unwrap_or("")
    .to_string();
  let target_base_lower = target_base.to_ascii_lowercase();
  let mut pids = Vec::new();
  for entry in std::fs::read_dir("/proc").map_err(|e| e.to_string())? {
    let entry = match entry {
      Ok(entry) => entry,
      Err(_) => continue,
    };
    let file_name = entry.file_name();
    let pid_str = match file_name.to_str() {
      Some(name) => name,
      None => continue,
    };
    if !pid_str.chars().all(|c| c.is_ascii_digit()) {
      continue;
    }
    let pid: i32 = match pid_str.parse() {
      Ok(pid) => pid,
      Err(_) => continue,
    };
    let proc_path = entry.path();
    if !process_matches_path(
      &proc_path,
      &target,
      &target_base,
      &target_base_lower,
    ) {
      continue;
    }
    if process_in_namespace(&proc_path, ns_inode) {
      pids.push(pid);
    }
  }

  if pids.is_empty() {
    return Ok(0);
  }

  for pid in &pids {
    unsafe {
      libc::kill(*pid, libc::SIGTERM);
    }
  }
  std::thread::sleep(Duration::from_millis(300));
  for pid in &pids {
    if std::fs::metadata(format!("/proc/{pid}")).is_ok() {
      unsafe {
        libc::kill(*pid, libc::SIGKILL);
      }
    }
  }
  Ok(pids.len() as u32)
}

fn process_matches_path(
  proc_dir: &PathBuf,
  target: &PathBuf,
  target_base: &str,
  target_base_lower: &str,
) -> bool {
  let exe_path = proc_dir.join("exe");
  if let Ok(link) = std::fs::read_link(&exe_path) {
    if &link == target {
      return true;
    }
  }

  let cmdline_path = proc_dir.join("cmdline");
  if let Ok(raw) = std::fs::read(cmdline_path) {
    let parts: Vec<String> = raw
      .split(|b| *b == 0)
      .filter_map(|slice| {
        if slice.is_empty() {
          None
        } else {
          Some(String::from_utf8_lossy(slice).to_string())
        }
      })
      .collect();
    for arg in parts {
      if &arg == target.to_string_lossy().as_ref() {
        return true;
      }
      if !target_base.is_empty() {
        if let Some(base) = std::path::Path::new(&arg).file_name().and_then(|s| s.to_str()) {
          if base == target_base || base.to_ascii_lowercase() == target_base_lower {
            return true;
          }
        }
      }
    }
  }

  if !target_base.is_empty() {
    let comm_path = proc_dir.join("comm");
    if let Ok(comm) = std::fs::read_to_string(comm_path) {
      let comm = comm.trim();
      if comm == target_base || comm.to_ascii_lowercase() == target_base_lower {
        return true;
      }
    }
  }

  false
}

fn read_netns_inode(ns_name: &str) -> Result<Option<u64>, String> {
  let ns_path = format!("/var/run/netns/{ns_name}");
  match std::fs::metadata(ns_path) {
    Ok(meta) => Ok(Some(meta.ino())),
    Err(err) => {
      if err.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
      } else {
        Err(err.to_string())
      }
    }
  }
}

fn process_in_namespace(proc_dir: &PathBuf, ns_inode: u64) -> bool {
  let ns_path = proc_dir.join("ns/net");
  match std::fs::metadata(ns_path) {
    Ok(meta) => meta.ino() == ns_inode,
    Err(_) => false,
  }
}

fn collect_ui_env() -> Vec<(String, String)> {
  let keys = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
  ];
  let mut out = Vec::new();
  for key in keys {
    if let Ok(value) = std::env::var(key) {
      if !value.trim().is_empty() {
        out.push((key.to_string(), value));
      }
    }
  }
  out
}

#[tauri::command]
pub fn set_last_tunnel(tunnel_id: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  store
    .set_last_tunnel_id(&tunnel_id)
    .map_err(map_error)?;
  Ok(())
}

#[tauri::command]
pub fn set_last_app(app_id: String, store: State<'_, AppStateStore>) -> Result<(), String> {
  store.set_last_app_id(&app_id).map_err(map_error)?;
  Ok(())
}

#[derive(Debug, Serialize)]
pub struct PolkitStatus {
  pub running: bool,
  pub detail: String,
}

#[tauri::command]
pub fn check_polkit_agent() -> Result<PolkitStatus, String> {
  let patterns = [
    "polkit-kde-authentication-agent-1",
    "polkit-gnome-authentication-agent-1",
    "lxqt-policykit",
  ];
  let mut running = false;
  for pattern in patterns {
    let ok = Command::new("pgrep")
      .args(["-f", pattern])
      .status()
      .map(|status| status.success())
      .unwrap_or(false);
    if ok {
      running = true;
      break;
    }
  }
  let detail = if running {
    "polkit-agent is running".to_string()
  } else {
    "polkit-agent is not running".to_string()
  };
  Ok(PolkitStatus { running, detail })
}

#[tauri::command]
pub fn enable_polkit_autostart() -> Result<(), String> {
  let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
  let autostart_dir = PathBuf::from(home).join(".config/autostart");
  std::fs::create_dir_all(&autostart_dir).map_err(|e| e.to_string())?;
  let desktop_path = autostart_dir.join("polkit-kde-agent.desktop");
  let contents = r#"[Desktop Entry]
Type=Application
Name=Polkit KDE Agent
Exec=/usr/lib/polkit-kde-authentication-agent-1
X-KDE-autostart-after=panel
"#;
  std::fs::write(desktop_path, contents).map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
pub fn start_polkit_agent() -> Result<(), String> {
  let candidates = [
    "/usr/lib/polkit-kde-authentication-agent-1",
    "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1",
    "/usr/bin/lxqt-policykit",
  ];
  for path in candidates {
    if std::path::Path::new(path).exists() {
      Command::new(path)
        .spawn()
        .map_err(|e| format!("failed to start polkit agent: {e}"))?;
      return Ok(());
    }
  }
  Err("polkit agent not found".to_string())
}

fn map_error(err: StorageError) -> String {
  err.to_string()
}

fn map_helper_error(err: HelperError) -> String {
  let message = err.to_string();
  if message.contains("Error accessing")
    || message.contains("Permission denied")
    || message.contains("status 127")
    || message.contains("install failed")
  {
    return "Недостаточно прав. Убедитесь, что pkexec и polkit-agent работают, затем повторите. При первом запуске потребуется установка helper в /usr/local/lib."
      .to_string();
  }
  if message.contains("wg-quick error") && message.contains("resolvconf") {
    return "Ошибка DNS: wg-quick попытался изменить DNS. Уберите DNS= из конфигурации или используйте systemd-resolved."
      .to_string();
  }
  message
}
