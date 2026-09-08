mod ai;
mod audio;
mod bench;
mod config;
mod fanctl;
mod health;
mod perms;
mod net;
mod nvapi;
mod ocloop;
mod ocsafe;
mod ping;
mod platform;
mod policy;
mod ps;
mod report;
mod services;
mod sysmon;
mod tray;
mod trends;
mod voltage;

use config::Config;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, State};

#[derive(Serialize, Clone, Debug)]
pub struct LogEntry {
    pub ts: i64,
    pub level: String,
    pub msg: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct AppStatus {
    pub id: String,
    pub running: bool,
    pub procs: Vec<sysmon::ProcInfo>,
    pub in_happ_list: bool,
    pub qos_rules: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct GameModeState {
    pub active: bool,
    pub auto: bool,
    pub manual: Option<bool>,
    pub trigger: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Snapshot {
    pub ts: i64,
    pub admin: bool,
    pub adapters: Vec<net::Adapter>,
    pub routes: Vec<net::Route>,
    pub wifi: Vec<(String, String)>,
    pub services: Vec<services::ServiceStatus>,
    pub happ: services::HappConfigInfo,
    pub system_proxy_enabled: bool,
    pub system_proxy: String,
    pub apps: Vec<AppStatus>,
    pub cpu: sysmon::CpuInfo,
    pub mem: sysmon::MemInfo,
    pub gpu: sysmon::GpuInfo,
    pub net_rates: Vec<sysmon::NetRate>,
    pub game_mode: GameModeState,
    pub qos: Vec<policy::QosPolicy>,
    pub power_plans: Vec<sysmon::PowerPlan>,
    pub ping: Vec<ping::TargetSeries>,
    pub events: Vec<LogEntry>,
    pub default_via: String,
    pub self_stats: sysmon::SelfStats,
    /// Main window hidden (tray mode): collector runs in low-power cadence.
    pub hidden: bool,
    pub proxy: ProxyState,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ProxyState {
    pub port: u16,
    pub alive: bool,
    pub env: services::ProxyEnv,
    /// env vars point at the local proxy
    pub env_points_local: bool,
    pub exceptions_ok: bool,
    pub guard_enabled: bool,
    /// Guard removed the env vars because the proxy was dead; they will be restored when it is back.
    pub guard_holding: bool,
    pub problems: Vec<String>,
}

/// Periodic (30 s) context record used by the AI report to explain ping incidents.
#[derive(Serialize, Clone, Debug, Default)]
pub struct ContextEntry {
    pub ts: i64,
    pub cpu: f32,
    pub mem_pct: f32,
    pub rx_mbps: f32,
    pub tx_mbps: f32,
    pub wifi_link: String,
    pub game_mode: bool,
    pub running: Vec<String>,
    pub top: Vec<(String, f32)>,
    pub happ: String,
    pub zapret: bool,
    /// (target name, avg rtt last 30 s, loss % last 30 s)
    pub ping: Vec<(String, Option<f32>, f32)>,
}

pub struct AppState {
    context: Mutex<std::collections::VecDeque<ContextEntry>>,
    bench_hidden: std::sync::atomic::AtomicBool,
    cfg: Mutex<Config>,
    snap: Mutex<Snapshot>,
    ping: ping::PingMonitor,
    sys: Mutex<sysmon::SysCollector>,
    log: Mutex<Vec<LogEntry>>,
    game: Mutex<GameModeState>,
    admin: bool,
    svc_map: Mutex<(Option<Instant>, HashMap<u32, Vec<String>>)>,
    tgws_pid: Mutex<Option<u32>>,
    /// Proxy guard: env values we removed while the proxy was dead (to restore later).
    guard_saved: Mutex<Option<(String, String)>>,
}

type Shared = Arc<AppState>;

impl AppState {
    fn log(&self, level: &str, msg: impl Into<String>) {
        let mut l = self.log.lock().unwrap();
        l.push(LogEntry { ts: chrono::Utc::now().timestamp_millis(), level: level.into(), msg: msg.into() });
        if l.len() > 300 {
            let excess = l.len() - 300;
            l.drain(0..excess);
        }
    }
    fn cfg(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }
}

fn is_admin() -> bool {
    ps::run_ps("([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)")
        .map(|s| s.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Полная телеметрия видеокарты через NVAPI: то, чего не отдаёт nvidia-smi
/// (смещения частот, границы лимита мощности, режим вентиляторов).
#[tauri::command]
fn gpu_nvapi(state: State<'_, Shared>) -> Result<nvapi::GpuTelemetry, String> {
    nvapi::telemetry().ok_or_else(|| {
        state.log("error", "NVAPI недоступна: карта NVIDIA не найдена или драйвер не отвечает");
        "NVAPI недоступна".to_string()
    })
}

/// Честный отчёт о том, какие рычаги управления существуют на этой машине.
#[tauri::command]
fn gpu_capabilities() -> nvapi::GpuCapabilities {
    nvapi::capabilities()
}

/// Журнал разгона: что применено, что проверено, что уже приводило к сбою.
#[tauri::command]
fn oc_state() -> ocsafe::Journal {
    ocsafe::load()
}

/// Применить настройку. Значения зажимаются границами до записи в железо.
#[tauri::command]
fn oc_apply(
    state: State<'_, Shared>,
    candidate: ocsafe::GpuCandidate,
    stage: ocsafe::Stage,
) -> Result<ocsafe::ApplyReport, String> {
    let r = ocsafe::apply(candidate, stage);
    match &r {
        Ok(rep) => state.log("info", format!("Разгон применён: {}", rep.message)),
        Err(e) => state.log("error", format!("Разгон не применён: {e}")),
    }
    r
}

/// Ступень пройдена: следующая ступень либо перевод в проверенные.
#[tauri::command]
fn oc_confirm(state: State<'_, Shared>) -> Result<ocsafe::Journal, String> {
    let r = ocsafe::confirm();
    if r.is_ok() {
        state.log("info", "Настройка разгона прошла ступень проверки");
    }
    r
}

/// Настройка не прошла проверку: откат и запись в чёрный список.
#[tauri::command]
fn oc_reject(state: State<'_, Shared>, reason: String) -> Result<ocsafe::Journal, String> {
    let r = ocsafe::reject(&reason);
    state.log("warn", format!("Откат разгона: {reason}"));
    r
}

/// Вердикт по ступени: сводит ошибки вычислений, сбои драйвера и температуру,
/// после чего сам подтверждает настройку либо откатывает её.
#[tauri::command]
async fn oc_validate(
    state: State<'_, Shared>,
    evidence: ocsafe::StageEvidence,
) -> Result<ocsafe::StageVerdict, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let v = ocsafe::validate(evidence);
        match &v {
            Ok(r) if r.passed => st.log("info", format!("Проверка разгона: {}", r.reason)),
            Ok(r) => st.log("warn", format!("Проверка разгона не пройдена: {}", r.reason)),
            Err(e) => st.log("error", format!("Проверка разгона сорвалась: {e}")),
        }
        v
    })
    .await
    .map_err(err)?
}

/// Спросить у Claude следующий шаг подбора. Предложение возвращается уже
/// обрезанным по коридору — применять его или нет, решает вызывающий.
#[tauri::command]
async fn oc_propose(state: State<'_, Shared>, note: String) -> Result<ocloop::Suggestion, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        match ocloop::propose(&cfg, &note) {
            Ok(s) => {
                st.log(
                    "info",
                    format!(
                        "Claude предложил: ядро {:+} МГц, память {:+} МГц, мощность {:.0} %{}",
                        s.candidate.core_offset_mhz,
                        s.candidate.mem_offset_mhz,
                        s.candidate.power_percent,
                        if s.clamped { " (урезано до коридора)" } else { "" }
                    ),
                );
                Ok(s)
            }
            Err(e) => {
                st.log("error", format!("Claude не смог предложить шаг: {e}"));
                Err(err(e))
            }
        }
    })
    .await
    .map_err(err)?
}

