//! Permissions / prerequisites the app needs, with scripts that grant them.
use crate::ps::{ps_quote, run_ps, run_ps_json};
use anyhow::{anyhow, Result};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct PermItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub effect: String,
    pub state: Option<bool>,
    pub can_grant: bool,
    pub needs_admin: bool,
    pub detail: String,
}

pub fn status(admin: bool) -> Result<Vec<PermItem>> {
    let script = r#"
$loc = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location' -ErrorAction SilentlyContinue
$locU = Get-ItemProperty 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location' -ErrorAction SilentlyContinue
$locN = Get-ItemProperty 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location\NonPackaged' -ErrorAction SilentlyContinue
$lfsvc = Get-Service lfsvc -ErrorAction SilentlyContinue
$qos = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\QoS' -ErrorAction SilentlyContinue
$task = Get-ScheduledTask -TaskName 'DotPilot' -ErrorAction SilentlyContinue
$mod = Get-Module -ListAvailable AudioDeviceCmdlets -ErrorAction SilentlyContinue
$apo = Test-Path 'C:\Program Files\EqualizerAPO\config\config.txt'
$ps7 = [bool](Get-Command pwsh -ErrorAction SilentlyContinue)
$defender = $null
try { $defender = (Get-MpPreference -ErrorAction Stop).ExclusionPath } catch {}
@{
  loc_machine = [string]$loc.Value
  loc_user = [string]$locU.Value
  loc_nonpackaged = [string]$locN.Value
  lfsvc = [string]$lfsvc.Status
  lfsvc_start = [string]$lfsvc.StartType
  nla = ($qos.'Do not use NLA' -eq '1')
  task = [bool]$task
  task_state = [string]$task.State
  audiomod = [bool]$mod
  apo = $apo
} | ConvertTo-Json -Compress
"#;
    let v: serde_json::Value = run_ps_json(script)?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let b = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let loc_ok = s("loc_machine") == "Allow" && s("loc_user") != "Deny" && s("loc_nonpackaged") != "Deny" && s("lfsvc") == "Running";
    Ok(vec![
        PermItem {
            id: "admin".into(),
            name: "Права администратора".into(),
            description: "Нужны для QoS-правил, метрик адаптеров, служб, твиков реестра и записи пресетов эквалайзера.".into(),
            effect: "DotPilot перезапустится с запросом UAC. Без прав можно только смотреть данные.".into(),
            state: Some(admin),
            can_grant: !admin,
            needs_admin: false,
            detail: if admin { "Запущено с повышенными правами".into() } else { "Сейчас без прав администратора".into() },
        },
        PermItem {
            id: "location".into(),
            name: "Расположение для Wi-Fi".into(),
            description: "Windows 11 отдаёт SSID, уровень сигнала, канал и диапазон Wi-Fi только приложениям с доступом к расположению.".into(),
            effect: "Включается системный переключатель «Службы определения местоположения» и доступ для классических приложений. Геопозиция никуда не отправляется: DotPilot читает только параметры Wi-Fi.".into(),
            state: Some(loc_ok),
            can_grant: true,
            needs_admin: true,
            detail: format!("система: {} · пользователь: {} · классические приложения: {} · служба lfsvc: {}", s("loc_machine"), s("loc_user"), s("loc_nonpackaged"), s("lfsvc")),
        },
        PermItem {
            id: "nla".into(),
            name: "DSCP-метки вне домена".into(),
            description: "Без ключа «Do not use NLA» Windows на домашнем ПК игнорирует DSCP из QoS-политик, и приоритет игрового трафика не работает.".into(),
            effect: "Ставится строковый параметр реестра в Tcpip\\QoS. Применяется сразу для новых соединений, откатывается одной кнопкой.".into(),
            state: Some(b("nla")),
            can_grant: true,
            needs_admin: true,
            detail: if b("nla") { "Ключ установлен".into() } else { "Ключ отсутствует".into() },
        },
        PermItem {
            id: "autostart".into(),
            name: "Автозапуск при входе без UAC".into(),
            description: "Задача планировщика с наивысшими правами запускает DotPilot при входе в систему, и окно UAC больше не появляется.".into(),
            effect: "Создаётся задача «DotPilot» в Планировщике заданий. Отключается той же кнопкой. Запустить без UAC вручную: schtasks /run /tn DotPilot.".into(),
            state: Some(b("task")),
            can_grant: true,
            needs_admin: true,
            detail: if b("task") { format!("Задача существует · {}", s("task_state")) } else { "Задачи нет".into() },
        },
        PermItem {
            id: "audiomod".into(),
            name: "Модуль управления звуком (AudioDeviceCmdlets)".into(),
            description: "PowerShell-модуль из PSGallery, который умеет переключать устройство вывода по умолчанию. Без него DotPilot показывает устройства, но не переключает их.".into(),
            effect: "Скачивается ~100 КБ из PowerShell Gallery в профиль пользователя. Интернет нужен (через Happ, если Gallery недоступна напрямую).".into(),
            state: Some(b("audiomod")),
            can_grant: true,
            needs_admin: false,
            detail: if b("audiomod") { "Установлен".into() } else { "Не установлен".into() },
        },
        PermItem {
            id: "apo".into(),
            name: "Equalizer APO".into(),
            description: "Системный эквалайзер, через который DotPilot включает пресеты «Шаги», «Голос», «Музыка» для наушников.".into(),
            effect: "Устанавливается вручную с sourceforge (кнопка открывает страницу загрузки). После установки в Configurator нужно отметить наушники.".into(),
            state: Some(b("apo")),
            can_grant: !b("apo"),
            needs_admin: false,
            detail: if b("apo") { "Установлен (C:\\Program Files\\EqualizerAPO)".into() } else { "Не найден".into() },
        },
    ])
}

