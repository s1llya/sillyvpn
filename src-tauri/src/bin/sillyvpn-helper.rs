use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const NS_NAME: &str = "sillyvpn-ns";
const VETH_HOST: &str = "svpn0";
const VETH_NS: &str = "svpn1";
const VETH_HOST_IP: &str = "10.200.0.1/24";
const VETH_NS_IP: &str = "10.200.0.2/24";
const VETH_SUBNET: &str = "10.200.0.0/24";
const TABLE_ID: &str = "51820";
const FWMARK: &str = "0x51";
const STATE_DIR: &str = "/run/sillyvpn";
const STATE_FILE: &str = "/run/sillyvpn/state.json";
const NETNS_ETC_DIR: &str = "/etc/netns/sillyvpn-ns";

#[derive(Debug)]
struct HelperState {
  wg_ifname: String,
  config_path: String,
  temp_config: String,
  ip_forward_prev: String,
}

fn main() {
  if let Err(err) = run() {
    eprintln!("sillyvpn-helper error: {err}");
    std::process::exit(1);
  }
}

fn run() -> Result<(), String> {
  let mut args = std::env::args().skip(1);
  let cmd = args.next().ok_or("missing command")?;
  match cmd.as_str() {
    "enable" => {
      let mut config = None;
      let mut ifname = None;
      while let Some(arg) = args.next() {
        match arg.as_str() {
          "--config" => config = args.next(),
          "--ifname" => ifname = args.next(),
          _ => return Err(format!("unknown argument: {arg}")),
        }
      }
      let config = config.ok_or("--config missing")?;
      let ifname = ifname.ok_or("--ifname missing")?;
      enable(Path::new(&config), &ifname)
    }
    "disable" => disable(),
    "run" => {
      let mut bins: Vec<String> = Vec::new();
      let mut envs: Vec<(String, String)> = Vec::new();
      while let Some(arg) = args.next() {
        match arg.as_str() {
          "--bin" => {
            if let Some(value) = args.next() {
              bins.push(value);
            } else {
              return Err("--bin missing value".into());
            }
          }
          "--env" => {
            let pair = args.next().ok_or("--env missing value")?;
            if let Some((key, value)) = parse_env_pair(&pair)? {
              envs.push((key, value));
            }
          }
          _ => return Err(format!("unknown argument: {arg}")),
        }
      }
      if bins.is_empty() {
        return Err("--bin missing".into());
      }
      for bin in bins {
        run_in_namespace(Path::new(&bin), &envs)?;
      }
      Ok(())
    }
    _ => Err(format!("unknown command: {cmd}")),
  }
}