/// Долгие наблюдения: деградация охлаждения и расход электричества.
#[tauri::command]
async fn trends_report(state: State<'_, Shared>) -> Result<trends::TrendReport, String> {
    let tariff = state.cfg().power_tariff;
    tauri::async_runtime::spawn_blocking(move || trends::report(tariff))
        .await
        .map_err(err)
}

/// Регуляторы напряжения, которые отдаёт ACPI-интерфейс платы.
#[tauri::command]
async fn voltage_state() -> Result<voltage::VoltageState, String> {
    tauri::async_runtime::spawn_blocking(voltage::read_state).await.map_err(err)
}

/// Смещение напряжения. Возвращает фактически применённое значение после обрезки.
#[tauri::command]
async fn voltage_set_offset(state: State<'_, Shared>, id: i32, millivolts: i32) -> Result<i32, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = voltage::set_offset(id, millivolts);
        match &r {
            Ok(v) => st.log("warn", format!("Экспериментально: смещение напряжения {id} = {v} мВ")),
            Err(e) => st.log("error", format!("Смещение напряжения не применено: {e}")),
        }
        r
    })
    .await
    .map_err(err)?
}

/// Вернуть все смещения напряжения к нулю.
#[tauri::command]
async fn voltage_reset(state: State<'_, Shared>) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = voltage::reset_all();
        match &r {
            Ok(()) => st.log("info", "Смещения напряжения сняты"),
            Err(e) => st.log("error", format!("Не удалось снять смещения напряжения: {e}")),
        }
        r
    })
    .await
    .map_err(err)?
}

/// Оценка состояния ПК: что измерено, что считается нормой и где расхождение.
#[tauri::command]
async fn health_check(state: State<'_, Shared>) -> Result<health::HealthReport, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Температуру процессора нельзя судить без контекста нагрузки.
        let load = st.snap.lock().unwrap().cpu.usage;
        let r = health::check(load);
        st.log(
            if r.problems > 0 { "warn" } else { "info" },
            format!("Проверка состояния: {}", r.summary),
        );
        r
    })
    .await
    .map_err(err)
}

/// Второе мнение Claude по отчёту о состоянии.
#[tauri::command]
async fn health_advice(state: State<'_, Shared>, report: serde_json::Value) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        ocloop::advise_health(&cfg, &report).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Состояние вентиляторов и датчиков платы через ACPI-интерфейс.
#[tauri::command]
async fn fans_state() -> Result<fanctl::FanControllerState, String> {
    tauri::async_runtime::spawn_blocking(fanctl::read_state).await.map_err(err)
}

/// Пороги остановки и запуска вентилятора. Возвращает применённые значения
/// после обрезки пределами — они могут отличаться от запрошенных.
#[tauri::command]
async fn fans_set_limits(state: State<'_, Shared>, id: u8, off_c: u8, on_c: u8) -> Result<(u8, u8), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = fanctl::set_temp_limits(id, off_c, on_c);
        match &r {
            Ok((o, n)) => st.log("info", format!("Вентилятор {id}: пороги {o}/{n} °C")),
            Err(e) => st.log("error", format!("Вентилятор {id}: {e}")),
        }
        r
    })
    .await
    .map_err(err)?
}

/// Разрешить или запретить полную остановку вентилятора.
#[tauri::command]
async fn fans_set_zero(state: State<'_, Shared>, id: u8, enabled: bool) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = fanctl::set_zero_fan(id, enabled);
        match &r {
            Ok(()) => st.log("info", format!("Вентилятор {id}: остановка {}", if enabled { "разрешена" } else { "запрещена" })),
            Err(e) => st.log("warn", format!("Вентилятор {id}: {e}")),
        }
        r
    })
    .await
    .map_err(err)?
}

/// Привязать вентилятор к другому датчику температуры.
#[tauri::command]
async fn fans_set_sensor(state: State<'_, Shared>, id: u8, sensor: u8) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = fanctl::set_target_sensor(id, sensor);
        if r.is_ok() {
            st.log("info", format!("Вентилятор {id} привязан к датчику {sensor}"));
        }
        r
    })
    .await
    .map_err(err)?
}

/// Принудительно раскрутить вентилятор, отменив остановку.
#[tauri::command]
async fn fans_force_on(state: State<'_, Shared>, id: u8) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = fanctl::force_on(id);
        if r.is_ok() {
            st.log("info", format!("Вентилятор {id} раскручен принудительно"));
        }
        r
    })
    .await
    .map_err(err)?
}

/// Вернуть все вентиляторы под кривую Smart Fan из BIOS.
#[tauri::command]
async fn fans_restore(state: State<'_, Shared>) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = fanctl::restore_bios_control();
        match &r {
            Ok(()) => st.log("info", "Вентиляторы возвращены под управление BIOS"),
            Err(e) => st.log("error", format!("Не удалось вернуть вентиляторы под BIOS: {e}")),
        }
        r
    })
    .await
    .map_err(err)?
}

/// План правок BIOS от Claude на основе снятых замеров.
#[tauri::command]
async fn bios_advice(state: State<'_, Shared>, question: String) -> Result<ocloop::BiosSuggestion, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        match ocloop::advise_bios(&cfg, &question) {
            Ok(s) => {
                st.log("info", format!("Claude предложил {} правок в BIOS", s.advice.settings.len()));
                Ok(s)
            }
            Err(e) => {
                st.log("error", format!("Совет по BIOS не получен: {e}"));
                Err(err(e))
            }
        }
    })
    .await
    .map_err(err)?
}

/// Состояние того, что настраивается в BIOS: память, прошивка, профиль.
#[tauri::command]
async fn platform_state() -> Result<(platform::FirmwareInfo, platform::MemoryConfig), String> {
    tauri::async_runtime::spawn_blocking(|| (platform::firmware_info(), platform::memory_config()))
        .await
        .map_err(err)
}

/// Снять замер: прогнать тесты и записать результат для сравнения до и после BIOS.
#[tauri::command]
async fn platform_measure(
    state: State<'_, Shared>,
    label: String,
    seconds: u64,
    memory_mb: usize,
) -> Result<platform::BaselineStore, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let b = platform::measure(&label, seconds, memory_mb);
        st.log(
            "info",
            format!(
                "Замер «{}»: процессор {:.0} проходов/с, память {:.0} МБ/с, частота под нагрузкой {}",
                b.label,
                b.cpu_passes_per_sec,
                b.memory_mb_per_sec,
                b.loaded_clock_mhz.map(|c| format!("{c:.0} МГц")).unwrap_or("неизвестна".into())
            ),
        );
        platform::save_baseline(b)
    })
    .await
    .map_err(err)?
}

