use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    /// Wi-Fi напрямую, минуя VPN.
    Direct,
    /// Через Happ VPN (список process_path в конфиге sing-box).
    Vpn,
    /// Radmin VPN в приоритете (LAN 26.0.0.0/8), интернет напрямую.
    Radmin,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    /// Полные пути к exe (все процессы приложения). Путь к папке = все exe внутри.
    pub exe_paths: Vec<String>,
    /// game | browser | dev | tool
    pub kind: String,
    pub policy: Policy,
    /// DSCP-метка (46 = EF, голосовой приоритет в Wi-Fi WMM). None = без приоритета.
    pub dscp: Option<u8>,
    /// Лимит скорости в Мбит/с. Применяется в игровом режиме, если background = true.
    pub throttle_mbps: Option<u32>,
    /// Фоновое приложение: в игровом режиме получает лимит скорости.
    pub background: bool,
    pub color: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PingTarget {
    pub id: String,
    pub name: String,
    pub host: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub power_plan: String,
    pub game_mode: bool,
    pub zapret_running: Option<bool>,
    pub happ_running: Option<bool>,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub apps: Vec<AppEntry>,
    pub ping_targets: Vec<PingTarget>,
    pub profiles: Vec<Profile>,
    pub active_profile: String,
    pub auto_game_mode: bool,
    pub sync_happ_config: bool,
    pub happ_config_path: String,
    pub happ_exe: String,
    pub happ_service: String,
    pub zapret_dir: String,
    pub zapret_service: String,
    pub radmin_exe: String,
    pub radmin_service: String,
    pub radmin_alias: String,
    pub anthropic_api_key: String,
    pub ai_model: String,
    pub ai_proxy: String,
    pub ai_effort: String,
    pub poll_ms: u64,
    /// Telegram Desktop exe.
    pub tg_exe: String,
    /// Local proxy helper for Telegram (TgWsProxy) and whether DotPilot starts it at launch.
    pub tgws_exe: String,
    pub tgws_autostart: bool,
    /// tg://socks?server=…&port=… or tg://proxy?…&secret=… — opened once to register the proxy in Telegram.
    pub tg_proxy_link: String,
    /// Products whose Hands-Free profile is disabled and which get the footsteps EQ.
    pub headphone_products: Vec<String>,
    /// Proxy guard: never leave HTTP_PROXY/HTTPS_PROXY or the system proxy pointing at a dead local port.
    pub proxy_guard: bool,
    /// Local proxy port of Happ (xray HTTP inbound).
    pub proxy_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Public".into());
        Config {
            apps: vec![
                AppEntry {
                    id: "warface".into(),
                    name: "Warface".into(),
                    exe_paths: vec![
                        "D:\\Games\\AstrumPlay\\Warface\\Bin64Release\\Game.exe".into(),
                        format!("{local}\\AstrumPlay\\AstrumPlay.exe"),
                    ],
                    kind: "game".into(),
                    policy: Policy::Direct,
                    dscp: Some(46),
                    throttle_mbps: None,
                    background: false,
                    color: "#f2b33d".into(),
                    note: "Игра идёт по Wi-Fi напрямую, без VPN. Трафик помечен как приоритетный (DSCP 46).".into(),
                },
                AppEntry {
                    id: "minecraft".into(),
                    name: "Minecraft SteamPunk".into(),
                    exe_paths: vec![
                        "D:\\DotNotFact\\Desktop\\Minecraft SteamPunk\\LL.exe".into(),
                        "D:\\DotNotFact\\Desktop\\Minecraft SteamPunk\\jre".into(),
                    ],
                    kind: "game".into(),
                    policy: Policy::Radmin,
                    dscp: Some(46),
                    throttle_mbps: None,
                    background: false,
                    color: "#5fd18c".into(),
                    note: "Интернет по Wi-Fi, сеть друзей через Radmin VPN (26.x.x.x) в приоритете.".into(),
                },
                AppEntry {
                    id: "yandex".into(),
                    name: "Яндекс Браузер".into(),
                    exe_paths: vec!["C:\\Program Files\\Yandex\\YandexBrowser\\Application\\browser.exe".into()],
                    kind: "browser".into(),
                    policy: Policy::Direct,
                    dscp: None,
                    throttle_mbps: None,
                    background: false,
                    color: "#3987e5".into(),
                    note: "Обычный Wi-Fi, zapret работает в фоне. Переключи на «Через Happ», когда нужны заблокированные вкладки.".into(),
                },
                AppEntry {
                    id: "claude-code".into(),
                    name: "Claude Code".into(),
                    exe_paths: claude_code_paths(),
                    kind: "dev".into(),
                    policy: Policy::Direct,
                    dscp: Some(8),
                    throttle_mbps: Some(40),
                    background: true,
                    color: "#d97757".into(),
                    note: "Напрямую: прокси-переменные не дают Claude работать без VPN, поэтому не используются. В игровом режиме получает лимит скорости.".into(),
                },
                AppEntry {
                    id: "docker".into(),
                    name: "Docker Desktop".into(),
                    exe_paths: vec![
                        "C:\\Program Files\\Docker\\Docker\\resources\\com.docker.backend.exe".into(),
                        "C:\\Program Files\\Docker\\Docker\\resources\\com.docker.build.exe".into(),
                    ],
                    kind: "dev".into(),
                    policy: Policy::Direct,
                    dscp: Some(8),
                    throttle_mbps: Some(30),
                    background: true,
                    color: "#2496ed".into(),
                    note: "Фоновая загрузка образов. В игровом режиме ограничивается по скорости.".into(),
                },
                telegram_entry(),
            ],
            ping_targets: vec![
                PingTarget { id: "gw".into(), name: "Роутер".into(), host: "192.168.0.1".into(), color: "#3987e5".into() },
                PingTarget { id: "yadns".into(), name: "Яндекс DNS".into(), host: "77.88.8.8".into(), color: "#d95926".into() },
                PingTarget { id: "gdns".into(), name: "Google DNS".into(), host: "8.8.8.8".into(), color: "#199e70".into() },
                PingTarget { id: "cf".into(), name: "Cloudflare".into(), host: "1.1.1.1".into(), color: "#c98500".into() },
            ],
            profiles: vec![
                Profile { id: "standard".into(), name: "Стандарт".into(), description: "Сбалансированная схема питания, сеть без лимитов.".into(), power_plan: "381b4222-f694-41f0-9685-ff5bb260df2e".into(), game_mode: false, zapret_running: None, happ_running: None, icon: "sliders".into() },
                Profile { id: "gaming".into(), name: "Игра".into(), description: "Высокая производительность и игровой режим сети: фоновые приложения ограничены, игры в приоритете.".into(), power_plan: "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".into(), game_mode: true, zapret_running: Some(true), happ_running: None, icon: "gamepad".into() },
                Profile { id: "work".into(), name: "Работа".into(), description: "Claude Code, Docker, Flutter: высокая производительность, сеть без лимитов, Happ включён.".into(), power_plan: "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".into(), game_mode: false, zapret_running: Some(true), happ_running: Some(true), icon: "briefcase".into() },
                Profile { id: "eco".into(), name: "Экономия".into(), description: "Тихий режим: экономия энергии, фоновые загрузки ограничены.".into(), power_plan: "a1841308-3541-4fab-bc81-f71556f20b4a".into(), game_mode: true, zapret_running: None, happ_running: None, icon: "leaf".into() },
            ],
            active_profile: "standard".into(),
            auto_game_mode: true,
            sync_happ_config: false,
            happ_config_path: format!("{local}\\Happ\\config.json"),
            happ_exe: "C:\\Program Files\\FlyFrogLLC\\Happ\\Happ.exe".into(),
            happ_service: "HappService".into(),
            zapret_dir: "D:\\DotNotFact\\Desktop\\zapret-discord-youtube-1.10.0".into(),
            zapret_service: "zapret".into(),
            radmin_exe: "C:\\Program Files (x86)\\Radmin VPN\\Radmin.exe".into(),
            radmin_service: "RvControlSvc".into(),
            radmin_alias: "Radmin VPN".into(),
            anthropic_api_key: String::new(),
            ai_model: "claude-opus-5".into(),
            ai_proxy: "http://127.0.0.1:10809".into(),
            ai_effort: "medium".into(),
            poll_ms: 3000,
            tg_exe: "D:\\Application\\Telegram Desktop\\Telegram.exe".into(),
            tgws_exe: "D:\\DotNotFact\\Desktop\\TgWsProxy_windows.exe".into(),
            tgws_autostart: false,
            tg_proxy_link: String::new(),
            headphone_products: vec!["EDIFIER".into(), "Redmi Buds".into()],
            proxy_guard: true,
            proxy_port: 10809,
        }
    }
}

