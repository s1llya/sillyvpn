export type Tunnel = {
  id: string;
  name: string;
  path: string;
};

export type AppItem = {
  id: string;
  label: string;
  path: string;
};

export type AppState = {
  tunnels: Tunnel[];
  apps: AppItem[];
  last_tunnel_id?: string | null;
  last_app_id?: string | null;
  vpn_enabled: boolean;
};

export type PolkitStatus = {
  running: boolean;
  detail: string;
};
