//! Happ / zapret / Radmin control and status.
use crate::config::Config;
use crate::ps::{ps_quote, run_ps, run_ps_json, spawn_detached};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug, Default)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub service: String,
    pub service_state: String, // Running | Stopped | Missing
    pub gui_running: bool,
    pub gui_pids: Vec<u32>,
    pub detail: String,
    pub mode: String, // Happ: tun | proxy | idle | off
    pub proxied_apps: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawService {
    name: String,
    status: serde_json::Value,
}

pub fn service_states(names: &[&str]) -> Result<Vec<(String, String)>> {
    let list = names.iter().map(|n| ps_quote(n)).collect::<Vec<_>>().join(",");
    let script = format!(
        "@(Get-Service -Name {list} -ErrorAction SilentlyContinue | Select-Object Name, Status) | ConvertTo-Json -Compress"
    );
    let raw: Vec<RawService> = run_ps_json(&script)?;
    Ok(raw
        .into_iter()
        .map(|r| {
            let st = match r.status {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => match n.as_i64() {
                    Some(4) => "Running".into(),
                    Some(1) => "Stopped".into(),
                    Some(2) => "StartPending".into(),
                    Some(3) => "StopPending".into(),
                    _ => n.to_string(),
                },
                _ => "Unknown".into(),
            };
            (r.name, st)
        })
        .collect())
}

/// One PowerShell round-trip for the 5-second tier: service states + system proxy + svchost map.
#[derive(Deserialize)]
struct RawMid {
    services: Vec<RawService>,
    proxy_enable: i64,
    proxy_server: String,
    #[serde(default)]
    proxy_override: String,
    #[serde(default)]
    env_http: String,
    #[serde(default)]
    env_https: String,
    #[serde(default)]
    env_no: String,
}

/// System proxy + user proxy environment variables, read together with the service states.
#[derive(Serialize, Clone, Debug, Default)]
pub struct ProxyEnv {
    pub system_enabled: bool,
    pub system_server: String,
    pub system_override: String,
    pub env_http: String,
    pub env_https: String,
    pub env_no_proxy: String,
}

pub fn mid_status(names: &[&str]) -> Result<(Vec<(String, String)>, ProxyEnv)> {
    let list = names.iter().map(|n| ps_quote(n)).collect::<Vec<_>>().join(",");
    let script = format!(
        "$s = @(Get-Service -Name {list} -ErrorAction SilentlyContinue | Select-Object Name, Status); $p = Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; $e = Get-ItemProperty 'HKCU:\\Environment' -ErrorAction SilentlyContinue; @{{ services = $s; proxy_enable = [int]$p.ProxyEnable; proxy_server = [string]$p.ProxyServer; proxy_override = [string]$p.ProxyOverride; env_http = [string]$e.HTTP_PROXY; env_https = [string]$e.HTTPS_PROXY; env_no = [string]$e.NO_PROXY }} | ConvertTo-Json -Depth 3 -Compress"
    );
    let raw: RawMid = run_ps_json(&script)?;
    let penv = ProxyEnv {
        system_enabled: raw.proxy_enable == 1,
        system_server: raw.proxy_server.clone(),
        system_override: raw.proxy_override.clone(),
        env_http: raw.env_http.clone(),
        env_https: raw.env_https.clone(),
        env_no_proxy: raw.env_no.clone(),
    };
    let svc = raw
        .services
        .into_iter()
        .map(|r| {
            let st = match r.status {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => match n.as_i64() {
                    Some(4) => "Running".into(),
                    Some(1) => "Stopped".into(),
                    Some(2) => "StartPending".into(),
                    Some(3) => "StopPending".into(),
                    _ => n.to_string(),
                },
                _ => "Unknown".into(),
            };
            (r.name, st)
        })
        .collect();
    Ok((svc, penv))
}

/// Is something listening on 127.0.0.1:port? (cheap, no PowerShell)
pub fn port_alive(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_ok()
}

pub const NO_PROXY_DEFAULT: &str = "localhost,127.0.0.1,::1,192.168.*,10.*,172.16.*,172.17.*,172.18.*,172.19.*,172.2*.*,172.30.*,172.31.*,26.*,25.*";

/// Proxy repair actions. Env vars are written straight to HKCU\Environment and announced with a
/// non-blocking WM_SETTINGCHANGE: `[Environment]::SetEnvironmentVariable` uses SendMessageTimeout and
/// can hang for a minute if any window on the desktop is stuck.
const ENV_BROADCAST: &str = "$sig = '[DllImport(\"user32.dll\", CharSet=CharSet.Unicode)] public static extern bool SendNotifyMessageW(IntPtr h, uint m, UIntPtr w, string l);'; $t = Add-Type -MemberDefinition $sig -Name Bc -Namespace Dp -PassThru; [void]$t::SendNotifyMessageW([IntPtr]0xffff, 0x001A, [UIntPtr]::Zero, 'Environment');";