pub fn grant(id: &str, on: bool, exe: &str) -> Result<String> {
    match (id, on) {
        ("location", true) => run_ps(
            "foreach ($k in 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location','HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location\\NonPackaged','HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location','HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location\\NonPackaged') { New-Item -Path $k -Force | Out-Null; Set-ItemProperty -Path $k -Name Value -Value 'Allow' -Type String }; \
             New-Item 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\lfsvc\\Service\\Configuration' -Force | Out-Null; Set-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\lfsvc\\Service\\Configuration' -Name Status -Value 1 -Type DWord; \
             $so = 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Sensor\\Overrides\\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}'; New-Item $so -Force | Out-Null; Set-ItemProperty $so -Name SensorPermissionState -Value 1 -Type DWord; \
             $svc = Get-Service lfsvc -ErrorAction SilentlyContinue; if ($svc) { if ($svc.StartType -eq 'Disabled') { Set-Service lfsvc -StartupType Manual }; Restart-Service lfsvc -Force -ErrorAction SilentlyContinue }; Restart-Service camsvc -Force -ErrorAction SilentlyContinue; Start-Sleep 2; \
             $o = netsh wlan show interfaces; if ($o -match 'Access is denied|Отказано') { 'Ключи расположения выставлены, но Windows применит их только после перезагрузки (или включи вручную: Параметры → Конфиденциальность → Расположение)' } else { 'Доступ к расположению выдан, данные Wi-Fi доступны' }",
        ),
        ("location", false) => run_ps(
            "Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location' -Name Value -Value 'Deny' -Type String; 'Доступ к расположению отключён'",
        ),
        ("nla", true) => run_ps("New-Item -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Force | Out-Null; Set-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Name 'Do not use NLA' -Value '1' -Type String; 'DSCP разрешён'"),
        ("nla", false) => run_ps("Remove-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\QoS' -Name 'Do not use NLA' -ErrorAction SilentlyContinue; 'DSCP-ключ удалён'"),
        ("autostart", true) => {
            let dir = std::path::Path::new(exe).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            run_ps(&format!(
                "$a = New-ScheduledTaskAction -Execute {} -WorkingDirectory {}; $t = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME; $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero); Register-ScheduledTask -TaskName 'DotPilot' -Action $a -Trigger $t -Settings $s -RunLevel Highest -Force | Out-Null; 'Автозапуск включён'",
                ps_quote(exe),
                ps_quote(&dir)
            ))
        }
        ("autostart", false) => run_ps("Unregister-ScheduledTask -TaskName 'DotPilot' -Confirm:$false -ErrorAction SilentlyContinue; 'Автозапуск выключен'"),
        ("audiomod", true) => crate::audio::install_module().map(|v| format!("AudioDeviceCmdlets {v} установлен")),
        ("apo", true) => crate::audio::open_sound("apo-download").map(|_| "Открыта страница загрузки Equalizer APO".into()),
        _ => Err(anyhow!("Нет сценария для {id}={on}")),
    }
}