/// Сохранённые замеры и сравнение двух последних.
#[tauri::command]
fn platform_baselines() -> (platform::BaselineStore, Option<platform::Comparison>) {
    let store = platform::load_baselines();
    let cmp = if store.items.len() >= 2 {
        let n = store.items.len();
        Some(platform::compare(&store.items[n - 2], &store.items[n - 1]))
    } else {
        None
    };
    (store, cmp)
}

/// Нагрузка на процессор со сверкой контрольных сумм.
#[tauri::command]
async fn bench_cpu(seconds: u64, threads: usize) -> Result<bench::StressResult, String> {
    tauri::async_runtime::spawn_blocking(move || bench::cpu_stress(seconds, threads))
        .await
        .map_err(err)
}

/// Проверка памяти записью и чтением цепочки.
#[tauri::command]
async fn bench_memory(megabytes: usize, seconds: u64) -> Result<bench::StressResult, String> {
    tauri::async_runtime::spawn_blocking(move || bench::memory_test(megabytes, seconds))
        .await
        .map_err(err)
}

/// Наибольшая температура видеокарты за короткое наблюдение.
#[tauri::command]
async fn gpu_peak_temp(samples: u32, interval_ms: u64) -> Result<Option<i32>, String> {
    tauri::async_runtime::spawn_blocking(move || bench::gpu_peak_temp(samples, interval_ms))
        .await
        .map_err(err)
}

/// Снять разгон и вернуть карту к штатным значениям.
#[tauri::command]
fn oc_reset(state: State<'_, Shared>) -> Result<ocsafe::Journal, String> {
    let r = ocsafe::reset();
    match &r {
        Ok(_) => state.log("info", "Разгон снят, карта в штатном режиме"),
        Err(e) => state.log("error", format!("Не удалось снять разгон: {e}")),
    }
    r
}

// ---------- collector thread ----------

