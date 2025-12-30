import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { open } from "@tauri-apps/api/dialog";
import { AppItem, AppState, PolkitStatus, Tunnel } from "./types";

const LOG_POLL_MS = 1500;

const emptyState: AppState = {
  tunnels: [],
  apps: [],
  last_tunnel_id: null,
  last_app_id: null,
  vpn_enabled: false
};

function basename(path: string) {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export default function App() {
  const [state, setState] = useState<AppState>(emptyState);
  const [selectedTunnelId, setSelectedTunnelId] = useState<string>("");
  const [selectedAppId, setSelectedAppId] = useState<string>("");
  const [manualAppPath, setManualAppPath] = useState("");
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [polkit, setPolkit] = useState<PolkitStatus | null>(null);

  const selectedTunnel: Tunnel | undefined = useMemo(
    () => state.tunnels.find((t) => t.id === selectedTunnelId),
    [state.tunnels, selectedTunnelId]
  );

  const refreshState = async () => {
    const next = await invoke<AppState>("get_state");
    setState(next);
    if (!selectedTunnelId && next.last_tunnel_id) {
      setSelectedTunnelId(next.last_tunnel_id);
    }
    if (!selectedAppId && next.last_app_id) {
      setSelectedAppId(next.last_app_id);
    }
  };

  const refreshLogs = async () => {
    const next = await invoke<string[]>("get_logs");
    setLogs(next);
  };

  useEffect(() => {
    refreshState().catch(console.error);
    refreshLogs().catch(console.error);
    invoke<PolkitStatus>("check_polkit_agent")
      .then((status) => {
        setPolkit(status);
        if (!status.running) {
          invoke("start_polkit_agent").catch(console.error);
        }
      })
      .catch(console.error);
    const timer = setInterval(() => {
      refreshLogs().catch(console.error);
    }, LOG_POLL_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);

  const onImport = async () => {
    setError(null);
    const selected = await open({
      multiple: false,
      filters: [{ name: "WireGuard", extensions: ["conf"] }]
    });
    if (!selected || Array.isArray(selected)) return;
    setBusy(true);
    try {
      await invoke("import_conf", { path: selected });
      await refreshState();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onAddAppManual = async () => {
    setError(null);
    if (!manualAppPath.trim()) {
      setError("Provide a binary path.");
      return;
    }
    setBusy(true);
    try {
      const label = basename(manualAppPath.trim());
      await invoke("add_app", { path: manualAppPath.trim(), label });
      setManualAppPath("");
      await refreshState();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onRemoveApp = async (app: AppItem) => {
    setError(null);
    setBusy(true);
    try {
      await invoke("remove_app", { appId: app.id });
      await refreshState();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onEnable = async () => {
    setError(null);
    if (!selectedTunnelId) {
      setError("Select a tunnel first.");
      return;
    }
    setBusy(true);
    try {
      await invoke("enable_vpn", { tunnelId: selectedTunnelId });
      await refreshState();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onDisable = async () => {
    setError(null);
    setBusy(true);
    try {
      await invoke("disable_vpn");
      await refreshState();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onRun = async (app: AppItem) => {
    setError(null);
    try {
      await invoke("run_app_via_vpn", { appId: app.id });
    } catch (err) {
      setError(String(err));
    }
  };

  const onClearLogs = async () => {
    setError(null);
    try {
      await invoke("clear_logs");
      setLogs([]);
    } catch (err) {
      setError(String(err));
    }
  };

  const onStartPolkit = async () => {
    setError(null);
    try {
      await invoke("start_polkit_agent");
      const status = await invoke<PolkitStatus>("check_polkit_agent");
      setPolkit(status);
    } catch (err) {
      setError(String(err));
    }
  };

  const onEnablePolkitAutostart = async () => {
    setError(null);
    try {
      await invoke("enable_polkit_autostart");
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="app">
      <header className="app-header">
        <div>
          <p className="app-eyebrow">sillyvpn</p>
          <h1>Split tunneling for WireGuard</h1>
        </div>
        <div className="status-pill" data-connected={state.vpn_enabled}>
          {state.vpn_enabled ? "Connected" : "Disconnected"}
        </div>
      </header>

      <div className="grid">
        {polkit && !polkit.running && (
          <section className="card polkit-card">
            <div className="card-header">
              <h2>Polkit agent</h2>
              <span className="muted">required</span>
            </div>
            <p className="muted">
              Polkit agent is not running. Privileged actions will fail.
            </p>
            <div className="status-actions">
              <button className="ghost" onClick={onStartPolkit}>
                Start agent
              </button>
              <button className="ghost" onClick={onEnablePolkitAutostart}>
                Enable autostart
              </button>
            </div>
          </section>
        )}
        <section className="card status-card">
          <div className="card-header">
            <h2>Status</h2>
            <span className="muted">live</span>
          </div>
          <div className="status-block">
            <div>
              <p className="label">Tunnel</p>
              <p className="value">{selectedTunnel?.name ?? "None"}</p>
            </div>
            <div>
              <p className="label">Local IP</p>
              <p className="value">
                {state.vpn_enabled ? "via namespace" : "-"}
              </p>
            </div>
          </div>
          {error && <div className="error">{error}</div>}
          <div className="status-actions">
            <button
              className="primary"
              onClick={state.vpn_enabled ? onDisable : onEnable}
              disabled={busy}
            >
              {state.vpn_enabled ? "Disable VPN" : "Enable VPN"}
            </button>
          </div>
        </section>

        <section className="card config-card">
          <div className="card-header">
            <h2>Tunnel configuration</h2>
            <button onClick={onImport} className="ghost" disabled={busy}>
              Import .conf
            </button>
          </div>
          <div className="field">
            <label>Available tunnels</label>
            <select
              value={selectedTunnelId}
              onChange={async (event) => {
                const value = event.target.value;
                setSelectedTunnelId(value);
                if (value) {
                  await invoke("set_last_tunnel", { tunnelId: value });
                }
              }}
            >
              <option value="">Select tunnel</option>
              {state.tunnels.map((tunnel) => (
                <option key={tunnel.id} value={tunnel.id}>
                  {tunnel.name}
                </option>
              ))}
            </select>
          </div>

          <div className="list-header">
            <h3>VPN apps</h3>
          </div>
          <div className="manual-add">
            <div className="field">
              <label>App path</label>
              <div className="manual-row">
                <input
                  type="text"
                  placeholder="/usr/bin/discord"
                  value={manualAppPath}
                  onChange={(event) => setManualAppPath(event.target.value)}
                />
                <button
                  className="ghost"
                  onClick={onAddAppManual}
                  disabled={busy}
                >
                  Add by path
                </button>
              </div>
            </div>
          </div>
          <p className="hint">
            Close the app before running via VPN. Existing instances will not be
            captured.
          </p>
          <div className="app-list">
            {state.apps.length === 0 && (
              <p className="muted">No apps configured yet.</p>
            )}
            {state.apps.map((app) => (
              <div
                className={`app-row ${selectedAppId === app.id ? "selected" : ""}`}
                key={app.id}
                onClick={async () => {
                  setSelectedAppId(app.id);
                  await invoke("set_last_app", { appId: app.id });
                }}
              >
                <div className="app-info">
                  <p className="value">{app.label}</p>
                  <p className="muted">{app.path}</p>
                </div>
                <div className="row-actions">
                  <button
                    className="ghost"
                    onClick={(event) => {
                      event.stopPropagation();
                      onRun(app);
                    }}
                    disabled={!state.vpn_enabled}
                  >
                    Run via VPN
                  </button>
                  <button
                    className="danger"
                    onClick={(event) => {
                      event.stopPropagation();
                      onRemoveApp(app);
                    }}
                    disabled={false}
                  >
                    Remove
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="card logs logs-card">
          <div className="card-header">
            <h2>Logs</h2>
            <div className="log-actions">
              <span className="muted">last {logs.length} lines</span>
              <button className="ghost" onClick={onClearLogs}>
                Clear
              </button>
            </div>
          </div>
          <div className="log-window">
            {logs.length === 0 && (
              <p className="muted">No logs yet.</p>
            )}
            {logs.map((line, index) => (
              <p key={`${line}-${index}`}>{line}</p>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