fn enable(config_path: &Path, _ifname: &str) -> Result<(), String> {
  if !config_path.exists() {
    return Err("config does not exist".into());
  }

  fs::create_dir_all(STATE_DIR).map_err(|e| e.to_string())?;
  let temp_config = Path::new(STATE_DIR).join("wg-temp.conf");
  let (temp_config, dns_servers) = sanitize_config(config_path, &temp_config)?;
  let ifname = temp_config
    .file_stem()
    .and_then(|s| s.to_str())
    .unwrap_or("wg-temp")
    .to_string();

  let ip_forward_prev = read_ip_forward()?;
  write_ip_forward("1")?;

  let _ = run_cmd("ip", &["link", "del", VETH_HOST]);
  let _ = run_cmd("ip", &["netns", "del", NS_NAME]);

  let result = (|| -> Result<(), String> {
    run_cmd("ip", &["netns", "add", NS_NAME])?;
    setup_dns_for_namespace(&dns_servers)?;
    run_cmd(
      "ip",
      &["link", "add", VETH_HOST, "type", "veth", "peer", "name", VETH_NS],
    )?;
    run_cmd("ip", &["link", "set", VETH_NS, "netns", NS_NAME])?;
    run_cmd("ip", &["addr", "add", VETH_HOST_IP, "dev", VETH_HOST])?;
    run_cmd("ip", &["link", "set", VETH_HOST, "up"])?;
    run_cmd(
      "ip",
      &["netns", "exec", NS_NAME, "ip", "addr", "add", VETH_NS_IP, "dev", VETH_NS],
    )?;
    run_cmd(
      "ip",
      &["netns", "exec", NS_NAME, "ip", "link", "set", VETH_NS, "up"],
    )?;
    run_cmd(
      "ip",
      &[
        "netns",
        "exec",
        NS_NAME,
        "ip",
        "route",
        "add",
        "default",
        "via",
        "10.200.0.1",
      ],
    )?;

    run_cmd("wg-quick", &["up", temp_config.to_str().unwrap()])?;

    run_cmd("ip", &["rule", "add", "fwmark", FWMARK, "table", TABLE_ID])?;
    run_cmd(
      "ip",
      &[
        "route",
        "add",
        "default",
        "dev",
        &ifname,
        "table",
        TABLE_ID,
      ],
    )?;
    run_cmd(
      "iptables",
      &[
        "-t",
        "mangle",
        "-A",
        "PREROUTING",
        "-i",
        VETH_HOST,
        "-j",
        "MARK",
        "--set-mark",
        FWMARK,
      ],
    )?;
    run_cmd(
      "iptables",
      &[
        "-A",
        "FORWARD",
        "-i",
        VETH_HOST,
        "-o",
        &ifname,
        "-j",
        "ACCEPT",
      ],
    )?;
    run_cmd(
      "iptables",
      &[
        "-A",
        "FORWARD",
        "-i",
        &ifname,
        "-o",
        VETH_HOST,
        "-j",
        "ACCEPT",
      ],
    )?;
    run_cmd(
      "iptables",
      &[
        "-t",
        "nat",
        "-A",
        "POSTROUTING",
        "-s",
        VETH_SUBNET,
        "-o",
        &ifname,
        "-j",
        "MASQUERADE",
      ],
    )?;

    let state = HelperState {
      wg_ifname: ifname.to_string(),
      config_path: config_path.to_string_lossy().to_string(),
      temp_config: temp_config.to_string_lossy().to_string(),
      ip_forward_prev: ip_forward_prev.clone(),
    };
    write_state(&state)?;
    Ok(())
  })();

  if let Err(err) = result {
    cleanup_best_effort();
    let _ = cleanup_dns_for_namespace();
    let _ = write_ip_forward(&ip_forward_prev);
    let _ = run_cmd(
      "iptables",
      &[
        "-D",
        "FORWARD",
        "-i",
        VETH_HOST,
        "-o",
        &ifname,
        "-j",
        "ACCEPT",
      ],
    );
    let _ = run_cmd(
      "iptables",
      &[
        "-D",
        "FORWARD",
        "-i",
        &ifname,
        "-o",
        VETH_HOST,
        "-j",
        "ACCEPT",
      ],
    );
    let _ = run_cmd(
      "iptables",
      &[
        "-t",
        "nat",
        "-D",
        "POSTROUTING",
        "-s",
        VETH_SUBNET,
        "-o",
        &ifname,
        "-j",
        "MASQUERADE",
      ],
    );
    let _ = run_cmd("wg-quick", &["down", temp_config.to_str().unwrap()]);
    return Err(err);
  }

  Ok(())
}

fn disable() -> Result<(), String> {
  let state = match read_state() {
    Ok(state) => state,
    Err(_) => {
      cleanup_best_effort();
      return Ok(());
    }
  };

  let _ = run_cmd(
    "iptables",
    &[
      "-t",
      "mangle",
      "-D",
      "PREROUTING",
      "-i",
      VETH_HOST,
      "-j",
      "MARK",
      "--set-mark",
      FWMARK,
    ],
  );
  let _ = run_cmd(
    "iptables",
    &[
      "-D",
      "FORWARD",
      "-i",
      VETH_HOST,
      "-o",
      &state.wg_ifname,
      "-j",
      "ACCEPT",
    ],
  );
  let _ = run_cmd(
    "iptables",
    &[
      "-D",
      "FORWARD",
      "-i",
      &state.wg_ifname,
      "-o",
      VETH_HOST,
      "-j",
      "ACCEPT",
    ],
  );
  let _ = run_cmd(
    "iptables",
    &[
      "-t",
      "nat",
      "-D",
      "POSTROUTING",
      "-s",
      VETH_SUBNET,
      "-o",
      &state.wg_ifname,
      "-j",
      "MASQUERADE",
    ],
  );
  let _ = run_cmd("ip", &["rule", "del", "fwmark", FWMARK, "table", TABLE_ID]);
  let _ = run_cmd(
    "ip",
    &["route", "del", "default", "dev", &state.wg_ifname, "table", TABLE_ID],
  );
  let _ = run_cmd("wg-quick", &["down", &state.temp_config]);

  cleanup_best_effort();
  write_ip_forward(&state.ip_forward_prev)?;
  let _ = fs::remove_file(STATE_FILE);
  Ok(())
}

