//! Автоподбор общего смещения напряжения процессора.
//!
//! Ищет, насколько глубоко можно опустить напряжение, не теряя стабильности.
//! Смещение общее, а не по ядрам: пооядерный Curve Optimizer через ACPI-интерфейс
//! платы недоступен, для него нужен BIOS.
//!
//! # Почему это безопасно
//!
//! Смещение волатильно — перезагрузка снимает его так же, как смещения NVAPI.
//! Значит, зависание лечится кнопкой Reset, а не восстановлением системы. На этом
//! построена вся конструкция: намерение пишется на диск **до** применения, и если
//! приложение или машина не дожили до вердикта, при следующем запуске это видно.
//!
//! # Как обнаруживается нестабильность
//!
//! Слишком глубокий андервольт редко роняет систему сразу. Гораздо чаще он даёт
//! неверные результаты вычислений, которые обычный «поработало и не упало» не ловит.
//! Поэтому шаг считается провальным по любому из четырёх признаков:
//!
//! 1. расхождения контрольных сумм в тесте процессора;
//! 2. испорченные ячейки в тесте памяти — контроллер памяти питается от того же SOC;
//! 3. записи WHEA за время шага — железо само сообщило об ошибке;
//! 4. неподтверждённая запись в журнале при следующем запуске — значит, был вылет.

use crate::{bench, ps, voltage};
use serde::{Deserialize, Serialize};