fn collector(app: tauri::AppHandle, st: Shared) {
    let mut last_slow = Instant::now() - Duration::from_secs(60);
    let mut last_mid = Instant::now() - Duration::from_secs(60);
    let mut cached_adapters: Vec<net::Adapter> = vec![];
    let mut cached_routes: Vec<net::Route> = vec![];
    let mut cached_qos: Vec<policy::QosPolicy> = vec![];
    let mut cached_plans: Vec<sysmon::PowerPlan> = vec![];
    let mut cached_services: Vec<(String, String)> = vec![];
    let mut cached_wifi: Vec<(String, String)> = vec![];
    let mut cached_gpu = sysmon::GpuInfo::default();
    let mut cached_proxy = services::ProxyEnv::default();
    let mut cached_alive = false;
    let mut exceptions_fixed = false;
    let mut cached_happ = services::HappConfigInfo::default();
    let mut prev_game_active: Option<bool> = None;
    let mut last_tray = Instant::now() - Duration::from_secs(60);
    let mut last_ctx = Instant::now() - Duration::from_secs(60);

    loop {
        let cfg = st.cfg();
        // Tray mode: the window is hidden, so only what the tray shows is refreshed, and rarely.
        let hidden = app.get_webview_window("main").map(|w| !w.is_visible().unwrap_or(true)).unwrap_or(false) || st.bench_hidden.load(std::sync::atomic::Ordering::Relaxed);
        let poll = if hidden { Duration::from_millis(cfg.poll_ms.max(1000)).max(Duration::from_secs(8)) } else { Duration::from_millis(cfg.poll_ms.max(1000)) };
        let mid_every = if hidden { Duration::from_secs(20) } else { Duration::from_secs(5) };
        let slow_every = if hidden { Duration::from_secs(60) } else { Duration::from_secs(12) };

        // --- fast ---
        let (cpu, mem, rates, found, gui) = {
            let mut sys = st.sys.lock().unwrap();
            let cpu = sys.cpu();
            let mem = sys.mem();
            let rates = sys.net_rates();
            let found = sys.find_apps(&cfg.apps);
            let gui = sys.find_by_name(&["Happ.exe", "happd.exe", "xray.exe", "sing-box.exe", "winws.exe", "Radmin.exe", "RvRvpnGui.exe"]);
            (cpu, mem, rates, found, gui)
        };

        // --- mid ---
        if last_mid.elapsed() > mid_every {
            last_mid = Instant::now();
            if let Ok((svc, proxy)) = services::mid_status(&[&cfg.happ_service, &cfg.zapret_service, &cfg.radmin_service]) {
                cached_services = svc;
                cached_proxy = proxy;
            }
            cached_alive = services::port_alive(cfg.proxy_port);
            // ---- proxy guard ----
            if cfg.proxy_guard {
                let local = format!("127.0.0.1:{}", cfg.proxy_port);
                let env_local = cached_proxy.env_http.contains(&local) || cached_proxy.env_https.contains(&local);
                let mut saved = st.guard_saved.lock().unwrap();
                if !cached_alive && env_local {
                    // proxy is dead but every new process would still be told to use it → remove for now
                    *saved = Some((cached_proxy.env_http.clone(), cached_proxy.env_https.clone()));
                    match services::proxy_fix("clear-env", cfg.proxy_port) {
                        Ok(_) => st.log("info", format!("Прокси-страж: 127.0.0.1:{} не отвечает — переменные HTTP_PROXY/HTTPS_PROXY временно убраны, вернутся когда Happ поднимется", cfg.proxy_port)),
                        Err(e) => st.log("error", format!("Прокси-страж: не удалось убрать переменные: {e}")),
                    }
                    cached_proxy.env_http.clear();
                    cached_proxy.env_https.clear();
                }
                if !cached_alive && cached_proxy.system_enabled && cached_proxy.system_server.contains(&local) {
                    match services::proxy_fix("disable-system", cfg.proxy_port) {
                        Ok(_) => st.log("info", "Прокси-страж: системный прокси указывал на мёртвый порт — выключен (Happ включит его при подключении)"),
                        Err(e) => st.log("error", format!("Прокси-страж: системный прокси: {e}")),
                    }
                    cached_proxy.system_enabled = false;
                }
                if cached_alive && saved.is_some() && cached_proxy.env_http.is_empty() {
                    match services::proxy_fix("set-env", cfg.proxy_port) {
                        Ok(_) => {
                            st.log("info", "Прокси-страж: Happ снова отвечает — переменные HTTP_PROXY/HTTPS_PROXY восстановлены (с исключениями для Radmin и локальных сетей)");
                            *saved = None;
                        }
                        Err(e) => st.log("error", format!("Прокси-страж: восстановление: {e}")),
                    }
                }
                let exc_ok = cached_proxy.system_override.split(';').any(|t| t.trim() == "26.*") && (cached_proxy.env_no_proxy.contains("26.") || cached_proxy.env_http.is_empty());
                if !exc_ok && !exceptions_fixed {
                    exceptions_fixed = true;
                    match services::proxy_fix("no-proxy", cfg.proxy_port) {
                        Ok(_) => st.log("info", "Прокси-страж: сеть Radmin (26.*, 25.*) и локальные сети добавлены в исключения прокси"),
                        Err(e) => st.log("error", format!("Прокси-страж: исключения: {e}")),
                    }
                }
            }
            cached_gpu = sysmon::gpu();
            // Образец для долгих наблюдений. Модуль сам ограничивает частоту записи,
            // поэтому вызывать его на каждом медленном цикле безопасно.
            trends::maybe_record(&cached_gpu, cpu.usage);
            if !hidden {
                cached_wifi = net::wifi_info().unwrap_or_default();
            }
            cached_happ = services::happ_config(&cfg.happ_config_path);
        }
        // --- slow ---
        if last_slow.elapsed() > slow_every {
            last_slow = Instant::now();
            if let Ok(a) = net::adapters() {
                cached_adapters = a;
            }
            if let Ok(r) = net::routes() {
                cached_routes = r;
            }
            cached_qos = policy::list_qos().unwrap_or_default();
            cached_plans = sysmon::power_plans();
        }

        // merge throughput into adapters
        let mut adapters = cached_adapters.clone();
        for a in adapters.iter_mut() {
            if let Some(r) = rates.iter().find(|r| r.name == a.name) {
                a.rx_bps = r.rx_bps;
                a.tx_bps = r.tx_bps;
            }
        }

        // services
        let svc_state = |name: &str| -> String {
            cached_services
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, s)| s.clone())
                .unwrap_or_else(|| "Missing".into())
        };
        let pids_of = |names: &[&str]| -> Vec<u32> {
            gui.iter()
                .filter(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n)))
                .map(|p| p.pid)
                .collect()
        };
        let happ_tun_up = adapters.iter().any(|a| a.role == "happ" && a.status == "Up" && a.name.to_lowercase().contains("happ"));
        let happ_gui = pids_of(&["Happ.exe"]);
        let happ_core = pids_of(&["xray.exe", "sing-box.exe"]);
        let happ_mode = if happ_tun_up {
            "tun"
        } else if cached_proxy.system_enabled && cached_proxy.system_server.contains("127.0.0.1") && !happ_core.is_empty() {
            "proxy"
        } else if !happ_core.is_empty() {
            "idle"
        } else {
            "off"
        };
        let services_v = vec![
            services::ServiceStatus {
                id: "happ".into(),
                name: "Happ VPN".into(),
                service: cfg.happ_service.clone(),
                service_state: svc_state(&cfg.happ_service),
                gui_running: !happ_gui.is_empty(),
                gui_pids: happ_gui,
                detail: match happ_mode {
                    "tun" => "TUN-режим: выбранные приложения идут в туннель".into(),
                    "proxy" => format!("Системный прокси {} (только приложения, уважающие прокси)", cached_proxy.system_server),
                    "idle" => "Ядро запущено, трафик не перехватывается".into(),
                    _ => "Выключен".into(),
                },
                mode: happ_mode.into(),
                proxied_apps: cached_happ.proxied_paths.clone(),
            },
            services::ServiceStatus {
                id: "zapret".into(),
                name: "zapret (обход DPI)".into(),
                service: cfg.zapret_service.clone(),
                service_state: svc_state(&cfg.zapret_service),
                gui_running: !pids_of(&["winws.exe"]).is_empty(),
                gui_pids: pids_of(&["winws.exe"]),
                detail: if pids_of(&["winws.exe"]).is_empty() { "winws.exe не запущен".into() } else { "winws.exe фильтрует TCP 80/443 и UDP 443 + игровые порты".into() },
                mode: String::new(),
                proxied_apps: vec![],
            },
            services::ServiceStatus {
                id: "radmin".into(),
                name: "Radmin VPN".into(),
                service: cfg.radmin_service.clone(),
                service_state: svc_state(&cfg.radmin_service),
                gui_running: !pids_of(&["Radmin.exe", "RvRvpnGui.exe"]).is_empty(),
                gui_pids: pids_of(&["Radmin.exe", "RvRvpnGui.exe"]),
                detail: adapters
                    .iter()
                    .find(|a| a.role == "radmin")
                    .map(|a| format!("{} · {} · метрика {}", a.status, a.ipv4.join(", "), a.metric.map(|m| m.to_string()).unwrap_or("-".into())))
                    .unwrap_or_else(|| "Адаптер не найден".into()),
                mode: String::new(),
                proxied_apps: vec![],
            },
        ];

        // apps
        let apps: Vec<AppStatus> = cfg
            .apps
            .iter()
            .map(|a| {
                let procs = found.get(&a.id).cloned().unwrap_or_default();
                let in_happ = cached_happ.proxied_paths.iter().any(|p| {
                    a.exe_paths.iter().any(|e| p.to_lowercase().starts_with(&e.to_lowercase()))
                });
                let rules = cached_qos.iter().filter(|q| q.name.starts_with(&format!("DotPilot-{}-", a.id))).count();
                AppStatus { id: a.id.clone(), running: !procs.is_empty(), procs, in_happ_list: in_happ, qos_rules: rules }
            })
            .collect();

        // game mode
        let game_running: Vec<String> = cfg
            .apps
            .iter()
            .filter(|a| a.kind == "game" && apps.iter().any(|s| s.id == a.id && s.running))
            .map(|a| a.name.clone())
            .collect();
        let gm = {
            let mut g = st.game.lock().unwrap();
            g.auto = cfg.auto_game_mode;
            let desired = match g.manual {
                Some(m) => m,
                None => cfg.auto_game_mode && !game_running.is_empty(),
            };
            g.trigger = if g.manual.is_some() {
                "вручную".into()
            } else if !game_running.is_empty() {
                format!("запущено: {}", game_running.join(", "))
            } else {
                "игры не запущены".into()
            };
            g.active = desired;
            g.clone()
        };
        if prev_game_active != Some(gm.active) {
            let first = prev_game_active.is_none();
            prev_game_active = Some(gm.active);
            if !first || gm.active {
                let cfg2 = cfg.clone();
                let st2 = st.clone();
                let app2 = app.clone();
                let active = gm.active;
                thread::spawn(move || {
                    match policy::apply_qos(&cfg2, active) {
                        Ok(lines) => st2.log(
                            "info",
                            if active {
                                format!("Игровой режим ВКЛ: применено {} QoS-правил (лимиты фоновых приложений активны)", lines.len())
                            } else {
                                format!("Игровой режим ВЫКЛ: применено {} QoS-правил (лимиты сняты)", lines.len())
                            },
                        ),
                        Err(e) => st2.log("error", format!("Игровой режим: ошибка QoS: {e}")),
                    }
                    let _ = app2.emit("game-mode", active);
                });
            }
        }

        let default_via = net::egress_for(&cached_routes, "1.1.1.1").unwrap_or_default();
        let events = {
            let l = st.log.lock().unwrap();
            l.iter().rev().take(60).cloned().collect::<Vec<_>>()
        };
        let snap = Snapshot {
            ts: chrono::Utc::now().timestamp_millis(),
            admin: st.admin,
            adapters,
            routes: cached_routes.clone(),
            wifi: cached_wifi.clone(),
            services: services_v,
            happ: cached_happ.clone(),
            system_proxy_enabled: cached_proxy.system_enabled,
            system_proxy: cached_proxy.system_server.clone(),
            proxy: {
                let local = format!("127.0.0.1:{}", cfg.proxy_port);
                let env_local = cached_proxy.env_http.contains(&local) || cached_proxy.env_https.contains(&local);
                let mut problems = Vec::new();
                if env_local && !cached_alive {
                    problems.push("HTTP_PROXY/HTTPS_PROXY указывают на порт, который никто не слушает: любая новая программа, уважающая эти переменные (Java-лаунчеры, node, git, curl), не выйдет в сеть".into());
                }
                if cached_proxy.system_enabled && !cached_alive {
                    problems.push("Системный прокси включён, но порт мёртв: браузеры и лаунчеры получат «отказано в подключении»".into());
                }
                if !cached_proxy.system_override.split(';').any(|t| t.trim() == "26.*") {
                    problems.push("В исключениях системного прокси нет сети Radmin 26.*".into());
                }
                if env_local && !cached_proxy.env_no_proxy.contains("26.") {
                    problems.push("NO_PROXY не содержит 26.*: программы с переменными прокси пойдут к друзьям по Radmin через Happ".into());
                }
                ProxyState {
                    port: cfg.proxy_port,
                    alive: cached_alive,
                    env: cached_proxy.clone(),
                    env_points_local: env_local,
                    exceptions_ok: cached_proxy.system_override.split(';').any(|t| t.trim() == "26.*"),
                    guard_enabled: cfg.proxy_guard,
                    guard_holding: st.guard_saved.lock().unwrap().is_some(),
                    problems,
                }
            },
            apps,
            cpu,
            mem,
            gpu: cached_gpu.clone(),
            net_rates: rates,
            game_mode: gm,
            qos: cached_qos.clone(),
            power_plans: cached_plans.clone(),
            ping: st.ping.snapshot(if hidden { 2 } else { 180 }),
            events,
            default_via,
            self_stats: st.sys.lock().unwrap().self_stats(),
            hidden,
        };
        // context record for the AI report (every 30 s)
        if last_ctx.elapsed() > Duration::from_secs(30) {
            last_ctx = Instant::now();
            let top = st.sys.lock().unwrap().top_cpu(4);
            let ping: Vec<(String, Option<f32>, f32)> = snap
                .ping
                .iter()
                .map(|p| {
                    let last: Vec<&ping::Sample> = p.samples.iter().rev().take(30).collect();
                    let ok: Vec<f32> = last.iter().filter_map(|s| s.rtt).collect();
                    let avg = if ok.is_empty() { None } else { Some(ok.iter().sum::<f32>() / ok.len() as f32) };
                    let loss = if last.is_empty() { 0.0 } else { (last.len() - ok.len()) as f32 * 100.0 / last.len() as f32 };
                    (p.name.clone(), avg, loss)
                })
                .collect();
            let phys: Vec<&net::Adapter> = snap.adapters.iter().filter(|a| a.status == "Up" && (a.role == "wifi" || a.role == "ethernet")).collect();
            let entry = ContextEntry {
                ts: snap.ts,
                cpu: snap.cpu.usage,
                mem_pct: snap.mem.percent,
                rx_mbps: phys.iter().map(|a| a.rx_bps).sum::<f64>() as f32 / 1e6,
                tx_mbps: phys.iter().map(|a| a.tx_bps).sum::<f64>() as f32 / 1e6,
                wifi_link: snap.adapters.iter().find(|a| a.role == "wifi" && a.status == "Up").map(|a| a.link_speed.clone()).unwrap_or_default(),
                game_mode: snap.game_mode.active,
                running: cfg.apps.iter().filter(|a| snap.apps.iter().any(|s| s.id == a.id && s.running)).map(|a| a.name.clone()).collect(),
                top,
                happ: snap.services.iter().find(|s| s.id == "happ").map(|s| s.mode.clone()).unwrap_or_default(),
                zapret: snap.services.iter().find(|s| s.id == "zapret").map(|s| s.gui_running).unwrap_or(false),
                ping,
            };
            let mut c = st.context.lock().unwrap();
            c.push_back(entry);
            while c.len() > 480 {
                c.pop_front();
            }
        }
        *st.snap.lock().unwrap() = snap.clone();
        if !hidden {
            let _ = app.emit("snapshot", ());
        }
        if last_tray.elapsed() > Duration::from_secs(4) {
            last_tray = Instant::now();
            tray::refresh(&app, snap);
        }
        thread::sleep(poll);
    }
}

