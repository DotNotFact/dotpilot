//! Бюджет шума: потолок оборотов и автоматическое снижение мощности под него.
//!
//! Шум приложению измерить нечем — микрофона у него нет. Переводить проценты
//! оборотов в децибелы значило бы выдумывать числа, поэтому бюджет задаётся в том,
//! что действительно измеряется: в оборотах вентилятора.
//!
//! Смысл в замкнутой петле. Пользователь говорит «не громче половины оборотов»,
//! приложение ограничивает вентилятор и смотрит, что получилось с температурой.
//! Если она уходит выше потолка, снижается лимит мощности — карта меньше греется,
//! и заданных оборотов снова хватает. Когда запас появляется, мощность возвращается.
//!
//! # Почему потолок сделан именно так
//!
//! NVAPI умеет два режима: автоматика драйвера или фиксированный уровень. Режима
//! «не выше X» в нём нет. Поэтому под нагрузкой уровень фиксируется на потолке, а в
//! простое возвращается автоматика: под нагрузкой кривая драйвера всё равно ушла бы
//! выше потолка, а в простое она тише него. Итог совпадает с обещанием «не громче X»,
//! и при этом в простое карта не шумит на потолке впустую.
//!
//! # Кто владеет лимитом мощности
//!
//! Пока бюджет включён, лимитом мощности распоряжается эта петля. Значение мощности
//! из профиля приложения при этом не действует — смещения частот действуют по-прежнему.
//! Правило простое и объявлено в интерфейсе: ограничение по шуму сильнее пожелания
//! по мощности, иначе два механизма боролись бы за один регулятор.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

/// Реже, чем раз в столько секунд, решения не принимаются: у нагрева большая
/// инерция, и частая подстройка приводила бы к раскачке вместо стабилизации.
const DECIDE_EVERY_SECS: i64 = 45;
/// Ниже этой загрузки считаем, что карта простаивает: температура в простое
/// ничего не говорит об эффективности охлаждения.
const LOAD_THRESHOLD_PCT: u32 = 30;
/// Зона нечувствительности вокруг цели, чтобы не дёргать лимит туда-сюда.
const HYSTERESIS_C: i32 = 3;
/// Шаг изменения лимита мощности.
const POWER_STEP_PCT: f32 = 5.0;

static LAST_DECISION: AtomicI64 = AtomicI64::new(0);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoiseBudget {
    pub enabled: bool,
    /// Потолок оборотов вентилятора видеокарты в процентах.
    pub max_fan_percent: u32,
    /// Температура, выше которой петля начинает снижать мощность.
    pub target_temp_c: i32,
    /// Ниже этого лимит мощности не опускается.
    pub floor_power_percent: f32,
    /// Отсюда петля начинает и сюда возвращается, когда появляется запас.
    pub ceiling_power_percent: f32,
    /// Что петля выставила последним решением.
    pub current_power_percent: f32,
    /// Последнее объяснение, почему сделано именно так.
    pub last_reason: String,
    pub last_change: Option<i64>,
}

impl Default for NoiseBudget {
    fn default() -> Self {
        Self {
            enabled: false,
            max_fan_percent: 55,
            target_temp_c: 75,
            floor_power_percent: 70.0,
            ceiling_power_percent: 100.0,
            current_power_percent: 100.0,
            last_reason: "Бюджет шума выключен.".into(),
            last_change: None,
        }
    }
}

fn path() -> std::path::PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.noise.json");
    p
}