/// Шаг подбора. Мельче нет смысла: разброс от нагрева между прогонами сопоставим.
const STEP_MV: i32 = 25;
/// Глубже приложение не опускается независимо от результатов.
const FLOOR_MV: i32 = -150;
/// Отступ от первого неустойчивого значения, который остаётся в итоге.
///
/// Два шага, а не один: настройка, устойчивая на прохладной машине зимой, может
/// перестать быть таковой летом или при просевшем питании. Запас нужен на условия,
/// которых во время подбора не было.
const SAFETY_STEPS: i32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PendingStep {
    pub offset_mv: i32,
    pub started_at: i64,
    pub boot_id: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TuneEvent {
    pub at: i64,
    pub offset_mv: i32,
    pub outcome: String,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TuneState {
    /// Идентификатор регулятора смещения напряжения ядра.
    pub regulator_id: Option<i32>,
    /// Самое глубокое смещение, прошедшее проверку.
    pub deepest_stable_mv: i32,
    /// Первое смещение, на котором проверка не прошла.
    pub first_unstable_mv: Option<i32>,
    /// Шаг, применённый к железу, но ещё не получивший вердикта.
    pub pending: Option<PendingStep>,
    /// Итоговое смещение с запасом.
    pub recommended_mv: Option<i32>,
    pub finished: bool,
    pub history: Vec<TuneEvent>,
}

fn path() -> std::path::PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.cpuoc.json");
    p
}

pub fn load() -> TuneState {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Запись с принудительным сбросом на диск.
///
/// Без этого журнал мог бы остаться в кеше ОС и потеряться при зависании — ровно
/// в том случае, ради которого он и ведётся.
fn store(s: &TuneState) -> Result<(), String> {
    use std::io::Write;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let p = path();
    let tmp = p.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

/// Момент загрузки ОС: одинаков внутри одной загрузки, меняется после перезагрузки.
fn boot_id() -> i64 {
    let uptime = (unsafe { GetTickCount64() } / 1000) as i64;
    let raw = now() - uptime;
    raw - raw.rem_euclid(60)
}

/// Записи WHEA за время шага — железо само сообщило об ошибке.
fn whea_since(ts: i64) -> Vec<String> {
    let script = format!(
        "$t=[DateTimeOffset]::FromUnixTimeSeconds({ts}).LocalDateTime; \
         $e=Get-WinEvent -FilterHashtable @{{ LogName='System'; ProviderName='Microsoft-Windows-WHEA-Logger'; StartTime=$t }} \
            -MaxEvents 5 -ErrorAction SilentlyContinue; \
         if($e){{ ($e | ForEach-Object {{ \"WHEA $($_.Id)\" }}) -join '; ' }}"
    );
    ps::run_ps(&script)
        .ok()
        .map(|o| o.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// Находит регулятор смещения напряжения ядра среди тех, что отдала плата.
fn find_regulator() -> Result<i32, String> {
    let state = voltage::read_state();
    state
        .items
        .iter()
        .find(|i| i.adjustable && i.is_offset && !i.name.to_lowercase().contains("soc"))
        .map(|i| i.id)
        .ok_or_else(|| {
            "Регулятор смещения напряжения ядра на этой плате не найден. Автоподбор доступен \
             только там, где плата отдаёт его через ACPI-интерфейс."
                .to_string()
        })
}

#[derive(Serialize, Clone, Debug)]
pub struct StepResult {
    pub offset_mv: i32,
    pub passed: bool,
    pub detail: String,
    pub state: TuneState,
}

/// Начинает подбор заново.
pub fn start() -> Result<TuneState, String> {
    let id = find_regulator()?;
    voltage::set_offset(id, 0)?;
    let s = TuneState {
        regulator_id: Some(id),
        deepest_stable_mv: 0,
        first_unstable_mv: None,
        pending: None,
        recommended_mv: None,
        finished: false,
        history: vec![TuneEvent {
            at: now(),
            offset_mv: 0,
            outcome: "start".into(),
            detail: "Подбор начат от нулевого смещения.".into(),
        }],
    };
    store(&s)?;
    Ok(s)
}

/// Один шаг подбора целиком: применить, проверить, вынести вердикт.
///
/// Тесты процессора и памяти нативные, поэтому шаг выполняется одной командой и не
/// требует пересылки промежуточных результатов в интерфейс и обратно.
pub fn step(seconds: u64, memory_mb: usize) -> Result<StepResult, String> {
    if let Some(r) = crate::anticheat::writes_blocked() {
        return Err(r);
    }
    let mut s = load();
    let id = s.regulator_id.ok_or_else(|| "Подбор не начат.".to_string())?;
    if s.finished {
        return Err("Подбор уже завершён. Начните заново, если нужно повторить.".into());
    }
    if s.pending.is_some() {
        return Err("Предыдущий шаг не получил вердикта. Перезапустите подбор.".into());
    }

    let next = s.deepest_stable_mv - STEP_MV;
    if next < FLOOR_MV {
        s.finished = true;
        s.recommended_mv = Some(s.deepest_stable_mv);
        s.history.push(TuneEvent {
            at: now(),
            offset_mv: s.deepest_stable_mv,
            outcome: "floor".into(),
            detail: format!("Достигнута нижняя граница {FLOOR_MV} мВ, глубже приложение не идёт."),
        });
        store(&s)?;
        return Ok(StepResult {
            offset_mv: s.deepest_stable_mv,
            passed: true,
            detail: "Дошли до нижней границы. Итог — последнее устойчивое значение.".into(),
            state: s,
        });
    }

    // Намерение на диск раньше, чем оно попадёт в железо.
    let started = now();
    s.pending = Some(PendingStep { offset_mv: next, started_at: started, boot_id: boot_id() });
    store(&s)?;

    let applied = voltage::set_offset(id, next).map_err(|e| {
        let mut back = load();
        back.pending = None;
        let _ = store(&back);
        e
    })?;

    // Процессор и память проверяются вместе: контроллер памяти питается от того же
    // узла, и слишком глубокий андервольт проявляется в нём не реже, чем в ядрах.
    let cpu = bench::cpu_stress(seconds, 0);
    let mem = bench::memory_test(memory_mb, seconds.max(5) / 2);
    let whea = whea_since(started);

    let failure = if cpu.mismatches > 0 {
        Some(format!("процессор дал {} расхождений контрольных сумм", cpu.mismatches))
    } else if mem.mismatches > 0 {
        Some(format!("в памяти испорчено ячеек: {}", mem.mismatches))
    } else if !whea.is_empty() {
        Some(format!("железо сообщило об ошибках: {}", whea.join(", ")))
    } else {
        None
    };

    s = load();
    s.pending = None;

    let result = match failure {
        None => {
            s.deepest_stable_mv = applied;
            s.history.push(TuneEvent {
                at: now(),
                offset_mv: applied,
                outcome: "pass".into(),
                detail: format!(
                    "{applied} мВ: {} проходов процессора и {} проходов памяти без расхождений",
                    cpu.passes, mem.passes
                ),
            });
            StepResult {
                offset_mv: applied,
                passed: true,
                detail: format!("{applied} мВ выдержало проверку."),
                state: s.clone(),
            }
        }
        Some(why) => {
            s.first_unstable_mv = Some(applied);
            // Отступаем на два шага: запас нужен на жару и просадки питания,
            // которых во время подбора могло не быть.
            let safe = (applied + STEP_MV * SAFETY_STEPS).min(0);
            s.recommended_mv = Some(safe);
            s.finished = true;
            s.history.push(TuneEvent {
                at: now(),
                offset_mv: applied,
                outcome: "fail".into(),
                detail: format!("{applied} мВ не выдержало: {why}"),
            });
            voltage::set_offset(id, safe)?;
            StepResult {
                offset_mv: applied,
                passed: false,
                detail: format!(
                    "{applied} мВ не выдержало ({why}). Оставлено {safe} мВ — на два шага выше, \
                     чтобы был запас на жару и просадки питания."
                ),
                state: s.clone(),
            }
        }
    };
    store(&s)?;
    Ok(result)
}

/// Прерывает подбор и возвращает последнее устойчивое значение.
pub fn stop() -> Result<TuneState, String> {
    let mut s = load();
    let id = s.regulator_id.ok_or_else(|| "Подбор не начат.".to_string())?;
    let back = s.recommended_mv.unwrap_or(s.deepest_stable_mv);
    voltage::set_offset(id, back)?;
    s.pending = None;
    s.finished = true;
    s.recommended_mv = Some(back);
    s.history.push(TuneEvent {
        at: now(),
        offset_mv: back,
        outcome: "stop".into(),
        detail: format!("Подбор остановлен вручную, оставлено {back} мВ."),
    });
    store(&s)?;
    Ok(s)
}

/// Проверка при запуске: неподтверждённый шаг означает, что прошлый сеанс не дожил
/// до вердикта.
///
/// Смещение к этому моменту уже снято перезагрузкой, поэтому трогать железо не нужно —
/// нужно запомнить, что на этом значении машина не устояла, и не идти туда снова.
pub fn boot_check() -> Option<String> {
    let mut s = load();
    let p = s.pending.take()?;
    let rebooted = boot_id() != p.boot_id;
    let why = if rebooted {
        "система перезагрузилась, не дождавшись вердикта"
    } else {
        "приложение завершилось, не дождавшись вердикта"
    };

    s.first_unstable_mv = Some(p.offset_mv);
    let safe = (p.offset_mv + STEP_MV * SAFETY_STEPS).min(0);
    s.recommended_mv = Some(safe);
    s.finished = true;
    s.history.push(TuneEvent {
        at: now(),
        offset_mv: p.offset_mv,
        outcome: "crash".into(),
        detail: format!("{} мВ: {why}", p.offset_mv),
    });
    let _ = store(&s);

    Some(format!(
        "Автоподбор напряжения: на {} мВ {why}. Это значение больше не предлагается, \
         рекомендованным осталось {safe} мВ. Смещение снято перезагрузкой само.",
        p.offset_mv
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_margin_is_two_steps_and_never_positive() {
        // Провал на -100 мВ должен оставить -50 мВ.
        let unstable = -100;
        let safe = (unstable + STEP_MV * SAFETY_STEPS).min(0);
        assert_eq!(safe, -50);

        // Провал на первом же шаге не должен приводить к положительному смещению:
        // поднимать напряжение выше штатного приложение не имеет права.
        let safe_first = (-STEP_MV + STEP_MV * SAFETY_STEPS).min(0);
        assert_eq!(safe_first, 0, "запас не должен уводить смещение в плюс");
    }

    /// Граница включительная: до −150 мВ дойти можно, глубже — нет.
    #[test]
    fn floor_is_inclusive() {
        // С предпоследней ступени шаг попадает ровно в границу и должен быть разрешён.
        let from = FLOOR_MV + STEP_MV;
        assert!(from - STEP_MV >= FLOOR_MV, "до самой границы дойти можно");
        assert_eq!(from - STEP_MV, FLOOR_MV);

        // С границы следующий шаг уводит ниже, и подбор обязан остановиться.
        assert!(FLOOR_MV - STEP_MV < FLOOR_MV, "ниже границы шагать нельзя");
    }
}
