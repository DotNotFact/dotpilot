//! Per-app policy engine: QoS (DSCP / throttle), Happ sync, game mode, tweaks.
use crate::config::{Config, Policy};
use crate::ps::{ps_quote, run_ps, run_ps_json};
use crate::services;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "DotPilot-";

#[derive(Serialize, Clone, Debug)]
pub struct ApplyReport {
    pub ok: bool,
    pub lines: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct QosPolicy {
    pub name: String,
    pub app_path_name_match_condition: Option<String>,
    #[serde(rename = "DSCPAction")]
    pub dscp_action: Option<i32>,
    pub throttle_rate_action_bits_per_second: Option<u64>,
}

pub fn list_qos() -> Result<Vec<QosPolicy>> {
    run_ps_json(
        "@(Get-NetQosPolicy -ErrorAction SilentlyContinue | Select-Object Name, AppPathNameMatchCondition, DSCPAction, ThrottleRateActionBitsPerSecond) | ConvertTo-Json -Compress",
    )
}

/// Expand configured paths to concrete exe files: a file, a directory (2 levels), or a name prefix
/// such as `C:\Program Files\WindowsApps\Claude_` (every folder starting with it).
pub fn expand_exes(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    for p in paths {
        if p.to_lowercase().ends_with(".exe") {
            out.push(p.clone());
        } else if std::path::Path::new(p).is_dir() {
            dirs.push(p.clone());
        } else if let (Some(parent), Some(prefix)) = (std::path::Path::new(p).parent(), std::path::Path::new(p).file_name()) {
            let prefix = prefix.to_string_lossy().to_lowercase();
            if let Ok(rd) = std::fs::read_dir(parent) {
                for e in rd.flatten() {
                    if e.path().is_dir() && e.file_name().to_string_lossy().to_lowercase().starts_with(&prefix) {
                        dirs.push(e.path().to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    for p in &dirs {
        if let Ok(rd) = std::fs::read_dir(p) {
            // directory: take exe files up to 2 levels deep
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                    out.push(path.to_string_lossy().to_string());
                } else if path.is_dir() {
                    if let Ok(rd2) = std::fs::read_dir(&path) {
                        for e2 in rd2.flatten() {
                            let p2 = e2.path();
                            if p2.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                                out.push(p2.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
            // and deeper "bin" folders for JRE-like layouts
            for cand in ["x64\\bin\\javaw.exe", "x64\\bin\\java.exe", "bin\\javaw.exe", "bin\\java.exe"] {
                if let Ok(rd) = std::fs::read_dir(p) {
                    for e in rd.flatten() {
                        let c = e.path().join(cand);
                        if c.exists() {
                            out.push(c.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// Recreate all DotPilot QoS policies from the config.
/// `game_mode` = whether throttles for background apps should be active.
pub fn apply_qos(cfg: &Config, game_mode: bool) -> Result<Vec<String>> {
    let mut script = String::new();
    // DSCP marking on non-domain PCs requires this registry value.
    script.push_str(
        "New-Item -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Force | Out-Null; Set-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Name 'Do not use NLA' -Value '1' -Type String;\n",
    );
    script.push_str(&format!(
        "Get-NetQosPolicy -ErrorAction SilentlyContinue | Where-Object {{ $_.Name -like '{PREFIX}*' }} | Remove-NetQosPolicy -Confirm:$false -ErrorAction SilentlyContinue;\n"
    ));
    let mut lines = Vec::new();
    for app in &cfg.apps {
        let exes = expand_exes(&app.exe_paths);
        let throttle = if game_mode && app.background { app.throttle_mbps } else { None };
        if app.dscp.is_none() && throttle.is_none() {
            continue;
        }
        for (i, exe) in exes.iter().enumerate() {
            let name = format!("{PREFIX}{}-{i}", app.id);
            let mut cmd = format!(
                "New-NetQosPolicy -Name {} -AppPathNameMatchCondition {} -IPProtocolMatchCondition Both -NetworkProfile All",
                ps_quote(&name),
                ps_quote(exe)
            );
            if let Some(d) = app.dscp {
                cmd.push_str(&format!(" -DSCPAction {d}"));
            }
            if let Some(t) = throttle {
                cmd.push_str(&format!(" -ThrottleRateActionBitsPerSecond {}", t as u64 * 1_000_000));
            }
            cmd.push_str(" -ErrorAction Stop | Out-Null;\n");
            script.push_str(&cmd);
            let what = match (app.dscp, throttle) {
                (Some(d), Some(t)) => format!("DSCP {d}, лимит {t} Мбит/с"),
                (Some(d), None) => format!("DSCP {d}"),
                (None, Some(t)) => format!("лимит {t} Мбит/с"),
                _ => String::new(),
            };
            lines.push(format!("{}: {} → {}", app.name, file_name(exe), what));
        }
    }
    script.push_str("'ok'");
    run_ps(&script)?;
    Ok(lines)
}

#[allow(dead_code)]
pub fn remove_qos() -> Result<()> {
    run_ps(&format!(
        "Get-NetQosPolicy -ErrorAction SilentlyContinue | Where-Object {{ $_.Name -like '{PREFIX}*' }} | Remove-NetQosPolicy -Confirm:$false -ErrorAction SilentlyContinue; 'ok'"
    ))?;
    Ok(())
}

fn file_name(p: &str) -> String {
    p.rsplit('\\').next().unwrap_or(p).to_string()
}

/// Sync Happ's per-app proxy list with our policies (Vpn → proxied, Direct/Radmin → removed).
pub fn sync_happ(cfg: &Config) -> Result<Vec<String>> {
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for app in &cfg.apps {
        let exes = expand_exes(&app.exe_paths);
        match app.policy {
            Policy::Vpn => add.extend(exes),
            Policy::Direct | Policy::Radmin => remove.extend(exes),
        }
    }
    let list = services::happ_sync_paths(&cfg.happ_config_path, &add, &remove)?;
    Ok(list)
}

/// Full policy application. Returns a human readable report.
pub fn apply_all(cfg: &Config, game_mode: bool) -> ApplyReport {
    let mut lines = Vec::new();
    let mut ok = true;
    match apply_qos(cfg, game_mode) {
        Ok(l) => {
            lines.push(format!("QoS: создано правил — {}", l.len()));
            lines.extend(l.into_iter().map(|s| format!("  • {s}")));
        }
        Err(e) => {
            ok = false;
            lines.push(format!("QoS ошибка: {e}"));
        }
    }
    if cfg.sync_happ_config {
        match sync_happ(cfg) {
            Ok(list) => {
                lines.push(format!("Happ: список приложений через прокси обновлён ({} записей). Переподключи Happ, чтобы применить.", list.len()));
            }
            Err(e) => {
                ok = false;
                lines.push(format!("Happ sync ошибка: {e}"));
            }
        }
    }
    // Radmin priority check
    if cfg.apps.iter().any(|a| a.policy == Policy::Radmin) {
        let script = format!(
            "$i = Get-NetIPInterface -InterfaceAlias {} -AddressFamily IPv4 -ErrorAction SilentlyContinue; if ($i) {{ if ($i.InterfaceMetric -gt 5) {{ Set-NetIPInterface -InterfaceAlias {} -AddressFamily IPv4 -InterfaceMetric 1 }}; 'metric=' + (Get-NetIPInterface -InterfaceAlias {} -AddressFamily IPv4).InterfaceMetric }} else {{ 'absent' }}",
            ps_quote(&cfg.radmin_alias),
            ps_quote(&cfg.radmin_alias),
            ps_quote(&cfg.radmin_alias)
        );
        match run_ps(&script) {
            Ok(s) => lines.push(format!("Radmin VPN: интерфейс {} (сеть 26.0.0.0/8 остаётся за Radmin)", s.trim())),
            Err(e) => lines.push(format!("Radmin проверка: {e}")),
        }
    }
    ApplyReport { ok, lines }
}

#[derive(Serialize, Clone, Debug)]
pub struct Tweak {
    pub id: String,
    pub name: String,
    pub description: String,
    pub state: Option<bool>,
    pub reboot: bool,
}

pub fn tweaks_state() -> Result<Vec<Tweak>> {
    let script = r#"
$mm = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile' -ErrorAction SilentlyContinue
$tcp = Get-NetTCPSetting -SettingName Internet
$nagle = $false
$ifs = Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces' -ErrorAction SilentlyContinue
foreach ($k in $ifs) { $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue; if ($p.TcpAckFrequency -eq 1 -and $p.TCPNoDelay -eq 1) { $nagle = $true } }
$qos = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\QoS' -ErrorAction SilentlyContinue
@{
  throttling = ($mm.NetworkThrottlingIndex -eq 4294967295)
  responsiveness = ($mm.SystemResponsiveness -eq 0)
  nagle = $nagle
  autotuning = ($tcp.AutoTuningLevelLocal.ToString() -eq 'Normal')
  ecn = ($tcp.EcnCapability.ToString() -eq 'Enabled')
  nla = ($qos.'Do not use NLA' -eq '1')
} | ConvertTo-Json -Compress
"#;
    let v: serde_json::Value = run_ps_json(script)?;
    let b = |k: &str| v.get(k).and_then(|x| x.as_bool());
    Ok(vec![
        Tweak { id: "throttling".into(), name: "Отключить сетевой троттлинг".into(), description: "NetworkThrottlingIndex = 0xFFFFFFFF: Windows перестаёт ограничивать сетевые пакеты во время мультимедиа/игр.".into(), state: b("throttling"), reboot: true },
        Tweak { id: "responsiveness".into(), name: "Приоритет играм (SystemResponsiveness = 0)".into(), description: "Планировщик MMCSS отдаёт до 100% CPU играм и мультимедиа вместо резерва 20% под фон.".into(), state: b("responsiveness"), reboot: true },
        Tweak { id: "nagle".into(), name: "Отключить алгоритм Нейгла".into(), description: "TcpAckFrequency=1 и TCPNoDelay=1 на всех интерфейсах: маленькие TCP-пакеты уходят сразу, ниже задержка в играх.".into(), state: b("nagle"), reboot: true },
        Tweak { id: "autotuning".into(), name: "Автонастройка окна TCP: Normal".into(), description: "Стандартное значение. Отключение (disabled) иногда уменьшает буферизацию на Wi-Fi, но режет скорость загрузок.".into(), state: b("autotuning"), reboot: false },
        Tweak { id: "ecn".into(), name: "ECN (явное уведомление о перегрузке)".into(), description: "Помогает роутерам с AQM избегать потерь при перегрузке. Включай, если роутер поддерживает.".into(), state: b("ecn"), reboot: false },
        Tweak { id: "nla".into(), name: "DSCP-метки вне домена".into(), description: "Разрешает Windows проставлять приоритет (DSCP) в пакетах на домашнем ПК. Нужно для политик приоритета DotPilot.".into(), state: b("nla"), reboot: false },
    ])
}

pub fn set_tweak(id: &str, on: bool) -> Result<String> {
    let script = match (id, on) {
        ("throttling", true) => "Set-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' -Name NetworkThrottlingIndex -Value 0xffffffff -Type DWord; 'ok'".to_string(),
        ("throttling", false) => "Set-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' -Name NetworkThrottlingIndex -Value 10 -Type DWord; 'ok'".to_string(),
        ("responsiveness", true) => "Set-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' -Name SystemResponsiveness -Value 0 -Type DWord; 'ok'".to_string(),
        ("responsiveness", false) => "Set-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile' -Name SystemResponsiveness -Value 20 -Type DWord; 'ok'".to_string(),
        ("nagle", true) => "Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces' | ForEach-Object { Set-ItemProperty $_.PSPath -Name TcpAckFrequency -Value 1 -Type DWord; Set-ItemProperty $_.PSPath -Name TCPNoDelay -Value 1 -Type DWord }; 'ok'".to_string(),
        ("nagle", false) => "Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces' | ForEach-Object { Remove-ItemProperty $_.PSPath -Name TcpAckFrequency -ErrorAction SilentlyContinue; Remove-ItemProperty $_.PSPath -Name TCPNoDelay -ErrorAction SilentlyContinue }; 'ok'".to_string(),
        ("autotuning", true) => "Set-NetTCPSetting -SettingName Internet -AutoTuningLevelLocal Normal; 'ok'".to_string(),
        ("autotuning", false) => "Set-NetTCPSetting -SettingName Internet -AutoTuningLevelLocal Disabled; 'ok'".to_string(),
        ("ecn", true) => "Set-NetTCPSetting -SettingName Internet -EcnCapability Enabled; 'ok'".to_string(),
        ("ecn", false) => "Set-NetTCPSetting -SettingName Internet -EcnCapability Disabled; 'ok'".to_string(),
        ("nla", true) => "New-Item -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Force | Out-Null; Set-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Name 'Do not use NLA' -Value '1' -Type String; 'ok'".to_string(),
        ("nla", false) => "Remove-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Name 'Do not use NLA' -ErrorAction SilentlyContinue; 'ok'".to_string(),
        _ => return Err(anyhow::anyhow!("unknown tweak {id}")),
    };
    run_ps(&script)
}

pub fn quick_action(id: &str) -> Result<String> {
    let script = match id {
        "flushdns" => "ipconfig /flushdns | Out-Null; Clear-DnsClientCache; 'DNS-кэш очищен'",
        "renew" => "ipconfig /release | Out-Null; ipconfig /renew | Out-Null; 'DHCP-адрес обновлён'",
        "winsock" => "netsh winsock reset | Out-Null; 'Winsock сброшен — нужна перезагрузка'",
        "ipreset" => "netsh int ip reset | Out-Null; 'Стек TCP/IP сброшен — нужна перезагрузка'",
        "arp" => "netsh interface ip delete arpcache | Out-Null; 'ARP-кэш очищен'",
        "wifi-reconnect" => "netsh wlan disconnect | Out-Null; Start-Sleep -Seconds 2; netsh wlan connect name=(netsh wlan show interfaces | Select-String -Pattern '^\\s*SSID' | Select-Object -First 1).ToString().Split(':')[1].Trim() | Out-Null; 'Wi-Fi переподключён'",
        _ => return Err(anyhow::anyhow!("unknown action {id}")),
    };
    run_ps(script)
}
