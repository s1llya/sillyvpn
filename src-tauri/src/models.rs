use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tunnel {
  pub id: String,
  pub name: String,
  pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppItem {
  pub id: String,
  pub label: String,
  pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppStateFile {
  pub tunnels: Vec<Tunnel>,
  pub apps: Vec<AppItem>,
  pub last_tunnel_id: Option<String>,
  pub last_app_id: Option<String>,
  pub vpn_enabled: bool,
}