fn run_in_namespace(bin: &Path, envs: &[(String, String)]) -> Result<(), String> {
  if !bin.exists() {
    return Err("binary does not exist".into());
  }
  let (launcher, use_setsid) = find_setsid();
  let mut cmd = if use_setsid {
    let mut cmd = Command::new(launcher);
    cmd.arg("/usr/bin/ip");
    cmd
  } else {
    Command::new("/usr/bin/ip")
  };
  cmd.args(["netns", "exec", NS_NAME]);
  if let Some((uid, gid)) = caller_identity() {
    if let Some(setpriv) = find_setpriv() {
      cmd.arg(setpriv);
      cmd.args([
        "--reuid",
        &uid,
        "--regid",
        &gid,
        "--init-groups",
        "--inh-caps",
        "-all",
      ]);
    }
  }
  cmd.arg(bin);
  for (key, value) in envs {
    cmd.env(key, value);
  }
  cmd.stdin(Stdio::null());
  cmd.stdout(Stdio::null());
  cmd.stderr(Stdio::null());
  cmd.spawn().map_err(|e| e.to_string())?;
  Ok(())
}

fn sanitize_config(original: &Path, dest: &Path) -> Result<(PathBuf, Vec<String>), String> {
  let mut content = String::new();
  fs::File::open(original)
    .map_err(|e| e.to_string())?
    .read_to_string(&mut content)
    .map_err(|e| e.to_string())?;
  let dns_servers = extract_dns_servers(&content);
  let has_table = content.lines().any(|line| {
    let normalized = line.trim().replace(' ', "").to_ascii_lowercase();
    normalized == "table=off"
  });

  let mut output = String::new();
  let mut inserted = false;
  for line in content.lines() {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("dns=") || lower.starts_with("dns =") {
      continue;
    }
    output.push_str(line);
    output.push('\n');
    if !inserted && trimmed == "[Interface]" {
      if !has_table {
        output.push_str("Table = off\n");
      }
      inserted = true;
    }
  }

  fs::write(dest, output).map_err(|e| e.to_string())?;
  let mut perms = fs::metadata(dest).map_err(|e| e.to_string())?.permissions();
  perms.set_mode(0o600);
  fs::set_permissions(dest, perms).map_err(|e| e.to_string())?;
  Ok((dest.to_path_buf(), dns_servers))
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<(), String> {
  let output = Command::new(cmd)
    .args(args)
    .output()
    .map_err(|e| format!("{cmd} failed to start: {e}"))?;
  if output.status.success() {
    Ok(())
  } else {
    Err(format!(
      "{cmd} error: {}",
      String::from_utf8_lossy(&output.stderr)
    ))
  }
}

fn parse_env_pair(pair: &str) -> Result<Option<(String, String)>, String> {
  let mut parts = pair.splitn(2, '=');
  let key = parts.next().unwrap_or("").trim();
  let value = parts.next().unwrap_or("").to_string();
  if key.is_empty() {
    return Err("empty env key".into());
  }
  if !allowed_env_key(key) {
    return Ok(None);
  }
  Ok(Some((key.to_string(), value)))
}

fn allowed_env_key(key: &str) -> bool {
  matches!(
    key,
    "DISPLAY"
      | "WAYLAND_DISPLAY"
      | "XAUTHORITY"
      | "XDG_RUNTIME_DIR"
      | "DBUS_SESSION_BUS_ADDRESS"
      | "HOME"
      | "USER"
      | "LOGNAME"
      | "PATH"
  )
}

fn caller_identity() -> Option<(String, String)> {
  let uid = std::env::var("PKEXEC_UID").ok()?;
  let gid = gid_for_uid(&uid).unwrap_or_else(|| uid.clone());
  Some((uid, gid))
}

fn gid_for_uid(uid: &str) -> Option<String> {
  let content = fs::read_to_string("/etc/passwd").ok()?;
  for line in content.lines() {
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 4 {
      continue;
    }
    if parts[2] == uid {
      return Some(parts[3].to_string());
    }
  }
  None
}

fn find_setpriv() -> Option<&'static str> {
  if Path::new("/usr/bin/setpriv").exists() {
    return Some("/usr/bin/setpriv");
  }
  if Path::new("/bin/setpriv").exists() {
    return Some("/bin/setpriv");
  }
  None
}