// ---------- commands ----------

#[tauri::command]
fn get_snapshot(state: State<'_, Shared>) -> Snapshot {
    state.snap.lock().unwrap().clone()
}

#[tauri::command]
fn get_config(state: State<'_, Shared>) -> Config {
    state.cfg()
}

#[tauri::command]
fn save_config(state: State<'_, Shared>, cfg: Config) -> Result<(), String> {
    config::save(&cfg).map_err(err)?;
    state.ping.set_targets(&cfg.ping_targets);
    *state.cfg.lock().unwrap() = cfg;
    state.log("info", "Настройки сохранены");
    Ok(())
}

#[tauri::command]
async fn apply_policies(state: State<'_, Shared>) -> Result<policy::ApplyReport, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        let gm = st.game.lock().unwrap().active;
        let rep = policy::apply_all(&cfg, gm);
        st.log(if rep.ok { "info" } else { "error" }, format!("Политики применены: {}", rep.lines.first().cloned().unwrap_or_default()));
        rep
    })
    .await
    .map_err(err)
}

#[tauri::command]
async fn set_game_mode(state: State<'_, Shared>, mode: Option<bool>) -> Result<GameModeState, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut g = st.game.lock().unwrap();
        g.manual = mode;
        st.log("info", match mode {
            Some(true) => "Игровой режим включён вручную",
            Some(false) => "Игровой режим выключен вручную",
            None => "Игровой режим: автоматически по запущенным играм",
        });
        g.clone()
    })
    .await
    .map_err(err)
}