pub fn proxy_fix(action: &str, port: u16) -> Result<String> {
    let script = match action {
        "clear-env" => format!("foreach ($n in 'HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy') {{ Remove-ItemProperty 'HKCU:\\Environment' -Name $n -ErrorAction SilentlyContinue }}; {ENV_BROADCAST} 'Переменные HTTP_PROXY/HTTPS_PROXY убраны (для новых процессов)'"),
        "set-env" => format!("Set-ItemProperty 'HKCU:\\Environment' -Name HTTP_PROXY -Value 'http://127.0.0.1:{port}' -Type String; Set-ItemProperty 'HKCU:\\Environment' -Name HTTPS_PROXY -Value 'http://127.0.0.1:{port}' -Type String; Set-ItemProperty 'HKCU:\\Environment' -Name NO_PROXY -Value '{NO_PROXY_DEFAULT}' -Type String; {ENV_BROADCAST} 'Переменные прокси установлены на 127.0.0.1:{port} с исключениями для локальных сетей и Radmin'"),
        "no-proxy" => format!("$e = Get-ItemProperty 'HKCU:\\Environment' -ErrorAction SilentlyContinue; if ($e.HTTP_PROXY -or $e.HTTPS_PROXY) {{ Set-ItemProperty 'HKCU:\\Environment' -Name NO_PROXY -Value '{NO_PROXY_DEFAULT}' -Type String; {ENV_BROADCAST} }}; $k = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; $o = [string](Get-ItemProperty $k).ProxyOverride; $parts = @($o -split ';'); foreach ($x in '26.*','25.*') {{ if ($parts -notcontains $x) {{ $o = if ($o) {{ \"$x;$o\" }} else {{ $x }} }} }}; Set-ItemProperty $k -Name ProxyOverride -Value $o; 'Исключения добавлены: Radmin (26.*, 25.*) и локальные сети идут мимо прокси'"),
        "disable-system" => "Set-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyEnable -Value 0 -Type DWord; 'Системный прокси выключен (Happ включит его снова при подключении)'".to_string(),
        "enable-system" => format!("$k = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; Set-ItemProperty $k -Name ProxyServer -Value '127.0.0.1:{port}'; Set-ItemProperty $k -Name ProxyEnable -Value 1 -Type DWord; 'Системный прокси включён'"),
        _ => return Err(anyhow!("unknown proxy action {action}")),
    };
    let out = run_ps(&script)?;
    // tell WinINET users (browsers, launchers) that the proxy settings changed
    let _ = run_ps("$sig = '[DllImport(\"wininet.dll\")] public static extern bool InternetSetOption(IntPtr h, int o, IntPtr b, int l);'; $t = Add-Type -MemberDefinition $sig -Name WinInet -Namespace Dp -PassThru; [void]$t::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0); [void]$t::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0)");
    Ok(out)
}

/// pid → running service names (for svchost grouping).
pub fn service_pids() -> Result<std::collections::HashMap<u32, Vec<String>>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct R {
        process_id: u32,
        name: String,
    }
    let raw: Vec<R> = run_ps_json("@(Get-CimInstance Win32_Service -Filter \"State='Running'\" | Select-Object ProcessId, Name) | ConvertTo-Json -Compress")?;
    let mut m: std::collections::HashMap<u32, Vec<String>> = std::collections::HashMap::new();
    for r in raw {
        if r.process_id > 0 {
            m.entry(r.process_id).or_default().push(r.name);
        }
    }
    Ok(m)
}

/// Listening ports of a process (for TgWsProxy).
pub fn listening_ports(pid: u32) -> Vec<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct R {
        local_address: String,
        local_port: u32,
    }
    let raw: Vec<R> = run_ps_json(&format!("@(Get-NetTCPConnection -OwningProcess {pid} -State Listen -ErrorAction SilentlyContinue | Select-Object LocalAddress, LocalPort) | ConvertTo-Json -Compress")).unwrap_or_default();
    raw.into_iter().map(|r| format!("{}:{}", r.local_address, r.local_port)).collect()
}

pub fn set_service(name: &str, running: bool) -> Result<String> {
    let cmd = if running { "Start-Service" } else { "Stop-Service -Force" };
    run_ps(&format!("{cmd} -Name {}; (Get-Service -Name {}).Status.ToString()", ps_quote(name), ps_quote(name)))
}