fn find_setsid() -> (&'static str, bool) {
  if Path::new("/usr/bin/setsid").exists() {
    return ("/usr/bin/setsid", true);
  }
  if Path::new("/bin/setsid").exists() {
    return ("/bin/setsid", true);
  }
  ("/usr/bin/ip", false)
}

fn read_ip_forward() -> Result<String, String> {
  let mut content = String::new();
  fs::File::open("/proc/sys/net/ipv4/ip_forward")
    .map_err(|e| e.to_string())?
    .read_to_string(&mut content)
    .map_err(|e| e.to_string())?;
  Ok(content.trim().to_string())
}

fn write_ip_forward(value: &str) -> Result<(), String> {
  fs::File::create("/proc/sys/net/ipv4/ip_forward")
    .and_then(|mut file| file.write_all(value.as_bytes()))
    .map_err(|e| e.to_string())
}

fn write_state(state: &HelperState) -> Result<(), String> {
  let json = format!(
    "{{\"wg_ifname\":\"{}\",\"config_path\":\"{}\",\"temp_config\":\"{}\",\"ip_forward_prev\":\"{}\"}}",
    state.wg_ifname, state.config_path, state.temp_config, state.ip_forward_prev
  );
  fs::write(STATE_FILE, json).map_err(|e| e.to_string())?;
  Ok(())
}

fn read_state() -> Result<HelperState, String> {
  let mut content = String::new();
  fs::File::open(STATE_FILE)
    .map_err(|e| e.to_string())?
    .read_to_string(&mut content)
    .map_err(|e| e.to_string())?;
  let value: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
  Ok(HelperState {
    wg_ifname: value["wg_ifname"].as_str().unwrap_or("wg0").to_string(),
    config_path: value["config_path"].as_str().unwrap_or("").to_string(),
    temp_config: value["temp_config"].as_str().unwrap_or("").to_string(),
    ip_forward_prev: value["ip_forward_prev"].as_str().unwrap_or("0").to_string(),
  })
}

fn cleanup_best_effort() {
  let _ = run_cmd("ip", &["link", "del", VETH_HOST]);
  let _ = run_cmd("ip", &["netns", "del", NS_NAME]);
  let _ = cleanup_dns_for_namespace();
}

fn setup_dns_for_namespace(dns_servers: &[String]) -> Result<(), String> {
  fs::create_dir_all(NETNS_ETC_DIR).map_err(|e| e.to_string())?;
  let mut lines = String::new();
  if dns_servers.is_empty() {
    lines.push_str("nameserver 1.1.1.1\n");
    lines.push_str("nameserver 8.8.8.8\n");
  } else {
    for server in dns_servers {
      lines.push_str(&format!("nameserver {server}\n"));
    }
  }
  fs::write(format!("{NETNS_ETC_DIR}/resolv.conf"), lines).map_err(|e| e.to_string())?;
  Ok(())
}

fn extract_dns_servers(content: &str) -> Vec<String> {
  let mut servers = Vec::new();
  for line in content.lines() {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("dns=") && !lower.starts_with("dns =") {
      continue;
    }
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() < 2 {
      continue;
    }
    for raw in parts[1]
      .split(|c: char| c == ',' || c.is_whitespace())
      .filter(|s| !s.is_empty())
    {
      servers.push(raw.to_string());
    }
  }
  servers
}

fn cleanup_dns_for_namespace() -> Result<(), String> {
  let _ = fs::remove_file(format!("{NETNS_ETC_DIR}/resolv.conf"));
  let _ = fs::remove_dir(NETNS_ETC_DIR);
  Ok(())
}