#[tauri::command]
async fn set_interface_metric(state: State<'_, Shared>, if_index: u32, metric: Option<u32>) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = net::set_interface_metric(if_index, metric).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Метрика интерфейса #{if_index} → {:?}: {:?}", metric, r));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn set_adapter_enabled(state: State<'_, Shared>, name: String, enabled: bool) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = net::set_adapter_enabled(&name, enabled).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Адаптер «{name}» {}: {:?}", if enabled { "включён" } else { "выключен" }, r));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn service_control(state: State<'_, Shared>, id: String, action: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        let r: anyhow::Result<String> = match (id.as_str(), action.as_str()) {
            ("happ", "start") => services::set_service(&cfg.happ_service, true),
            ("happ", "stop") => services::set_service(&cfg.happ_service, false).and_then(|s| {
                let _ = services::kill_process("xray");
                let _ = services::kill_process("sing-box");
                Ok(s)
            }),
            ("happ", "launch") => services::launch(&cfg.happ_exe, &[], None).map(|p| format!("pid {p}")),
            ("happ", "kill") => services::kill_process("Happ"),
            ("zapret", "start") => services::zapret_start(&cfg),
            ("zapret", "stop") => services::zapret_stop(&cfg),
            ("zapret", "launch") => services::zapret_start(&cfg),
            ("radmin", "start") => services::set_service(&cfg.radmin_service, true),
            ("radmin", "stop") => services::set_service(&cfg.radmin_service, false),
            ("radmin", "launch") => services::launch(&cfg.radmin_exe, &[], None).map(|p| format!("pid {p}")),
            ("radmin", "kill") => services::kill_process("Radmin"),
            _ => Err(anyhow::anyhow!("unknown service/action {id}/{action}")),
        };
        let r = r.map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("{id} {action}: {}", match &r { Ok(s) => s.trim().to_string(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn launch_app(state: State<'_, Shared>, app_id: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        let app = cfg.apps.iter().find(|a| a.id == app_id).ok_or_else(|| "app not found".to_string())?;
        let exe = app.exe_paths.iter().find(|p| p.to_lowercase().ends_with(".exe")).ok_or_else(|| "нет exe".to_string())?;
        let cwd = std::path::Path::new(exe).parent().map(|p| p.to_string_lossy().to_string());
        // Environment per policy: direct/radmin apps must never inherit proxy variables
        // (a Java launcher with HTTPS_PROXY pointing at a dead port cannot list versions).
        let mut cmd = std::process::Command::new(exe);
        if let Some(d) = &cwd {
            cmd.current_dir(d);
        }
        let alive = services::port_alive(cfg.proxy_port);
        match app.policy {
            config::Policy::Direct | config::Policy::Radmin => {
                for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
                    cmd.env_remove(k);
                }
                cmd.env("NO_PROXY", "*");
                cmd.env("JAVA_TOOL_OPTIONS", "-Djava.net.useSystemProxies=false");
            }
            config::Policy::Vpn => {
                if alive {
                    let p = format!("http://127.0.0.1:{}", cfg.proxy_port);
                    cmd.env("HTTP_PROXY", &p).env("HTTPS_PROXY", &p).env("NO_PROXY", services::NO_PROXY_DEFAULT);
                } else {
                    for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
                        cmd.env_remove(k);
                    }
                }
            }
        }
        let r = cmd.spawn().map(|c| format!("pid {}{}", c.id(), if matches!(app.policy, config::Policy::Vpn) && !alive { " (Happ не отвечает — запущено без прокси)" } else { "" })).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Запуск {} ({:?}): {:?}", app.name, app.policy, r));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn get_connections(state: State<'_, Shared>, app_id: String) -> Result<Vec<net::Connection>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (pids, routes) = {
            let s = st.snap.lock().unwrap();
            let pids: Vec<u32> = s.apps.iter().find(|a| a.id == app_id).map(|a| a.procs.iter().map(|p| p.pid).collect()).unwrap_or_default();
            (pids, s.routes.clone())
        };
        net::connections(&pids, &routes).map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn tweaks_state() -> Result<Vec<policy::Tweak>, String> {
    tauri::async_runtime::spawn_blocking(|| policy::tweaks_state().map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn set_tweak(state: State<'_, Shared>, id: String, on: bool) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = policy::set_tweak(&id, on).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Твик {id} → {on}: {:?}", r));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn quick_action(state: State<'_, Shared>, id: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = policy::quick_action(&id).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Действие {id}: {}", match &r { Ok(s) => s.trim().to_string(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn set_power_plan(state: State<'_, Shared>, guid: String) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = sysmon::set_power_plan(&guid).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Схема питания → {guid}: {:?}", r));
        r
    })
    .await
    .map_err(err)?
}

pub fn do_apply_profile(st: &AppState, id: &str) -> Result<Vec<String>, String> {
    let mut cfg = st.cfg();
    let profile = cfg.profiles.iter().find(|p| p.id == id).cloned().ok_or_else(|| "profile not found".to_string())?;
    let mut lines = Vec::new();
    if !profile.power_plan.is_empty() {
        match sysmon::set_power_plan(&profile.power_plan) {
            Ok(_) => lines.push("Схема питания переключена".into()),
            Err(e) => lines.push(format!("Схема питания: {e}")),
        }
    }
    {
        let mut g = st.game.lock().unwrap();
        g.manual = if profile.game_mode { Some(true) } else { None };
        lines.push(if profile.game_mode { "Игровой режим сети: включён".into() } else { "Игровой режим сети: автоматически".into() });
    }
    if let Some(z) = profile.zapret_running {
        let r = if z { services::zapret_start(&cfg) } else { services::zapret_stop(&cfg) };
        lines.push(format!("zapret: {}", match r { Ok(s) => s.trim().to_string(), Err(e) => e.to_string() }));
    }
    if let Some(h) = profile.happ_running {
        let r = services::set_service(&cfg.happ_service, h);
        lines.push(format!("Happ: {}", match r { Ok(s) => s.trim().to_string(), Err(e) => e.to_string() }));
    }
    cfg.active_profile = id.to_string();
    let _ = config::save(&cfg);
    *st.cfg.lock().unwrap() = cfg;
    st.log("info", format!("Профиль «{}» применён", profile.name));
    Ok(lines)
}

#[tauri::command]
async fn apply_profile(state: State<'_, Shared>, id: String) -> Result<Vec<String>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || do_apply_profile(&st, &id)).await.map_err(err)?
}

fn build_export(st: &AppState) -> serde_json::Value {
    let snap = st.snap.lock().unwrap().clone();
    let mut cfg = st.cfg();
    cfg.anthropic_api_key = if cfg.anthropic_api_key.is_empty() { String::new() } else { "***".into() };
    let mut v = serde_json::to_value(&snap).unwrap_or_default();
    v["ping"] = serde_json::to_value(st.ping.all_samples()).unwrap_or_default();
    v["config"] = serde_json::to_value(&cfg).unwrap_or_default();
    v["events"] = serde_json::to_value(st.log.lock().unwrap().clone()).unwrap_or_default();
    v["exported_at"] = serde_json::Value::String(chrono::Local::now().to_rfc3339());
    v
}