pub fn kill_process(name: &str) -> Result<String> {
    run_ps(&format!("Stop-Process -Name {} -Force -ErrorAction SilentlyContinue; 'ok'", ps_quote(name.trim_end_matches(".exe"))))
}

pub fn launch(path: &str, args: &[&str], cwd: Option<&str>) -> Result<u32> {
    if !std::path::Path::new(path).exists() {
        return Err(anyhow!("Файл не найден: {path}"));
    }
    spawn_detached(path, args, cwd)
}

/// Happ's generated sing-box config: which exe paths go through the proxy.
#[derive(Serialize, Clone, Debug, Default)]
pub struct HappConfigInfo {
    pub exists: bool,
    pub tun_enabled: bool,
    pub strict_route: bool,
    pub final_outbound: String,
    pub proxied_paths: Vec<String>,
    pub direct_processes: Vec<String>,
}

pub fn happ_config(path: &str) -> HappConfigInfo {
    let Ok(text) = std::fs::read_to_string(path) else { return HappConfigInfo::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return HappConfigInfo::default() };
    let mut info = HappConfigInfo { exists: true, ..Default::default() };
    if let Some(inb) = v.get("inbounds").and_then(|x| x.as_array()) {
        for i in inb {
            if i.get("type").and_then(|t| t.as_str()) == Some("tun") {
                info.tun_enabled = true;
                info.strict_route = i.get("strict_route").and_then(|b| b.as_bool()).unwrap_or(false);
            }
        }
    }
    info.final_outbound = v
        .pointer("/route/final")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(rules) = v.pointer("/route/rules").and_then(|x| x.as_array()) {
        for r in rules {
            let outbound = r.get("outbound").and_then(|o| o.as_str()).unwrap_or("");
            if let Some(paths) = r.get("process_path").and_then(|p| p.as_array()) {
                if outbound == "proxy" {
                    info.proxied_paths.extend(paths.iter().filter_map(|p| p.as_str().map(String::from)));
                }
            }
            if let Some(names) = r.get("process_name").and_then(|p| p.as_array()) {
                if outbound == "direct" {
                    info.direct_processes.extend(names.iter().filter_map(|p| p.as_str().map(String::from)));
                }
            }
        }
    }
    info
}

/// Rewrite the `process_path -> proxy` rule in Happ's config so that it matches our policies.
/// Returns the new list. Creates a backup next to the file.
pub fn happ_sync_paths(path: &str, add: &[String], remove: &[String]) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    let rules = v
        .pointer_mut("/route/rules")
        .and_then(|x| x.as_array_mut())
        .ok_or_else(|| anyhow!("В конфиге Happ нет route.rules"))?;
    let mut found = false;
    let mut result = Vec::new();
    for r in rules.iter_mut() {
        if r.get("outbound").and_then(|o| o.as_str()) == Some("proxy") && r.get("process_path").is_some() {
            found = true;
            let mut list: Vec<String> = r["process_path"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
                .unwrap_or_default();
            list.retain(|p| !remove.iter().any(|x| x.eq_ignore_ascii_case(p)));
            for a in add {
                if !list.iter().any(|x| x.eq_ignore_ascii_case(a)) {
                    list.push(a.clone());
                }
            }
            r["process_path"] = serde_json::Value::Array(list.iter().map(|s| serde_json::Value::String(s.clone())).collect());
            result = list;
        }
    }
    if !found {
        let list: Vec<String> = add.to_vec();
        rules.insert(
            1.min(rules.len()),
            serde_json::json!({ "outbound": "proxy", "process_path": list }),
        );
        result = add.to_vec();
    }
    let backup = format!("{path}.dotpilot.bak");
    let _ = std::fs::write(&backup, &text);
    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(result)
}

pub fn zapret_start(cfg: &Config) -> Result<String> {
    // Prefer the installed service; fall back to general.bat
    if let Ok(states) = service_states(&[&cfg.zapret_service]) {
        if !states.is_empty() {
            return set_service(&cfg.zapret_service, true);
        }
    }
    let bat = format!("{}\\general.bat", cfg.zapret_dir);
    if std::path::Path::new(&bat).exists() {
        spawn_detached("cmd.exe", &["/c", "start", "", "/min", &bat], Some(&cfg.zapret_dir))?;
        return Ok("started general.bat".into());
    }
    Err(anyhow!("Не найден ни сервис zapret, ни general.bat в {}", cfg.zapret_dir))
}

pub fn zapret_stop(cfg: &Config) -> Result<String> {
    let mut msgs = Vec::new();
    if let Ok(states) = service_states(&[&cfg.zapret_service]) {
        if !states.is_empty() {
            msgs.push(set_service(&cfg.zapret_service, false)?);
        }
    }
    msgs.push(kill_process("winws")?);
    Ok(msgs.join("; "))
}
