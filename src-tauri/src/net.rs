//! Adapters, routes, Wi-Fi link info, per-process connections.
use crate::ps::{ps_quote, run_cmd, run_ps, run_ps_json};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::Ipv4Addr;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Adapter {
    pub name: String,
    pub description: String,
    pub status: String,
    pub link_speed: String,
    pub if_index: u32,
    pub mac: String,
    pub metric: Option<u32>,
    pub automatic_metric: Option<bool>,
    pub connected: bool,
    pub ipv4: Vec<String>,
    pub dns: Vec<String>,
    pub role: String, // wifi | ethernet | happ | radmin | virtual | other
    pub rx_bps: f64,
    pub tx_bps: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Route {
    pub prefix: String,
    pub next_hop: String,
    pub interface: String,
    pub if_index: u32,
    pub route_metric: u32,
    pub interface_metric: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawAdapter {
    name: String,
    interface_description: Option<String>,
    status: Option<serde_json::Value>,
    link_speed: Option<String>,
    if_index: Option<u32>,
    mac_address: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawIpIf {
    interface_alias: String,
    interface_metric: Option<u32>,
    automatic_metric: Option<serde_json::Value>,
    connection_state: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawIp {
    interface_alias: String,
    #[serde(rename = "IPAddress")]
    ip_address: Option<String>,
    prefix_length: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDns {
    interface_alias: String,
    server_addresses: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawNetBundle {
    adapters: Vec<RawAdapter>,
    ipifs: Vec<RawIpIf>,
    ips: Vec<RawIp>,
    dns: Vec<RawDns>,
}

fn json_bool(v: &Option<serde_json::Value>) -> Option<bool> {
    match v {
        Some(serde_json::Value::Bool(b)) => Some(*b),
        Some(serde_json::Value::Number(n)) => n.as_i64().map(|x| x == 1),
        Some(serde_json::Value::String(s)) => Some(s == "Enabled" || s == "True"),
        _ => None,
    }
}

fn status_str(v: &Option<serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => match n.as_i64() {
            Some(1) => "Up".into(),
            Some(2) => "Disconnected".into(),
            Some(0) => "Not Present".into(),
            _ => "Unknown".into(),
        },
        _ => "Unknown".into(),
    }
}

pub fn classify(name: &str, desc: &str) -> String {
    let n = name.to_lowercase();
    let d = desc.to_lowercase();
    if n.contains("radmin") || d.contains("radmin") {
        "radmin".into()
    } else if n.contains("happ") || n == "vgate0" || d.contains("wintun") || d.contains("tun") && !d.contains("bluetooth") {
        "happ".into()
    } else if d.contains("wifi") || d.contains("wi-fi") || d.contains("wireless") || d.contains("802.11") || n.contains("беспровод") {
        "wifi".into()
    } else if d.contains("hyper-v") || d.contains("virtual") || d.contains("tap-windows") || d.contains("anyconnect") || n.contains("vethernet") {
        "virtual".into()
    } else if d.contains("bluetooth") {
        "other".into()
    } else if d.contains("ethernet") || d.contains("gbe") || d.contains("realtek pcie") {
        "ethernet".into()
    } else {
        "other".into()
    }
}

pub fn adapters() -> Result<Vec<Adapter>> {
    let script = r#"
$a = @(Get-NetAdapter | Select-Object Name, InterfaceDescription, Status, LinkSpeed, ifIndex, MacAddress)
$i = @(Get-NetIPInterface -AddressFamily IPv4 | Select-Object InterfaceAlias, InterfaceMetric, AutomaticMetric, ConnectionState)
$p = @(Get-NetIPAddress -AddressFamily IPv4 | Select-Object InterfaceAlias, IPAddress, PrefixLength)
$d = @(Get-DnsClientServerAddress -AddressFamily IPv4 | Select-Object InterfaceAlias, ServerAddresses)
@{ adapters = $a; ipifs = $i; ips = $p; dns = $d } | ConvertTo-Json -Depth 4 -Compress
"#;
    let raw: RawNetBundle = run_ps_json(script)?;
    let mut out = Vec::new();
    for a in raw.adapters {
        let desc = a.interface_description.clone().unwrap_or_default();
        let mut ad = Adapter {
            role: classify(&a.name, &desc),
            name: a.name.clone(),
            description: desc,
            status: status_str(&a.status),
            link_speed: a.link_speed.unwrap_or_default(),
            if_index: a.if_index.unwrap_or(0),
            mac: a.mac_address.unwrap_or_default(),
            ..Default::default()
        };
        if let Some(ipif) = raw.ipifs.iter().find(|x| x.interface_alias == a.name) {
            ad.metric = ipif.interface_metric;
            ad.automatic_metric = json_bool(&ipif.automatic_metric);
            ad.connected = match &ipif.connection_state {
                Some(serde_json::Value::String(s)) => s == "Connected",
                Some(serde_json::Value::Number(n)) => n.as_i64() == Some(1),
                _ => false,
            };
        }
        for ip in raw.ips.iter().filter(|x| x.interface_alias == a.name) {
            let addr = ip.ip_address.clone().unwrap_or_default();
            if !addr.is_empty() {
                ad.ipv4.push(format!("{}/{}", addr, ip.prefix_length.unwrap_or(0)));
            }
        }
        if let Some(d) = raw.dns.iter().find(|x| x.interface_alias == a.name) {
            match &d.server_addresses {
                Some(serde_json::Value::Array(arr)) => {
                    ad.dns = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                }
                Some(serde_json::Value::String(s)) if !s.is_empty() => ad.dns = vec![s.clone()],
                _ => {}
            }
        }
        out.push(ad);
    }
    // Wi-Fi/ethernet first, then VPNs, then the rest; connected before disconnected.
    let rank = |a: &Adapter| -> (u8, u8) {
        let r = match a.role.as_str() {
            "wifi" => 0,
            "ethernet" => 1,
            "happ" => 2,
            "radmin" => 3,
            "virtual" => 5,
            _ => 6,
        };
        (if a.status == "Up" { 0 } else { 1 }, r)
    };
    out.sort_by_key(|a| rank(a));
    Ok(out)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawRoute {
    destination_prefix: String,
    next_hop: String,
    interface_alias: Option<String>,
    if_index: Option<u32>,
    route_metric: Option<u32>,
    interface_metric: Option<u32>,
}

pub fn routes() -> Result<Vec<Route>> {
    let script = "@(Get-NetRoute -AddressFamily IPv4 | Select-Object DestinationPrefix, NextHop, InterfaceAlias, ifIndex, RouteMetric, InterfaceMetric) | ConvertTo-Json -Compress";
    let raw: Vec<RawRoute> = run_ps_json(script)?;
    let mut v: Vec<Route> = raw
        .into_iter()
        .map(|r| Route {
            prefix: r.destination_prefix,
            next_hop: r.next_hop,
            interface: r.interface_alias.unwrap_or_default(),
            if_index: r.if_index.unwrap_or(0),
            route_metric: r.route_metric.unwrap_or(0),
            interface_metric: r.interface_metric.unwrap_or(0),
        })
        .collect();
    v.sort_by(|a, b| {
        let pa = prefix_len(&a.prefix);
        let pb = prefix_len(&b.prefix);
        pa.cmp(&pb).then((a.route_metric + a.interface_metric).cmp(&(b.route_metric + b.interface_metric)))
    });
    Ok(v)
}

fn prefix_len(p: &str) -> u8 {
    p.split('/').nth(1).and_then(|x| x.parse().ok()).unwrap_or(32)
}

fn parse_prefix(p: &str) -> Option<(u32, u8)> {
    let mut it = p.split('/');
    let ip: Ipv4Addr = it.next()?.parse().ok()?;
    let len: u8 = it.next()?.parse().ok()?;
    Some((u32::from(ip), len))
}

/// Longest-prefix-match lookup: which interface will carry traffic to `ip`.
pub fn egress_for(routes: &[Route], ip: &str) -> Option<String> {
    let addr: Ipv4Addr = ip.parse().ok()?;
    let a = u32::from(addr);
    let mut best: Option<(u8, u32, &Route)> = None;
    for r in routes {
        let Some((net, len)) = parse_prefix(&r.prefix) else { continue };
        let mask: u32 = if len == 0 { 0 } else { u32::MAX << (32 - len as u32) };
        if a & mask != net & mask {
            continue;
        }
        let cost = r.route_metric + r.interface_metric;
        match best {
            None => best = Some((len, cost, r)),
            Some((bl, bc, _)) if len > bl || (len == bl && cost < bc) => best = Some((len, cost, r)),
            _ => {}
        }
    }
    best.map(|(_, _, r)| r.interface.clone())
}

pub fn set_interface_metric(if_index: u32, metric: Option<u32>) -> Result<String> {
    let script = match metric {
        Some(m) => format!("Set-NetIPInterface -InterfaceIndex {if_index} -AddressFamily IPv4 -InterfaceMetric {m}; 'ok'"),
        None => format!("Set-NetIPInterface -InterfaceIndex {if_index} -AddressFamily IPv4 -AutomaticMetric Enabled; 'ok'"),
    };
    run_ps(&script)
}

pub fn set_adapter_enabled(name: &str, enabled: bool) -> Result<String> {
    let cmd = if enabled { "Enable-NetAdapter" } else { "Disable-NetAdapter" };
    run_ps(&format!("{cmd} -Name {} -Confirm:$false; 'ok'", ps_quote(name)))
}

/// Key/value pairs from `netsh wlan show interfaces` (locale independent: we keep raw keys).
pub fn wifi_info() -> Result<Vec<(String, String)>> {
    let out = match run_cmd("netsh", &["wlan", "show", "interfaces"]) {
        Ok(o) => o,
        Err(e) => e.to_string(),
    };
    if out.contains("ms-settings:privacy-location") || out.contains("WlanQueryInterface") {
        return Ok(vec![("__error__".to_string(), "location".to_string())]);
    }
    let mut pairs = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(" : ") {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if !k.is_empty() && !v.is_empty() {
                pairs.push((k, v));
            }
        } else if let Some((k, v)) = line.split_once(": ") {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if !k.is_empty() && !v.is_empty() && k.len() < 48 {
                pairs.push((k, v));
            }
        }
    }
    Ok(pairs)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Connection {
    pub pid: u32,
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub via: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawTcp {
    local_address: String,
    local_port: u32,
    remote_address: String,
    remote_port: u32,
    state: serde_json::Value,
    owning_process: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawUdp {
    local_address: String,
    local_port: u32,
    owning_process: u32,
}

#[derive(Deserialize)]
struct RawConnBundle {
    tcp: Vec<RawTcp>,
    udp: Vec<RawUdp>,
}

pub fn connections(pids: &[u32], routes: &[Route]) -> Result<Vec<Connection>> {
    if pids.is_empty() {
        return Ok(vec![]);
    }
    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let script = format!(
        "$pids = @({list}); $t = @(Get-NetTCPConnection -ErrorAction SilentlyContinue | Where-Object {{ $pids -contains $_.OwningProcess -and $_.State -ne 'Bound' }} | Select-Object LocalAddress, LocalPort, RemoteAddress, RemotePort, State, OwningProcess); $u = @(Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object {{ $pids -contains $_.OwningProcess }} | Select-Object LocalAddress, LocalPort, OwningProcess); @{{ tcp = $t; udp = $u }} | ConvertTo-Json -Depth 3 -Compress"
    );
    let raw: RawConnBundle = run_ps_json(&script)?;
    let mut out = Vec::new();
    for t in raw.tcp {
        let state = match &t.state {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(5) => "Established".into(),
                Some(2) => "Listen".into(),
                Some(9) => "TimeWait".into(),
                _ => n.to_string(),
            },
            _ => String::new(),
        };
        let via = if t.remote_address == "0.0.0.0" || t.remote_address == "::" {
            String::new()
        } else {
            egress_for(routes, &t.remote_address).unwrap_or_default()
        };
        out.push(Connection {
            pid: t.owning_process,
            proto: "TCP".into(),
            local: format!("{}:{}", t.local_address, t.local_port),
            remote: format!("{}:{}", t.remote_address, t.remote_port),
            state,
            via,
        });
    }
    for u in raw.udp {
        let via = if u.local_address == "0.0.0.0" || u.local_address == "::" || u.local_address == "127.0.0.1" {
            String::new()
        } else {
            egress_for(routes, &u.local_address).unwrap_or_default()
        };
        out.push(Connection {
            pid: u.owning_process,
            proto: "UDP".into(),
            local: format!("{}:{}", u.local_address, u.local_port),
            remote: "*".into(),
            state: String::new(),
            via,
        });
    }
    Ok(out)
}

/// System proxy (IE/WinINET) state: what Happ toggles in "системный прокси" mode.
#[allow(dead_code)]
pub fn system_proxy() -> Result<(bool, String)> {
    let script = "$p = Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'; @{ enable = [int]$p.ProxyEnable; server = [string]$p.ProxyServer } | ConvertTo-Json -Compress";
    let v: HashMap<String, serde_json::Value> = run_ps_json(script)?;
    let enable = v.get("enable").and_then(|x| x.as_i64()).unwrap_or(0) == 1;
    let server = v.get("server").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Ok((enable, server))
}