#[tauri::command]
async fn export_snapshot(state: State<'_, Shared>, path: String, format: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let data = build_export(&st);
        let content = if format == "csv" {
            let mut s = String::from("time;target;host;rtt_ms\n");
            if let Some(series) = data.get("ping").and_then(|p| p.as_array()) {
                for t in series {
                    let name = t.get("name").and_then(|x| x.as_str()).unwrap_or("");
                    let host = t.get("host").and_then(|x| x.as_str()).unwrap_or("");
                    for smp in t.get("samples").and_then(|x| x.as_array()).into_iter().flatten() {
                        let ts = smp.get("t").and_then(|x| x.as_i64()).unwrap_or(0);
                        let dt = chrono::DateTime::from_timestamp_millis(ts).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
                        let rtt = smp.get("rtt").and_then(|x| x.as_f64()).map(|r| format!("{r:.1}")).unwrap_or_else(|| "loss".into());
                        s.push_str(&format!("{dt};{name};{host};{rtt}\n"));
                    }
                }
            }
            s
        } else {
            serde_json::to_string_pretty(&data).map_err(err)?
        };
        std::fs::write(&path, content).map_err(err)?;
        st.log("info", format!("Экспорт сохранён: {path}"));
        Ok(path)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn ask_claude(state: State<'_, Shared>, question: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        let mut data = build_export(&st);
        // trim ping samples to stats + last 60 to keep the prompt small
        if let Some(series) = data.get_mut("ping").and_then(|p| p.as_array_mut()) {
            for t in series.iter_mut() {
                if let Some(s) = t.get_mut("samples").and_then(|x| x.as_array_mut()) {
                    let keep: Vec<_> = s.iter().rev().take(60).cloned().collect();
                    *s = keep.into_iter().rev().collect();
                }
            }
        }
        data["config"]["anthropic_api_key"] = serde_json::Value::Null;
        let r = ai::ask(&cfg.anthropic_api_key, &cfg.ai_model, &cfg.ai_proxy, &cfg.ai_effort, &question, &data).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, match &r { Ok(_) => "Claude: ответ получен".to_string(), Err(e) => format!("Claude: {e}") });
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn audio_info() -> Result<audio::AudioInfo, String> {
    tauri::async_runtime::spawn_blocking(|| audio::list().map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn audio_set_default(state: State<'_, Shared>, id: String, role: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = audio::set_default(&id, &role).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Звук: устройство по умолчанию ({role}) → {id}: {:?}", r));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn audio_set_enabled(state: State<'_, Shared>, instance_id: String, enabled: bool) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = audio::set_enabled(&instance_id, enabled).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Звук: {} {}: {:?}", if enabled { "включено" } else { "отключено" }, instance_id, r));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn eq_apply(state: State<'_, Shared>, preset: String, device: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = audio::eq_apply(&preset, &device).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Эквалайзер: {}", match &r { Ok(s) => s.clone(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn open_sound(target: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || audio::open_sound(&target).map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn perms_status(state: State<'_, Shared>) -> Result<Vec<perms::PermItem>, String> {
    let admin = state.admin;
    tauri::async_runtime::spawn_blocking(move || perms::status(admin).map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn perms_grant(state: State<'_, Shared>, id: String, on: bool) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let r = perms::grant(&id, on, &exe).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Доступ {id} → {on}: {}", match &r { Ok(s) => s.trim().to_string(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn relaunch_elevated(app: tauri::AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(err)?;
    let dir = exe.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let exe_s = exe.to_string_lossy().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        ps::run_ps(&format!("Start-Process -FilePath {} -WorkingDirectory {} -Verb RunAs", ps::ps_quote(&exe_s), ps::ps_quote(&dir))).map_err(err)
    })
    .await
    .map_err(err)??;
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1500));
        app.exit(0);
    });
    Ok(())
}

#[tauri::command]
async fn open_uri(uri: String) -> Result<(), String> {
    let ok = ["ms-settings:", "tg://", "https://", "http://", "ms-availablenetworks:"].iter().any(|p| uri.starts_with(p));
    if !ok || uri.contains('"') || uri.contains('&') && !uri.starts_with("tg://") && !uri.starts_with("http") {
        return Err(format!("Ссылка не разрешена: {uri}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        // `start` treats the URI as a document; "" is the (empty) window title argument.
        ps::run_ps(&format!("Start-Process {}", ps::ps_quote(&uri))).map(|_| ()).map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn get_processes(state: State<'_, Shared>, limit: usize) -> Result<Vec<sysmon::ProcRow>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let services = {
            let mut m = st.svc_map.lock().unwrap();
            let stale = m.0.map(|t| t.elapsed() > Duration::from_secs(30)).unwrap_or(true);
            if stale {
                if let Ok(map) = services::service_pids() {
                    *m = (Some(Instant::now()), map);
                }
            }
            m.1.clone()
        };
        let gpu = sysmon::gpu_apps();
        let mut sys = st.sys.lock().unwrap();
        Ok(sys.top_processes(limit.clamp(10, 300), &services, &gpu))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn kill_process(state: State<'_, Shared>, pid: u32) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = st.sys.lock().unwrap().kill(pid).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Процессы: {}", match &r { Ok(s) => s.clone(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn sound_fix_footsteps(state: State<'_, Shared>, products: Vec<String>, preset: String) -> Result<Vec<String>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = audio::fix_footsteps(&products, &preset).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Звук: настройка шагов для {}: {}", products.join(", "), match &r { Ok(l) => l.join(" · "), Err(e) => e.clone() }));
        if r.is_ok() {
            let mut cfg = st.cfg();
            cfg.headphone_products = products.clone();
            let _ = config::save(&cfg);
            *st.cfg.lock().unwrap() = cfg;
        }
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn sound_reset(state: State<'_, Shared>) -> Result<Vec<String>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = audio::reset_sound().map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Звук: сброс: {}", match &r { Ok(l) => l.join(" · "), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn apo_toggle(state: State<'_, Shared>, id: String, attach: bool) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = if attach { audio::apo_attach(&id) } else { audio::apo_detach(&id) };
        let r = r.and_then(|s| audio::restart_audio().map(|_| s)).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Equalizer APO: {}", match &r { Ok(s) => s.clone(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct TgwsStatus {
    pub running: bool,
    pub pid: u32,
    pub ports: Vec<String>,
    pub exe_exists: bool,
    pub telegram_running: bool,
}

fn tgws_find(st: &AppState) -> Option<u32> {
    let cfg = st.cfg();
    let name = std::path::Path::new(&cfg.tgws_exe).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    let mut sys = st.sys.lock().unwrap();
    sys.find_by_name(&[&name]).first().map(|p| p.pid)
}

#[tauri::command]
async fn tgws_status(state: State<'_, Shared>) -> Result<TgwsStatus, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        let pid = tgws_find(&st);
        let tg_name = std::path::Path::new(&cfg.tg_exe).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let telegram_running = !tg_name.is_empty() && !st.sys.lock().unwrap().find_by_name(&[&tg_name]).is_empty();
        Ok(TgwsStatus {
            running: pid.is_some(),
            pid: pid.unwrap_or(0),
            ports: pid.map(services::listening_ports).unwrap_or_default(),
            exe_exists: std::path::Path::new(&cfg.tgws_exe).exists(),
            telegram_running,
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn tgws_control(state: State<'_, Shared>, action: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        let r: Result<String, String> = match action.as_str() {
            "start" => {
                if tgws_find(&st).is_some() {
                    Ok("уже запущен".into())
                } else {
                    let dir = std::path::Path::new(&cfg.tgws_exe).parent().map(|p| p.to_string_lossy().to_string());
                    ps::run_ps(&format!("Start-Process -FilePath {} -WorkingDirectory {} -WindowStyle Hidden -PassThru | Select-Object -ExpandProperty Id", ps::ps_quote(&cfg.tgws_exe), ps::ps_quote(dir.as_deref().unwrap_or("."))))
                        .map(|s| {
                            let pid = s.trim().parse::<u32>().unwrap_or(0);
                            *st.tgws_pid.lock().unwrap() = Some(pid);
                            format!("запущен, pid {pid}")
                        })
                        .map_err(err)
                }
            }
            "stop" => {
                let name = std::path::Path::new(&cfg.tgws_exe).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                services::kill_process(&name).map(|_| "остановлен".into()).map_err(err)
            }
            "launch-telegram" => services::launch(&cfg.tg_exe, &[], None).map(|p| format!("Telegram запущен, pid {p}")).map_err(err),
            _ => Err("unknown action".into()),
        };
        st.log(if r.is_ok() { "info" } else { "error" }, format!("TgWsProxy {action}: {}", match &r { Ok(s) => s.clone(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn reset_config(state: State<'_, Shared>) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let old = st.cfg();
        let mut cfg = Config::default();
        cfg.anthropic_api_key = old.anthropic_api_key; // keep the key
        config::save(&cfg).map_err(err)?;
        st.ping.set_targets(&cfg.ping_targets);
        *st.cfg.lock().unwrap() = cfg;
        let _ = policy::remove_qos();
        st.game.lock().unwrap().manual = None;
        st.log("info", "Настройки сброшены к значениям по умолчанию, QoS-правила DotPilot удалены");
        Ok(())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn tweaks_reset(state: State<'_, Shared>) -> Result<Vec<String>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut lines = Vec::new();
        for (id, on) in [("throttling", false), ("responsiveness", false), ("nagle", false), ("autotuning", true), ("ecn", false), ("nla", false)] {
            match policy::set_tweak(id, on) {
                Ok(_) => lines.push(format!("{id}: значение Windows по умолчанию")),
                Err(e) => lines.push(format!("{id}: {e}")),
            }
        }
        st.log("info", "Твики сброшены к значениям Windows по умолчанию");
        Ok(lines)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn remove_qos_rules(state: State<'_, Shared>) -> Result<(), String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        policy::remove_qos().map_err(err)?;
        st.game.lock().unwrap().manual = Some(false);
        st.log("info", "Все QoS-правила DotPilot удалены; игровой режим выключен вручную");
        Ok(())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn proxy_fix(state: State<'_, Shared>, action: String) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = st.cfg();
        if action == "set-env" || action == "clear-env" {
            *st.guard_saved.lock().unwrap() = None;
        }
        let r = services::proxy_fix(&action, cfg.proxy_port).map_err(err);
        st.log(if r.is_ok() { "info" } else { "error" }, format!("Прокси: {}", match &r { Ok(s) => s.trim().to_string(), Err(e) => e.clone() }));
        r
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn ai_report(state: State<'_, Shared>, path: Option<String>) -> Result<String, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let md = report::build(&st);
        if let Some(p) = path {
            std::fs::write(&p, &md).map_err(err)?;
            st.log("info", format!("Лог для ИИ сохранён: {p}"));
        }
        Ok(md)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn self_sample(state: State<'_, Shared>) -> Result<sysmon::SelfStats, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(st.sys.lock().unwrap().self_sample())).await.map_err(err)?
}

#[tauri::command]
fn bench_mode(state: State<'_, Shared>, hidden: bool) {
    state.bench_hidden.store(hidden, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn config_file_path() -> String {
    config::config_path().to_string_lossy().to_string()
}

#[tauri::command]
fn app_log(state: State<'_, Shared>, level: String, msg: String) {
    state.log(&level, msg);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg = config::load();
    let ping = ping::PingMonitor::new();
    ping.set_targets(&cfg.ping_targets);
    let admin = is_admin();
    let state: Shared = Arc::new(AppState {
        context: Mutex::new(std::collections::VecDeque::new()),
        bench_hidden: std::sync::atomic::AtomicBool::new(false),
        cfg: Mutex::new(cfg),
        snap: Mutex::new(Snapshot::default()),
        ping,
        sys: Mutex::new(sysmon::SysCollector::new()),
        log: Mutex::new(Vec::new()),
        game: Mutex::new(GameModeState::default()),
        admin,
        svc_map: Mutex::new((None, HashMap::new())),
        tgws_pid: Mutex::new(None),
        guard_saved: Mutex::new(None),
    });
    // Optional helper autostart (TgWsProxy for Telegram).
    {
        let cfg = state.cfg();
        if cfg.tgws_autostart && std::path::Path::new(&cfg.tgws_exe).exists() && tgws_find(&state).is_none() {
            let dir = std::path::Path::new(&cfg.tgws_exe).parent().map(|p| p.to_string_lossy().to_string());
            match ps::run_ps(&format!("Start-Process -FilePath {} -WorkingDirectory {} -WindowStyle Hidden -PassThru | Select-Object -ExpandProperty Id", ps::ps_quote(&cfg.tgws_exe), ps::ps_quote(dir.as_deref().unwrap_or(".")))) {
                Ok(s) => state.log("info", format!("TgWsProxy запущен автоматически (pid {})", s.trim())),
                Err(e) => state.log("error", format!("TgWsProxy автозапуск: {e}")),
            }
        }
    }
    state.log("info", if admin { "DotPilot запущен с правами администратора" } else { "DotPilot запущен БЕЗ прав администратора: управление сетью недоступно" });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            tray::setup(&handle)?;

            // Проверка сбоя должна пройти до того, как что-либо снова применится
            // к карте: неподтверждённая запись в журнале означает прошлый вылет.
            let report = ocsafe::boot_check();
            if report.recovered {
                state.log("warn", report.message.clone());
            }

            let st = state.clone();
            thread::Builder::new().name("collector".into()).spawn(move || collector(handle, st)).ok();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_config,
            save_config,
            apply_policies,
            set_game_mode,
            set_interface_metric,
            set_adapter_enabled,
            service_control,
            launch_app,
            get_connections,
            tweaks_state,
            set_tweak,
            quick_action,
            set_power_plan,
            apply_profile,
            export_snapshot,
            ask_claude,
            audio_info,
            audio_set_default,
            audio_set_enabled,
            eq_apply,
            open_sound,
            perms_status,
            perms_grant,
            relaunch_elevated,
            open_uri,
            get_processes,
            kill_process,
            sound_fix_footsteps,
            sound_reset,
            apo_toggle,
            tgws_status,
            tgws_control,
            reset_config,
            tweaks_reset,
            remove_qos_rules,
            ai_report,
            proxy_fix,
            self_sample,
            bench_mode,
            config_file_path,
            app_log,
            gpu_nvapi,
            gpu_capabilities,
            oc_state,
            oc_apply,
            oc_confirm,
            oc_reject,
            oc_reset,
            oc_validate,
            bench_cpu,
            bench_memory,
            gpu_peak_temp,
            oc_propose,
            platform_state,
            platform_measure,
            platform_baselines,
            bios_advice,
            health_check,
            health_advice,
            voltage_state,
            voltage_set_offset,
            voltage_reset,
            trends_report,
            fans_state,
            fans_set_limits,
            fans_set_zero,
            fans_set_sensor,
            fans_force_on,
            fans_restore
        ])
        .run(tauri::generate_context!())
        .expect("error while running DotPilot");
}

