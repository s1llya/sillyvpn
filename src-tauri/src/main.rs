mod commands;
mod helper_call;
mod logging;
mod models;
mod storage;

use commands::*;
use logging::init_logger;
use storage::AppStateStore;

fn main() {
  let state_store = AppStateStore::new();
  init_logger(&state_store).expect("logger init");

  tauri::Builder::default()
    .manage(state_store)
    .invoke_handler(tauri::generate_handler![
      get_state,
      get_logs,
      import_conf,
      add_app,
      remove_app,
      enable_vpn,
      disable_vpn,
      run_app_via_vpn,
      set_last_tunnel,
      set_last_app,
      check_polkit_agent,
      enable_polkit_autostart,
      kill_all_apps,
      start_polkit_agent,
      get_running_apps,
      clear_logs
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
