//! Ядро безопасности разгона: журнал намерений, обнаружение сбоев, откат.
//!
//! Главная идея: **намерение записывается на диск до того, как что-то применяется
//! к железу**. Если приложение или вся система погибнут между записью и подтверждением,
//! при следующем запуске в журнале останется неподтверждённая запись — значит, был сбой.
//! Настройка помечается плохой и больше не предлагается.
//!
//! Разгон видеокарты через NVAPI волатилен: драйвер держит смещения только до
//! перезагрузки и не пишет их в саму карту. Поэтому аварийная перезагрузка сама по себе
//! является полным откатом, а нам остаётся не применить плохое значение повторно.
//!
//! Границы значений живут здесь, в коде. Ответ нейросети — недоверенный ввод: он
//! проходит `Bounds::clamp`, и превысить абсолютные потолки не может в принципе.

use crate::nvapi;
use crate::ps;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

// --- абсолютные потолки ----------------------------------------------------
//
// Их нельзя превысить ни настройкой, ни ответом модели, ни правкой файла журнала:
// пользовательские границы сами зажимаются этими числами.

const ABS_CORE_MIN: i32 = -500;
const ABS_CORE_MAX: i32 = 300;
const ABS_MEM_MIN: i32 = -1000;
const ABS_MEM_MAX: i32 = 1500;
const ABS_POWER_MIN: f32 = 50.0;
const ABS_POWER_MAX: f32 = 110.0;
const ABS_FAN_MIN: u32 = 30;
const ABS_FAN_MAX: u32 = 100;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Bounds {
    pub core_min_mhz: i32,
    pub core_max_mhz: i32,
    pub mem_min_mhz: i32,
    pub mem_max_mhz: i32,
    pub power_min_percent: f32,
    pub power_max_percent: f32,
    pub fan_min_percent: u32,
    pub fan_max_percent: u32,
}

impl Default for Bounds {
    fn default() -> Self {
        // Осознанно уже абсолютных потолков: это стартовый коридор для подбора.
        Self {
            core_min_mhz: -200,
            core_max_mhz: 200,
            mem_min_mhz: -500,
            mem_max_mhz: 1000,
            power_min_percent: 70.0,
            power_max_percent: 110.0,
            fan_min_percent: 30,
            fan_max_percent: 100,
        }
    }
}

impl Bounds {
    /// Зажимает сами границы абсолютными потолками — защита от правки файла журнала.
    pub fn sanitized(self) -> Self {
        Self {
            core_min_mhz: self.core_min_mhz.clamp(ABS_CORE_MIN, ABS_CORE_MAX),
            core_max_mhz: self.core_max_mhz.clamp(ABS_CORE_MIN, ABS_CORE_MAX),
            mem_min_mhz: self.mem_min_mhz.clamp(ABS_MEM_MIN, ABS_MEM_MAX),
            mem_max_mhz: self.mem_max_mhz.clamp(ABS_MEM_MIN, ABS_MEM_MAX),
            power_min_percent: self.power_min_percent.clamp(ABS_POWER_MIN, ABS_POWER_MAX),
            power_max_percent: self.power_max_percent.clamp(ABS_POWER_MIN, ABS_POWER_MAX),
            fan_min_percent: self.fan_min_percent.clamp(ABS_FAN_MIN, ABS_FAN_MAX),
            fan_max_percent: self.fan_max_percent.clamp(ABS_FAN_MIN, ABS_FAN_MAX),
        }
    }

    /// Приводит предложенные значения в разрешённый коридор.
    pub fn clamp(self, c: GpuCandidate) -> GpuCandidate {
        let b = self.sanitized();
        GpuCandidate {
            core_offset_mhz: c.core_offset_mhz.clamp(b.core_min_mhz, b.core_max_mhz),
            mem_offset_mhz: c.mem_offset_mhz.clamp(b.mem_min_mhz, b.mem_max_mhz),
            power_percent: c.power_percent.clamp(b.power_min_percent, b.power_max_percent),
            fan_level: c.fan_level.map(|l| l.clamp(b.fan_min_percent, b.fan_max_percent)),
        }
    }
}

// --- кандидат и ступени ----------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct GpuCandidate {
    pub core_offset_mhz: i32,
    pub mem_offset_mhz: i32,
    pub power_percent: f32,
    /// `None` — вентиляторами управляет драйвер.
    pub fan_level: Option<u32>,
}

impl Default for GpuCandidate {
    fn default() -> Self {
        Self { core_offset_mhz: 0, mem_offset_mhz: 0, power_percent: 100.0, fan_level: None }
    }
}

/// Ступени проверки. Настройка попадает в «проверенные» только после длинной.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Smoke,
    Medium,
    Long,
}

