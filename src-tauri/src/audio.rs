//! Headphones / audio endpoints, Equalizer APO presets, Windows sound settings.
use crate::ps::{ps_quote, run_ps, run_ps_json, spawn_detached};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub const APO_DIR: &str = "C:\\Program Files\\EqualizerAPO\\config";
pub const APO_PRE: &str = "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}";
pub const APO_POST: &str = "{EC1CC9CE-FAED-4822-828A-82A81A6F018F}";
const FX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d}";

#[derive(Serialize, Clone, Debug, Default)]
pub struct AudioDevice {
    pub id: String,          // {0.0.0.00000000}.{guid}
    pub instance_id: String, // SWD\MMDEVAPI\...
    pub name: String,
    pub flow: String, // playback | capture
    pub state: String, // active | disabled | unplugged | notpresent | unknown
    pub is_default: bool,
    pub is_default_comm: bool,
    pub hands_free: bool,
    pub bluetooth: bool,
    pub product: String,
    /// Equalizer APO attached to this endpoint (FxProperties SFX = EqualizerAPO).
    pub apo: bool,
    /// DotPilot keeps a registry backup for this endpoint (can restore).
    pub apo_backup: bool,
    pub enhancements_disabled: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct AudioInfo {
    pub module_installed: bool,
    pub devices: Vec<AudioDevice>,
    pub eq: EqStatus,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct EqStatus {
    pub installed: bool,
    pub include_present: bool,
    pub preset: String,
    pub device_filter: String,
    pub config_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPnp {
    friendly_name: Option<String>,
    status: Option<serde_json::Value>,
    instance_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDef {
    default: Option<bool>,
    default_communication: Option<bool>,
    #[serde(rename = "ID")]
    id: Option<String>,
}

#[derive(Deserialize)]
struct RawReg {
    id: String,
    flow: String,
    state: i64,
    #[serde(default)]
    sfx: String,
    #[serde(default)]
    backup: bool,
    #[serde(default)]
    nofx: i64,
}

#[derive(Deserialize)]
struct RawAudio {
    module: bool,
    pnp: Vec<RawPnp>,
    defs: Vec<RawDef>,
    reg: Vec<RawReg>,
}

fn product_of(name: &str) -> String {
    // "Наушники (EDIFIER STAX SPIRIT S3)" → "EDIFIER STAX SPIRIT S3"; strips " Hands-Free"
    let inner = name
        .rfind('(')
        .and_then(|s| name.rfind(')').map(|e| name[s + 1..e].to_string()))
        .unwrap_or_else(|| name.to_string());
    inner.replace(" Hands-Free", "").replace(" Stereo", "").trim().to_string()
}

pub fn list() -> Result<AudioInfo> {
    let script = r#"
$mod = Get-Module -ListAvailable AudioDeviceCmdlets -ErrorAction SilentlyContinue
$pnp = @(Get-PnpDevice -Class AudioEndpoint -ErrorAction SilentlyContinue | Select-Object FriendlyName, Status, InstanceId)
$defs = @()
if ($mod) { try { Import-Module AudioDeviceCmdlets -ErrorAction Stop; $defs = @(Get-AudioDevice -List | Select-Object Default, DefaultCommunication, ID) } catch {} }
$reg = @()
foreach ($flow in 'Render','Capture') {
  foreach ($e in (Get-ChildItem "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\$flow" -ErrorAction SilentlyContinue)) {
    $p = Get-ItemProperty $e.PSPath -ErrorAction SilentlyContinue
    $fx = Get-ItemProperty "$($e.PSPath)\FxProperties" -ErrorAction SilentlyContinue
    $sfx = [string]$fx.'{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},5'
    $nofx = 0; if ($null -ne $fx.'{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5') { $nofx = [int64]$fx.'{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5' }
    $bk = Test-Path "HKLM:\SOFTWARE\DotPilot\AudioBackup\$($e.PSChildName)"
    $reg += @{ id = $e.PSChildName; flow = $flow; state = [int64]$p.DeviceState; sfx = $sfx; backup = $bk; nofx = $nofx }
  }
}
@{ module = [bool]$mod; pnp = $pnp; defs = $defs; reg = $reg } | ConvertTo-Json -Depth 4 -Compress
"#;
    let raw: RawAudio = run_ps_json(script)?;
    let mut devices = Vec::new();
    for p in raw.pnp {
        let inst = p.instance_id.clone().unwrap_or_default();
        let name = p.friendly_name.clone().unwrap_or_default();
        if inst.is_empty() || name.is_empty() {
            continue;
        }
        // SWD\MMDEVAPI\{0.0.0.00000000}.{GUID}
        let id = inst.rsplit('\\').next().unwrap_or("").to_string();
        let guid = id.rsplit('.').next().unwrap_or("").to_lowercase();
        let flow = if id.starts_with("{0.0.1.") { "capture" } else { "playback" };
        let reg = raw
            .reg
            .iter()
            .find(|r| format!("{{{}}}", r.id.trim_matches(|c| c == '{' || c == '}')).eq_ignore_ascii_case(&guid) && r.flow.eq_ignore_ascii_case(if flow == "capture" { "Capture" } else { "Render" }));
        let state = reg
            .map(|r| match r.state & 0xf {
                1 => "active",
                2 => "disabled",
                4 => "notpresent",
                8 => "unplugged",
                _ => "unknown",
            })
            .unwrap_or_else(|| match &p.status {
                Some(serde_json::Value::String(s)) if s == "OK" => "active",
                _ => "unknown",
            })
            .to_string();
        let def = raw.defs.iter().find(|d| d.id.as_deref().map(|x| x.eq_ignore_ascii_case(&id)).unwrap_or(false));
        let hands_free = name.contains("Hands-Free") || name.contains("Головной телефон");
        devices.push(AudioDevice {
            product: product_of(&name),
            id,
            instance_id: inst,
            name,
            flow: flow.into(),
            state,
            is_default: def.and_then(|d| d.default).unwrap_or(false),
            is_default_comm: def.and_then(|d| d.default_communication).unwrap_or(false),
            hands_free,
            bluetooth: false,
            apo: reg.map(|r| r.sfx.to_uppercase().contains("EACD2258")).unwrap_or(false),
            apo_backup: reg.map(|r| r.backup).unwrap_or(false),
            enhancements_disabled: reg.map(|r| r.nofx == 1).unwrap_or(false),
        });
    }
    // bluetooth heuristic: a product that also has a Hands-Free endpoint
    let hf_products: Vec<String> = devices.iter().filter(|d| d.hands_free).map(|d| d.product.clone()).collect();
    for d in devices.iter_mut() {
        d.bluetooth = hf_products.iter().any(|p| p.eq_ignore_ascii_case(&d.product));
    }
    devices.sort_by_key(|d| (d.flow != "playback", d.state != "active", d.hands_free, d.name.clone()));
    Ok(AudioInfo { module_installed: raw.module, devices, eq: eq_status() })
}

pub fn set_default(id: &str, role: &str) -> Result<String> {
    let flag = match role {
        "comm" => "-CommunicationOnly",
        "both" => "",
        _ => "-DefaultOnly",
    };
    run_ps(&format!(
        "Import-Module AudioDeviceCmdlets -ErrorAction Stop; Set-AudioDevice -ID {} {} | Out-Null; 'ok'",
        ps_quote(id),
        flag
    ))
}

pub fn set_enabled(instance_id: &str, enabled: bool) -> Result<String> {
    let cmd = if enabled { "Enable-PnpDevice" } else { "Disable-PnpDevice" };
    run_ps(&format!("{cmd} -InstanceId {} -Confirm:$false -ErrorAction Stop; 'ok'", ps_quote(instance_id)))
}

pub fn install_module() -> Result<String> {
    run_ps(
        "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; if (-not (Get-PackageProvider -Name NuGet -ListAvailable -ErrorAction SilentlyContinue)) { Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -Scope CurrentUser | Out-Null }; Set-PSRepository -Name PSGallery -InstallationPolicy Trusted -ErrorAction SilentlyContinue; Install-Module -Name AudioDeviceCmdlets -Scope CurrentUser -Force -AllowClobber -ErrorAction Stop; (Get-Module -ListAvailable AudioDeviceCmdlets | Select-Object -First 1).Version.ToString()",
    )
}

pub fn eq_status() -> EqStatus {
    let cfg = format!("{APO_DIR}\\config.txt");
    let installed = std::path::Path::new(&cfg).exists();
    let mut st = EqStatus { installed, config_path: cfg.clone(), ..Default::default() };
    if !installed {
        return st;
    }
    if let Ok(text) = std::fs::read_to_string(&cfg) {
        st.include_present = text.lines().any(|l| l.trim().eq_ignore_ascii_case("Include: dotpilot.txt"));
    }
    if let Ok(text) = std::fs::read_to_string(format!("{APO_DIR}\\dotpilot.txt")) {
        for l in text.lines() {
            if let Some(v) = l.strip_prefix("# preset: ") {
                st.preset = v.trim().to_string();
            }
            if let Some(v) = l.strip_prefix("# device: ") {
                st.device_filter = v.trim().to_string();
            }
        }
    }
    if st.preset.is_empty() {
        st.preset = "off".into();
    }
    st
}

pub fn eq_preset_lines(preset: &str) -> Option<(&'static str, &'static str)> {
    // (preamp, GraphicEQ)
    match preset {
        "footsteps" => Some((
            "-6",
            "20 -9; 40 -9; 60 -8; 100 -7; 150 -5; 250 -3; 400 -1; 600 0; 1000 1; 1500 2.5; 2000 4; 2500 5; 3000 5.5; 4000 5; 5000 4; 6000 3; 8000 1.5; 10000 0.5; 12000 0; 16000 -1; 20000 -2",
        )),
        "balanced" => Some((
            "-4",
            "20 -5; 60 -4; 120 -3; 250 -1.5; 500 0; 1000 0.5; 2000 2; 3000 3; 4000 3; 6000 2; 8000 1; 12000 0; 16000 0",
        )),
        "voice" => Some((
            "-4",
            "20 -8; 80 -6; 150 -3; 300 0; 600 1; 1000 2; 2000 3; 3000 3.5; 4000 2; 6000 0; 8000 -1; 12000 -2; 16000 -3",
        )),
        "music" => Some(("-3", "20 2; 60 2.5; 120 1.5; 250 0; 500 -0.5; 1000 0; 2000 0.5; 4000 1; 8000 1.5; 12000 1.5; 16000 1")),
        "off" => Some(("0", "")),
        _ => None,
    }
}

pub fn eq_apply(preset: &str, device_filter: &str) -> Result<String> {
    let (preamp, geq) = eq_preset_lines(preset).ok_or_else(|| anyhow!("Неизвестный пресет {preset}"))?;
    let cfg = format!("{APO_DIR}\\config.txt");
    if !std::path::Path::new(&cfg).exists() {
        return Err(anyhow!("Equalizer APO не установлен ({cfg})"));
    }
    let mut body = String::new();
    body.push_str("# Generated by DotPilot. Do not edit by hand: the app rewrites this file.\n");
    body.push_str(&format!("# preset: {preset}\n# device: {device_filter}\n"));
    if preset != "off" {
        if !device_filter.trim().is_empty() {
            // Equalizer APO: several devices in one line, separated by ";"
            body.push_str(&format!("Device: {}\n", device_filter.trim().trim_end_matches(';')));
        }
        body.push_str(&format!("Preamp: {preamp} dB\n"));
        body.push_str(&format!("GraphicEQ: {geq}\n"));
    }
    std::fs::write(format!("{APO_DIR}\\dotpilot.txt"), body).map_err(|e| anyhow!("Не удалось записать dotpilot.txt (нужны права администратора): {e}"))?;
    let text = std::fs::read_to_string(&cfg)?;
    if !text.lines().any(|l| l.trim().eq_ignore_ascii_case("Include: dotpilot.txt")) {
        let mut t = text.clone();
        if !t.ends_with('\n') {
            t.push('\n');
        }
        t.push_str("Include: dotpilot.txt\n");
        std::fs::write(&cfg, t)?;
    }
    Ok(format!("Пресет «{preset}» записан в {APO_DIR}\\dotpilot.txt"))
}

fn endpoint_guid(id: &str) -> Result<String> {
    // "{0.0.0.00000000}.{guid}" → "{guid}"
    let g = id.rsplit('.').next().unwrap_or("").to_string();
    if g.len() != 38 || !g.starts_with('{') {
        return Err(anyhow!("Некорректный идентификатор устройства: {id}"));
    }
    Ok(g)
}

/// Attach Equalizer APO to a playback endpoint the same way its Configurator does:
/// back up FxProperties, register the original APOs as children, point SFX/MFX at Equalizer APO.
pub fn apo_attach(id: &str) -> Result<String> {
    let g = endpoint_guid(id)?;
    if !std::path::Path::new(&format!("{APO_DIR}\\config.txt")).exists() {
        return Err(anyhow!("Equalizer APO не установлен"));
    }
    // Windows 11 keeps FxProperties owned by TrustedInstaller: even administrators cannot write until
    // they take ownership (exactly what Equalizer APO's Configurator does).
    let script = format!(
        r#"
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class DpPriv {{
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr h, uint a, out IntPtr t);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool LookupPrivilegeValue(string s, string n, out long l);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr t, bool d, ref TP n, int l, IntPtr p, IntPtr r);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [StructLayout(LayoutKind.Sequential, Pack=4)] public struct TP {{ public int c; public long l; public int a; }}
  public static bool Enable(string name) {{ IntPtr t; if (!OpenProcessToken(GetCurrentProcess(), 0x28, out t)) return false; TP tp; tp.c = 1; tp.a = 2; if (!LookupPrivilegeValue(null, name, out tp.l)) return false; return AdjustTokenPrivileges(t, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero); }}
}}
"@
[void][DpPriv]::Enable('SeTakeOwnershipPrivilege'); [void][DpPriv]::Enable('SeRestorePrivilege'); [void][DpPriv]::Enable('SeBackupPrivilege')
$admins = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-32-544')
function Make-Writable([string]$sub) {{
  $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [System.Security.AccessControl.RegistryRights]::TakeOwnership)
  if ($null -eq $key) {{ throw "key not found: $sub" }}
  $acl = New-Object System.Security.AccessControl.RegistrySecurity
  $acl.SetOwner($admins); $key.SetAccessControl($acl); $key.Close()
  $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [System.Security.AccessControl.RegistryRights]::ChangePermissions)
  $acl = $key.GetAccessControl()
  $rule = New-Object System.Security.AccessControl.RegistryAccessRule($admins, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
  $acl.AddAccessRule($rule); $key.SetAccessControl($acl); $key.Close()
}}
$g = '{g}'
$sub = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$g\FxProperties"
$k = "HKLM:\$sub"
if (-not (Test-Path $k)) {{ Make-Writable "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$g"; New-Item $k -Force | Out-Null }}
Make-Writable $sub
$fx = Get-ItemProperty $k
$b = "HKLM:\SOFTWARE\DotPilot\AudioBackup\$g"
if (-not (Test-Path $b)) {{
  New-Item $b -Force | Out-Null
  foreach ($p in $fx.PSObject.Properties) {{ if ($p.Name -like '{{*') {{ Set-ItemProperty $b -Name $p.Name -Value ([string]$p.Value) -Type String }} }}
  Set-ItemProperty $b -Name '__saved' -Value (Get-Date).ToString('s') -Type String
}}
$c = "HKLM:\SOFTWARE\EqualizerAPO\Child APOs\$g"
New-Item $c -Force | Out-Null
foreach ($i in 1,2,5,6,7) {{
  $n = "{FX},$i"; $v = [string]$fx.$n
  if ([string]::IsNullOrEmpty($v) -or $v -match 'EACD2258|EC1CC9CE') {{ $v = '!VALUE' }}
  Set-ItemProperty $c -Name $n -Value $v -Type String
}}
$pre = [string]$fx.'{FX},5'; if ($pre -match 'EACD2258') {{ $pre = '' }}
$post = [string]$fx.'{FX},6'; if ($post -match 'EC1CC9CE') {{ $post = '' }}
Set-ItemProperty $c -Name PreMixChild -Value $pre -Type String
Set-ItemProperty $c -Name PostMixChild -Value $post -Type String
Set-ItemProperty $c -Name AllowSilentBufferModification -Value 'false' -Type String
Set-ItemProperty $c -Name Version -Value '2' -Type String
Set-ItemProperty $k -Name '{FX},5' -Value '{APO_PRE}' -Type String
Set-ItemProperty $k -Name '{FX},6' -Value '{APO_POST}' -Type String
Set-ItemProperty $k -Name '{{1da5d803-d492-4edd-8c23-e0c0ffee7f0e}},5' -Value 0 -Type DWord
'ok'
"#
    );
    run_ps(&script)?;
    Ok(format!("Equalizer APO подключён к {g}"))
}

/// Restore the endpoint's FxProperties from DotPilot's backup and drop the child-APO entry.
pub fn apo_detach(id: &str) -> Result<String> {
    let g = endpoint_guid(id)?;
    let script = format!(
        r#"
$g = '{g}'
$k = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$g\FxProperties"
$b = "HKLM:\SOFTWARE\DotPilot\AudioBackup\$g"
if (Test-Path $b) {{
  $bk = Get-ItemProperty $b
  foreach ($i in 1,2,5,6,7) {{ Remove-ItemProperty $k -Name "{FX},$i" -ErrorAction SilentlyContinue }}
  foreach ($p in $bk.PSObject.Properties) {{ if ($p.Name -like '{{*') {{ Set-ItemProperty $k -Name $p.Name -Value ([string]$p.Value) -Type String }} }}
  Remove-Item $b -Recurse -Force -ErrorAction SilentlyContinue
}} else {{
  $c = Get-ItemProperty "HKLM:\SOFTWARE\EqualizerAPO\Child APOs\$g" -ErrorAction SilentlyContinue
  foreach ($i in 5,6) {{ $n = "{FX},$i"; $v = [string]$c.$n; if ($v -and $v -ne '!VALUE') {{ Set-ItemProperty $k -Name $n -Value $v -Type String }} else {{ Remove-ItemProperty $k -Name $n -ErrorAction SilentlyContinue }} }}
}}
Remove-Item "HKLM:\SOFTWARE\EqualizerAPO\Child APOs\$g" -Recurse -Force -ErrorAction SilentlyContinue
'ok'
"#
    );
    run_ps(&script)?;
    Ok(format!("Equalizer APO отключён от {g}, настройки устройства восстановлены"))
}

/// Restart Windows Audio so endpoint APO changes take effect (2–3 s of silence).
pub fn restart_audio() -> Result<String> {
    run_ps("Restart-Service -Name audiosrv -Force -ErrorAction Stop; Start-Sleep -Milliseconds 800; (Get-Service audiosrv).Status.ToString()")
}

/// One-click footsteps setup for the given headphone products.
pub fn fix_footsteps(products: &[String], preset: &str) -> Result<Vec<String>> {
    let info = list()?;
    let mut lines = Vec::new();
    let matches = |d: &AudioDevice| products.iter().any(|p| d.product.to_lowercase().contains(&p.to_lowercase()));
    for d in info.devices.iter().filter(|d| d.flow == "playback" && matches(d)) {
        if d.hands_free {
            if d.state != "disabled" && d.state != "notpresent" {
                match set_enabled(&d.instance_id, false) {
                    Ok(_) => lines.push(format!("Отключён телефонный профиль: {}", d.name)),
                    Err(e) => lines.push(format!("Не удалось отключить {}: {e}", d.name)),
                }
            }
        } else if !d.apo {
            match apo_attach(&d.id) {
                Ok(_) => lines.push(format!("Equalizer APO подключён: {}", d.name)),
                Err(e) => lines.push(format!("APO не подключился к {}: {e}", d.name)),
            }
        } else {
            lines.push(format!("Equalizer APO уже подключён: {}", d.name));
        }
    }
    let filter = products.join("; ");
    lines.push(eq_apply(preset, &filter)?);
    match restart_audio() {
        Ok(s) => lines.push(format!("Служба Windows Audio перезапущена ({})", s.trim())),
        Err(e) => lines.push(format!("Перезапуск Windows Audio не удался: {e}. Переподключи наушники или перезагрузи ПК.")),
    }
    Ok(lines)
}

/// Undo everything DotPilot changed in audio: EQ off, APO detached where we attached it, Hands-Free re-enabled.
pub fn reset_sound() -> Result<Vec<String>> {
    let info = list()?;
    let mut lines = Vec::new();
    lines.push(eq_apply("off", "").unwrap_or_else(|e| format!("EQ: {e}")));
    for d in info.devices.iter().filter(|d| d.flow == "playback") {
        if d.apo_backup {
            match apo_detach(&d.id) {
                Ok(s) => lines.push(s),
                Err(e) => lines.push(format!("{}: {e}", d.name)),
            }
        }
        if d.hands_free && d.state == "disabled" {
            match set_enabled(&d.instance_id, true) {
                Ok(_) => lines.push(format!("Включён обратно: {}", d.name)),
                Err(e) => lines.push(format!("{}: {e}", d.name)),
            }
        }
    }
    match restart_audio() {
        Ok(_) => lines.push("Windows Audio перезапущена".into()),
        Err(e) => lines.push(format!("Windows Audio: {e}")),
    }
    Ok(lines)
}

pub fn open_sound(target: &str) -> Result<u32> {
    match target {
        "settings" => spawn_detached("cmd.exe", &["/c", "start", "", "ms-settings:sound"], None),
        "bluetooth" => spawn_detached("cmd.exe", &["/c", "start", "", "ms-settings:bluetooth"], None),
        "volume-mixer" => spawn_detached("cmd.exe", &["/c", "start", "", "ms-settings:apps-volume"], None),
        "classic" => spawn_detached("control.exe", &["mmsys.cpl"], None),
        "apo" => spawn_detached("C:\\Program Files\\EqualizerAPO\\Configurator.exe", &[], None),
        "apo-download" => spawn_detached("cmd.exe", &["/c", "start", "", "https://sourceforge.net/projects/equalizerapo/"], None),
        _ => Err(anyhow!("unknown target")),
    }
}