pub fn load() -> NoiseBudget {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn store(b: &NoiseBudget) -> Result<(), String> {
    let text = serde_json::to_string_pretty(b).map_err(|e| e.to_string())?;
    std::fs::write(path(), text).map_err(|e| e.to_string())
}

/// Решение петли — вынесено отдельно от железа, чтобы проверяться тестами.
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// Ничего не менять.
    Hold(&'static str),
    /// Выставить новый лимит мощности.
    SetPower(f32, String),
}

/// Чистая часть петли: по показаниям решает, что делать с лимитом мощности.
///
/// Правило простое: пока карта под нагрузкой и горячее цели — снижаем; когда есть
/// запас и лимит был снижен — возвращаем. В простое ничего не трогаем, потому что
/// температура в простое не говорит о том, хватает ли оборотов под нагрузкой.
pub fn decide(b: &NoiseBudget, temp_c: i32, util_pct: u32) -> Decision {
    if !b.enabled {
        return Decision::Hold("бюджет выключен");
    }
    if util_pct < LOAD_THRESHOLD_PCT {
        return Decision::Hold("карта простаивает, судить о температуре рано");
    }

    if temp_c > b.target_temp_c + HYSTERESIS_C {
        let next = (b.current_power_percent - POWER_STEP_PCT).max(b.floor_power_percent);
        if next < b.current_power_percent {
            return Decision::SetPower(
                next,
                format!(
                    "{temp_c} °C при потолке {} % оборотов — выше цели {} °C, снижаю мощность до {next:.0} %",
                    b.max_fan_percent, b.target_temp_c
                ),
            );
        }
        return Decision::Hold("мощность уже на нижней границе, ниже не опускаемся");
    }

    if temp_c < b.target_temp_c - HYSTERESIS_C && b.current_power_percent < b.ceiling_power_percent {
        let next = (b.current_power_percent + POWER_STEP_PCT).min(b.ceiling_power_percent);
        return Decision::SetPower(
            next,
            format!(
                "{temp_c} °C — есть запас до цели {} °C, возвращаю мощность до {next:.0} %",
                b.target_temp_c
            ),
        );
    }

    Decision::Hold("температура в пределах цели, менять нечего")
}

/// Шаг петли: вызывается сборщиком, сам решает, пора ли принимать решение.
///
/// Возвращает описание изменения, если оно было; в обычном случае молчит.
pub fn tick(temp_c: i32, util_pct: u32) -> Option<String> {
    let mut b = load();
    if !b.enabled {
        return None;
    }
    if crate::anticheat::writes_blocked().is_some() {
        return None;
    }

    // Потолок оборотов поддерживается на каждом шаге, а не только при решении:
    // драйвер мог вернуть автоматику после смены режима или перезапуска.
    let want_manual = util_pct >= LOAD_THRESHOLD_PCT;
    let _ = crate::nvapi::set_fan_level(if want_manual { Some(b.max_fan_percent) } else { None });

    let now = chrono::Utc::now().timestamp();
    if now - LAST_DECISION.load(Ordering::Relaxed) < DECIDE_EVERY_SECS {
        return None;
    }
    LAST_DECISION.store(now, Ordering::Relaxed);

    match decide(&b, temp_c, util_pct) {
        Decision::Hold(_) => None,
        Decision::SetPower(next, reason) => match crate::nvapi::set_power_limit_percent(next) {
            Ok(applied) => {
                b.current_power_percent = applied;
                b.last_reason = reason.clone();
                b.last_change = Some(now);
                let _ = store(&b);
                Some(reason)
            }
            Err(e) => {
                b.last_reason = format!("не удалось изменить лимит мощности: {e}");
                let _ = store(&b);
                Some(b.last_reason.clone())
            }
        },
    }
}

/// Включает или выключает бюджет. При выключении возвращает всё как было.
pub fn set_enabled(on: bool) -> Result<NoiseBudget, String> {
    let mut b = load();
    b.enabled = on;
    if on {
        b.current_power_percent = b.ceiling_power_percent;
        b.last_reason = "Бюджет включён, наблюдаю за температурой под нагрузкой.".into();
    } else {
        // Возвращаем автоматику вентилятора и потолок мощности: выключенный бюджет
        // не должен оставлять после себя следов.
        let _ = crate::nvapi::set_fan_level(None);
        let _ = crate::nvapi::set_power_limit_percent(b.ceiling_power_percent);
        b.current_power_percent = b.ceiling_power_percent;
        b.last_reason = "Бюджет выключен, обороты и мощность возвращены.".into();
    }
    store(&b)?;
    Ok(b)
}

pub fn update(max_fan: u32, target_temp: i32, floor: f32, ceiling: f32) -> Result<NoiseBudget, String> {
    let mut b = load();
    // Границы удерживаются в разумном: вентилятор ниже трети оборотов на нагрузке
    // бесполезен, а цель выше 85 °C сводит смысл бюджета на нет.
    b.max_fan_percent = max_fan.clamp(30, 100);
    b.target_temp_c = target_temp.clamp(60, 85);
    b.ceiling_power_percent = ceiling.clamp(50.0, 110.0);
    b.floor_power_percent = floor.clamp(50.0, b.ceiling_power_percent);
    if b.current_power_percent > b.ceiling_power_percent {
        b.current_power_percent = b.ceiling_power_percent;
    }
    store(&b)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> NoiseBudget {
        NoiseBudget { enabled: true, ..Default::default() }
    }

    #[test]
    fn idle_is_not_a_verdict() {
        // В простое температура низкая независимо от того, хватает ли оборотов.
        let d = decide(&budget(), 40, 5);
        assert!(matches!(d, Decision::Hold(_)), "в простое решений принимать нельзя: {d:?}");
    }

    #[test]
    fn hot_under_load_lowers_power() {
        let d = decide(&budget(), 82, 90);
        match d {
            Decision::SetPower(v, why) => {
                assert_eq!(v, 95.0, "шаг снижения должен быть 5 %");
                assert!(why.contains("снижаю"), "объяснение должно называть действие: {why}");
            }
            other => panic!("ожидалось снижение мощности, получено {other:?}"),
        }
    }

    #[test]
    fn does_not_go_below_floor() {
        let b = NoiseBudget { current_power_percent: 70.0, ..budget() };
        assert!(matches!(decide(&b, 90, 90), Decision::Hold(_)), "ниже нижней границы опускаться нельзя");
    }

    #[test]
    fn restores_power_when_margin_appears() {
        let b = NoiseBudget { current_power_percent: 80.0, ..budget() };
        match decide(&b, 68, 90) {
            Decision::SetPower(v, why) => {
                assert_eq!(v, 85.0);
                assert!(why.contains("возвращаю"), "объяснение должно называть действие: {why}");
            }
            other => panic!("ожидался возврат мощности, получено {other:?}"),
        }
    }

    #[test]
    fn does_not_exceed_ceiling() {
        let b = NoiseBudget { current_power_percent: 100.0, ..budget() };
        assert!(matches!(decide(&b, 60, 90), Decision::Hold(_)), "выше потолка подниматься нельзя");
    }

    #[test]
    fn hysteresis_prevents_flapping() {
        // Ровно на цели и в пределах зоны нечувствительности решений быть не должно.
        for t in [73, 75, 77] {
            assert!(
                matches!(decide(&budget(), t, 90), Decision::Hold(_)),
                "при {t} °C рядом с целью 75 °C менять ничего нельзя"
            );
        }
    }
}