impl Stage {
    pub fn seconds(self) -> u64 {
        match self {
            Stage::Smoke => 30,
            Stage::Medium => 300,
            Stage::Long => 1800,
        }
    }
    pub fn next(self) -> Option<Stage> {
        match self {
            Stage::Smoke => Some(Stage::Medium),
            Stage::Medium => Some(Stage::Long),
            Stage::Long => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Stage::Smoke => "короткая (30 с)",
            Stage::Medium => "средняя (5 мин)",
            Stage::Long => "длинная (30 мин)",
        }
    }
}

// --- журнал ----------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pending {
    pub candidate: GpuCandidate,
    pub stage: Stage,
    pub applied_at: i64,
    /// Момент загрузки ОС: позволяет отличить перезагрузку от падения приложения.
    pub boot_id: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rejected {
    pub candidate: GpuCandidate,
    pub reason: String,
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Event {
    pub at: i64,
    pub kind: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Journal {
    /// Применено, но ещё не признано стабильным.
    pub pending: Option<Pending>,
    /// Последняя настройка, прошедшая длинную ступень.
    pub last_known_good: Option<GpuCandidate>,
    pub rejected: Vec<Rejected>,
    pub history: Vec<Event>,
    #[serde(default)]
    pub bounds: Bounds,
}

fn journal_path() -> PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.oc.json");
    p
}

pub fn load() -> Journal {
    fs::read_to_string(journal_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Journal>(&s).ok())
        .unwrap_or_default()
}

/// Запись журнала с принудительным сбросом на диск.
///
/// Без `sync_all` запись могла бы остаться в кеше ОС и потеряться при жёстком
/// зависании — ровно в том случае, ради которого журнал и существует.
pub fn store(j: &Journal) -> Result<(), String> {
    let text = serde_json::to_string_pretty(j).map_err(|e| e.to_string())?;
    let path = journal_path();
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

/// Момент загрузки ОС в секундах Unix, огрублённый до минуты.
///
/// Одинаков для всех запусков приложения внутри одной загрузки и меняется после
/// перезагрузки — этого достаточно, чтобы отличить падение приложения от падения системы.
fn boot_id() -> i64 {
    let uptime_secs = (unsafe { GetTickCount64() } / 1000) as i64;
    let raw = now() - uptime_secs;
    raw - raw.rem_euclid(60)
}

/// Ищет в журнале Windows признаки аварийного завершения после указанного момента.
///
/// 41 — Kernel-Power (система перезагрузилась без корректного завершения),
/// 1001 — BugCheck (синий экран), 6008 — неожиданное выключение.
fn unclean_since(ts: i64) -> Option<String> {
    let script = format!(
        "$t=[DateTimeOffset]::FromUnixTimeSeconds({}).LocalDateTime; \
         $e=Get-WinEvent -FilterHashtable @{{LogName='System'; Id=41,1001,6008; StartTime=$t}} -MaxEvents 5 -ErrorAction SilentlyContinue; \
         if($e){{ ($e | ForEach-Object {{ \"$($_.Id): $($_.TimeCreated.ToString('dd.MM HH:mm'))\" }}) -join '; ' }}",
        ts
    );
    let out = ps::run_ps(&script).ok()?;
    let t = out.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

// --- применение и откат ----------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct ApplyReport {
    /// Что реально применилось после обрезки границами.
    pub applied: GpuCandidate,
    /// Было ли предложение урезано.
    pub clamped: bool,
    pub stage: Stage,
    /// Сколько секунд должна длиться проверка этой ступени.
    pub stage_seconds: u64,
    pub message: String,
}

/// Применяет кандидата: сначала журнал, потом железо.
///
/// Порядок принципиален. Если поменять его местами и умереть между применением
/// и записью, при следующем запуске мы не узнаем, что настройка была применена.
pub fn apply(requested: GpuCandidate, stage: Stage) -> Result<ApplyReport, String> {
    let mut j = load();
    let bounds = j.bounds.sanitized();
    let candidate = bounds.clamp(requested);
    let clamped = candidate != requested;

    if j.pending.is_some() {
        return Err("Предыдущая настройка ещё не подтверждена. Сначала подтвердите её или отклоните.".into());
    }
    if j.rejected.iter().any(|r| r.candidate == candidate) {
        return Err("Эта комбинация уже приводила к сбою и повторно не применяется.".into());
    }

    j.pending = Some(Pending { candidate, stage, applied_at: now(), boot_id: boot_id() });
    j.history.push(Event {
        at: now(),
        kind: "apply".into(),
        text: format!(
            "Применяется: ядро {:+} МГц, память {:+} МГц, мощность {:.0} %, ступень {}",
            candidate.core_offset_mhz, candidate.mem_offset_mhz, candidate.power_percent, stage.label()
        ),
    });
    store(&j)?;

    // Железо трогаем только после того, как намерение легло на диск.
    if let Err(e) = push_to_hardware(&candidate) {
        // Не удалось применить — снимаем намерение и возвращаем карту в штатное.
        let _ = nvapi::reset_all();
        let mut j = load();
        j.pending = None;
        j.history.push(Event { at: now(), kind: "error".into(), text: format!("Применение не удалось: {e}") });
        store(&j)?;
        return Err(e);
    }

    Ok(ApplyReport {
        applied: candidate,
        clamped,
        stage,
        stage_seconds: stage.seconds(),
        message: if clamped {
            "Предложение выходило за границы и было урезано до разрешённого коридора.".into()
        } else {
            "Применено. Настройка не подтверждена, пока не пройдёт проверку.".into()
        },
    })
}

fn push_to_hardware(c: &GpuCandidate) -> Result<(), String> {
    nvapi::set_power_limit_percent(c.power_percent)?;
    nvapi::set_clock_offsets(c.core_offset_mhz, c.mem_offset_mhz)?;
    nvapi::set_fan_level(c.fan_level)?;
    Ok(())
}

/// Настройка выдержала текущую ступень: переходим к следующей либо признаём проверенной.
pub fn confirm() -> Result<Journal, String> {
    let mut j = load();
    let p = j.pending.clone().ok_or_else(|| "Нет настройки, ожидающей подтверждения.".to_string())?;

    match p.stage.next() {
        Some(next) => {
            j.pending = Some(Pending { stage: next, applied_at: now(), boot_id: boot_id(), ..p });
            j.history.push(Event {
                at: now(),
                kind: "stage".into(),
                text: format!("Ступень {} пройдена, переходим к ступени {}", p.stage.label(), next.label()),
            });
        }
        None => {
            j.last_known_good = Some(p.candidate);
            j.pending = None;
            j.history.push(Event {
                at: now(),
                kind: "promote".into(),
                text: format!(
                    "Настройка признана проверенной: ядро {:+} МГц, память {:+} МГц, мощность {:.0} %",
                    p.candidate.core_offset_mhz, p.candidate.mem_offset_mhz, p.candidate.power_percent
                ),
            });
        }
    }
    store(&j)?;
    Ok(j)
}

/// Настройка не прошла проверку: откат и запись в чёрный список.
pub fn reject(reason: &str) -> Result<Journal, String> {
    let mut j = load();
    let p = j.pending.clone().ok_or_else(|| "Нет настройки, ожидающей подтверждения.".to_string())?;

    let revert = revert_to_safe(&j);
    j.rejected.push(Rejected { candidate: p.candidate, reason: reason.to_string(), at: now() });
    j.pending = None;
    j.history.push(Event {
        at: now(),
        kind: "reject".into(),
        text: format!("Откат: {reason}. {revert}"),
    });
    store(&j)?;
    Ok(j)
}

/// Возвращает карту к проверенной настройке, а если её нет — к штатным значениям.
fn revert_to_safe(j: &Journal) -> String {
    match j.last_known_good {
        Some(good) => match push_to_hardware(&good) {
            Ok(()) => "Возвращена последняя проверенная настройка.".into(),
            Err(e) => format!("Не удалось вернуть проверенную настройку ({e}); сброс к штатным значениям: {}",
                              match nvapi::reset_all() { Ok(()) => "выполнен".to_string(), Err(e2) => format!("не удался — {e2}") }),
        },
        None => match nvapi::reset_all() {
            Ok(()) => "Карта возвращена к штатным значениям.".into(),
            Err(e) => format!("Сброс к штатным значениям не удался: {e}"),
        },
    }
}

/// Полный сброс: снять разгон и забыть незавершённую попытку.
pub fn reset() -> Result<Journal, String> {
    let mut j = load();
    let r = nvapi::reset_all();
    j.pending = None;
    j.history.push(Event {
        at: now(),
        kind: "reset".into(),
        text: match &r {
            Ok(()) => "Ручной сброс: карта возвращена к штатным значениям.".into(),
            Err(e) => format!("Ручной сброс завершился с ошибкой: {e}"),
        },
    });
    store(&j)?;
    r.map(|_| j)
}

// --- вердикт по ступени ----------------------------------------------------

/// Выше этой температуры настройка считается неприемлемой, даже если ошибок нет:
/// цель разгона — чтобы карта служила дольше, а не работала на пределе.
pub const TEMP_LIMIT_C: i32 = 83;

/// Что собрал тест за время ступени.
#[derive(Deserialize, Clone, Debug)]
pub struct StageEvidence {
    /// Несовпадения контрольных сумм в вычислительном тесте видеокарты.
    pub gpu_mismatches: u64,
    /// Момент начала проверки, unix-секунды.
    pub started_at: i64,
    /// Наибольшая температура ядра за проверку.
    pub peak_temp_c: Option<i32>,
    /// Доработал ли тест до конца (false — прервался или завис).
    pub completed: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct StageVerdict {
    pub passed: bool,
    pub reason: String,
    /// Сбои видеодрайвера, найденные в журнале Windows за время проверки.
    pub faults: Vec<String>,
    pub journal: Journal,
}

/// Решает судьбу текущей ступени и сразу применяет решение.
///
/// Проверяются четыре независимых признака. Достаточно одного, чтобы откатить:
/// молчаливые ошибки вычислений, восстановление видеодрайвера, перегрев и
/// незавершённый тест. Последнее важно отдельно: зависший тест — это не «успех».
pub fn validate(ev: StageEvidence) -> Result<StageVerdict, String> {
    let faults = crate::bench::gpu_faults_since(ev.started_at);

    let failure = if !ev.completed {
        Some("тест не доработал до конца".to_string())
    } else if ev.gpu_mismatches > 0 {
        Some(format!(
            "видеокарта вернула неверный результат {} раз — тихая ошибка вычислений",
            ev.gpu_mismatches
        ))
    } else if !faults.is_empty() {
        Some(format!("видеодрайвер восстанавливался: {}", faults.join(", ")))
    } else if ev.peak_temp_c.is_some_and(|t| t > TEMP_LIMIT_C) {
        Some(format!(
            "температура дошла до {} °C при потолке {} °C",
            ev.peak_temp_c.unwrap_or_default(),
            TEMP_LIMIT_C
        ))
    } else {
        None
    };

    match failure {
        Some(reason) => {
            let journal = reject(&reason)?;
            Ok(StageVerdict { passed: false, reason, faults, journal })
        }
        None => {
            let journal = confirm()?;
            let reason = match &journal.pending {
                Some(p) => format!("Ступень пройдена, переходим к проверке: {}", p.stage.label()),
                None => "Все ступени пройдены, настройка признана проверенной.".to_string(),
            };
            Ok(StageVerdict { passed: true, reason, faults, journal })
        }
    }
}

// --- проверка при старте ---------------------------------------------------

#[derive(Serialize, Clone, Debug, Default)]
pub struct BootReport {
    /// Была ли найдена неподтверждённая настройка, то есть признак сбоя.
    pub recovered: bool,
    /// Перезагружалась ли система (в отличие от падения только приложения).
    pub rebooted: bool,
    /// Найденные в журнале Windows события аварийного завершения.
    pub evidence: Option<String>,
    pub message: String,
}

/// Вызывается один раз при запуске приложения.
///
/// Неподтверждённая запись в журнале означает, что прошлый сеанс не дожил до
/// подтверждения: настройка помечается плохой, карта возвращается в штатное.
pub fn boot_check() -> BootReport {
    let mut j = load();
    let Some(p) = j.pending.clone() else {
        return BootReport {
            recovered: false,
            rebooted: false,
            evidence: None,
            message: "Незавершённых попыток разгона нет.".into(),
        };
    };

    let rebooted = boot_id() != p.boot_id;
    let evidence = if rebooted { unclean_since(p.applied_at) } else { None };

    let reason = if rebooted {
        match &evidence {
            Some(e) => format!("система аварийно перезагрузилась во время проверки ({e})"),
            None => "система перезагрузилась до подтверждения настройки".to_string(),
        }
    } else {
        "приложение завершилось, не подтвердив настройку".to_string()
    };

    // После перезагрузки смещения уже слетели сами; при падении приложения — нет.
    let revert = match nvapi::reset_all() {
        Ok(()) => "Карта приведена к штатным значениям.".to_string(),
        Err(e) => format!("Сброс к штатным значениям не удался: {e}"),
    };

    j.rejected.push(Rejected { candidate: p.candidate, reason: reason.clone(), at: now() });
    j.pending = None;
    j.history.push(Event { at: now(), kind: "recover".into(), text: format!("Восстановление после сбоя: {reason}. {revert}") });
    let _ = store(&j);

    BootReport {
        recovered: true,
        rebooted,
        evidence,
        message: format!(
            "Найдена неподтверждённая настройка (ядро {:+} МГц, память {:+} МГц, мощность {:.0} %): {reason}. {revert} Эта комбинация больше не будет предложена.",
            p.candidate.core_offset_mhz, p.candidate.mem_offset_mhz, p.candidate.power_percent
        ),
    }
}