/// Claude Code runs as %APPDATA%\Claude\claude-code\<version>\claude.exe (spawned by the Claude desktop app
/// from C:\Program Files\WindowsApps\Claude_<version>\app\Claude.exe) and as node.exe for MCP/tools.
pub fn claude_code_paths() -> Vec<String> {
    let roaming = std::env::var("APPDATA").unwrap_or_default();
    vec![
        format!("{roaming}\\Claude\\claude-code"),
        "C:\\Program Files\\WindowsApps\\Claude_".into(),
        "C:\\nvm4w\\nodejs\\node.exe".into(),
    ]
}

pub fn telegram_entry() -> AppEntry {
    AppEntry {
        id: "telegram".into(),
        name: "Telegram".into(),
        exe_paths: vec!["D:\\Application\\Telegram Desktop\\Telegram.exe".into()],
        kind: "tool".into(),
        policy: Policy::Direct,
        dscp: None,
        throttle_mbps: Some(20),
        background: true,
        color: "#2aa4e8".into(),
        note: "Идёт через локальный TgWsProxy (настраивается на странице «Сеть»), не через Happ. В игровом режиме ограничен по скорости.".into(),
    }
}

pub fn config_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join("DotPilot.config.json")
}

pub fn load() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let mut cfg = serde_json::from_str::<Config>(&s).unwrap_or_else(|e| {
                eprintln!("config parse error: {e}; using defaults");
                Config::default()
            });
            // migrations for configs created by older builds
            if !cfg.apps.iter().any(|a| a.id == "telegram") {
                cfg.apps.push(telegram_entry());
            }
            if let Some(cc) = cfg.apps.iter_mut().find(|a| a.id == "claude-code") {
                if !cc.exe_paths.iter().any(|p| p.contains("WindowsApps\\Claude_")) {
                    cc.exe_paths = claude_code_paths();
                }
            }
            if cfg.ping_targets.iter().any(|t| t.host == "26.0.0.1") {
                cfg.ping_targets.retain(|t| t.host != "26.0.0.1");
                cfg.ping_targets.push(PingTarget { id: "cf".into(), name: "Cloudflare".into(), host: "1.1.1.1".into(), color: "#c98500".into() });
            }
            cfg
        }
        Err(_) => {
            let cfg = Config::default();
            let _ = save(&cfg);
            cfg
        }
    }
}

pub fn save(cfg: &Config) -> anyhow::Result<()> {
    let s = serde_json::to_string_pretty(cfg)?;
    std::fs::write(config_path(), s)?;
    Ok(())
}
