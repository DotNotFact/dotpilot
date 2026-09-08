//! Долгие наблюдения: деградация охлаждения и расход электричества.
//!
//! Всё остальное в приложении отвечает на вопрос «что сейчас». Здесь — «что
//! меняется со временем», а это требует накопленной истории и честности насчёт
//! того, что данных пока мало.
//!
//! # Как определяется, что пора чистить от пыли
//!
//! Наивный признак — «обороты вентилятора выросли» — не работает. При фиксированной
//! кривой обороты жёстко следуют за температурой, поэтому на одной и той же
//! температуре они одинаковы и год назад, и сегодня.
//!
//! Правильный признак — **температура при той же потребляемой мощности**. Мощность
//! это тепловыделение: если карта отдаёт те же 200 Вт, а нагревается сильнее, значит,
//! тепло хуже уходит. Так видно и пыль, и высохшую термопасту, и остановившийся
//! вентилятор в корпусе.
//!
//! Поэтому образцы группируются по мощности, и внутри каждой группы сравнивается
//! медианная температура старых образцов с новыми. Медиана, а не среднее: одиночный
//! выброс от секундного пика нагрузки не должен сдвигать вывод.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

/// Как часто записывается образец. Чаще нет смысла: тренд измеряется неделями.
const RECORD_EVERY_SECS: i64 = 300;
/// Сколько образцов храним: при записи раз в пять минут это около десяти суток
/// непрерывной работы, а с перерывами — заметно дольше.
const MAX_SAMPLES: usize = 3000;
/// Ниже этой мощности видеокарта простаивает, и температура определяется не ею.
const GPU_LOAD_WATTS: f32 = 60.0;
/// Ширина группы по мощности: внутри неё образцы считаются сравнимыми.
const POWER_BUCKET_W: f32 = 25.0;
/// Меньше этого числа образцов в группе — вывод не делаем.
const MIN_PER_SIDE: usize = 12;
/// Меньше этого срока наблюдений тренд не показываем: суточные колебания
/// комнатной температуры дадут ложный сигнал.
const MIN_SPAN_DAYS: f64 = 3.0;
/// Изменение меньше этого считаем шумом, а не трендом.
const NOISE_C: f32 = 1.5;

static LAST_RECORD: AtomicI64 = AtomicI64::new(0);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sample {
    pub at: i64,
    pub gpu_temp_c: i32,
    pub gpu_power_w: f32,
    pub gpu_util: f32,
    pub gpu_fan_pct: f32,
    /// Температура процессора с датчика платы; ноль означает «не прочитана».
    pub cpu_temp_c: u8,
    pub cpu_load: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct History {
    pub samples: Vec<Sample>,
}

fn path() -> std::path::PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.trends.json");
    p
}

pub fn load() -> History {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(h: &History) -> Result<(), String> {
    let text = serde_json::to_string(h).map_err(|e| e.to_string())?;
    std::fs::write(path(), text).map_err(|e| e.to_string())
}

/// Записывает образец, если с прошлого прошло достаточно времени.
///
/// Вызывается из цикла сборщика на каждой итерации; внутренний счётчик не даёт
/// трогать диск чаще, чем раз в `RECORD_EVERY_SECS`.
pub fn maybe_record(gpu: &crate::sysmon::GpuInfo, cpu_load: f32) {
    if !gpu.available {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let last = LAST_RECORD.load(Ordering::Relaxed);
    if last != 0 && now - last < RECORD_EVERY_SECS {
        return;
    }
    LAST_RECORD.store(now, Ordering::Relaxed);

    // Температуру процессора берём с датчика платы: одного вызова раз в пять
    // минут интерфейс не замечает.
    let board = crate::fanctl::read_state();
    let cpu_temp = board
        .fans
        .first()
        .map(|f| f.target_sensor)
        .and_then(|id| board.sensors.iter().find(|s| s.id == id && s.connected))
        .map(|s| s.celsius)
        .unwrap_or(0);

    let mut h = load();
    h.samples.push(Sample {
        at: now,
        gpu_temp_c: gpu.temp_c as i32,
        gpu_power_w: gpu.power_w,
        gpu_util: gpu.util_pct,
        gpu_fan_pct: gpu.fan_pct,
        cpu_temp_c: cpu_temp,
        cpu_load,
    });
    let len = h.samples.len();
    if len > MAX_SAMPLES {
        h.samples.drain(0..len - MAX_SAMPLES);
    }
    let _ = store(&h);
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(v[v.len() / 2])
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct CoolingTrend {
    pub enough_data: bool,
    /// Насколько выросла температура видеокарты при той же мощности, в градусах.
    pub gpu_delta_c: Option<f32>,
    /// То же для процессора при сравнимой загрузке.
    pub cpu_delta_c: Option<f32>,
    /// Сколько образцов участвовало в сравнении.
    pub compared: usize,
    pub verdict: String,
}

/// Сравнивает старую и новую половины наблюдений внутри одинаковых условий.
fn cooling_trend(samples: &[Sample], span_days: f64) -> CoolingTrend {
    if span_days < MIN_SPAN_DAYS {
        return CoolingTrend {
            verdict: format!(
                "Наблюдений пока мало: нужно хотя бы {MIN_SPAN_DAYS:.0} суток, чтобы отличить \
                 деградацию охлаждения от суточных колебаний температуры в комнате."
            ),
            ..Default::default()
        };
    }

    let mid = samples.len() / 2;
    let (old, new) = samples.split_at(mid);

    // Видеокарта: сравниваем внутри одинаковых групп по мощности.
    let mut gpu_deltas: Vec<f32> = Vec::new();
    let mut compared = 0usize;
    let loaded = |s: &Sample| s.gpu_power_w >= GPU_LOAD_WATTS;
    let max_w = samples.iter().map(|s| s.gpu_power_w).fold(0.0f32, f32::max);
    let mut bucket = GPU_LOAD_WATTS;
    while bucket < max_w {
        let hi = bucket + POWER_BUCKET_W;
        let pick = |set: &[Sample]| -> Vec<f32> {
            set.iter()
                .filter(|s| loaded(s) && s.gpu_power_w >= bucket && s.gpu_power_w < hi)
                .map(|s| s.gpu_temp_c as f32)
                .collect()
        };
        let a = pick(old);
        let b = pick(new);
        if a.len() >= MIN_PER_SIDE && b.len() >= MIN_PER_SIDE {
            if let (Some(ma), Some(mb)) = (median(a.clone()), median(b.clone())) {
                gpu_deltas.push(mb - ma);
                compared += a.len() + b.len();
            }
        }
        bucket = hi;
    }

    // Процессор: группировка по загрузке, логика та же.
    let cpu_delta = {
        let pick = |set: &[Sample]| -> Vec<f32> {
            set.iter()
                .filter(|s| s.cpu_temp_c > 0 && s.cpu_load >= 20.0)
                .map(|s| s.cpu_temp_c as f32)
                .collect()
        };
        let a = pick(old);
        let b = pick(new);
        if a.len() >= MIN_PER_SIDE && b.len() >= MIN_PER_SIDE {
            match (median(a), median(b)) {
                (Some(ma), Some(mb)) => Some(mb - ma),
                _ => None,
            }
        } else {
            None
        }
    };

    let gpu_delta = median(gpu_deltas.clone());
    if gpu_delta.is_none() && cpu_delta.is_none() {
        return CoolingTrend {
            enough_data: false,
            verdict: "Сравнимых условий пока не набралось: нужны наблюдения под нагрузкой, \
                      а не только в простое."
                .into(),
            ..Default::default()
        };
    }

    let worst = gpu_delta.unwrap_or(0.0).max(cpu_delta.unwrap_or(0.0));
    let verdict = if worst > 5.0 {
        "При той же мощности железо стало заметно горячее. Обычно это пыль в радиаторе \
         или высохшая термопаста."
            .to_string()
    } else if worst > NOISE_C {
        "Небольшой рост температуры при той же мощности. Пока не повод разбирать, но стоит \
         посмотреть на пыль."
            .to_string()
    } else if worst < -NOISE_C {
        "Стало холоднее при той же мощности — похоже, охлаждение улучшили.".to_string()
    } else {
        "Температура при той же мощности не изменилась: охлаждение работает как раньше."
            .to_string()
    };

    CoolingTrend { enough_data: true, gpu_delta_c: gpu_delta, cpu_delta_c: cpu_delta, compared, verdict }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct EnergyReport {
    /// Киловатт-часы, посчитанные по фактическим замерам мощности видеокарты.
    pub gpu_kwh: f64,
    pub gpu_cost: f64,
    pub tariff: f64,
    /// За какой срок посчитано.
    pub span_days: f64,
    /// Средняя мощность видеокарты за период наблюдений.
    pub avg_watts: f64,
    pub note: String,
}

/// Считает потреблённую энергию по фактическим замерам.
///
/// Учитывается **только видеокарта**: её мощность приходит с датчика. Мощность
/// процессора и остальной системы без дополнительного оборудования не измеряется,
/// и подставлять сюда оценку значило бы выдавать догадку за замер.
fn energy(samples: &[Sample], tariff: f64) -> EnergyReport {
    if samples.len() < 2 {
        return EnergyReport {
            tariff,
            note: "Данных пока нет.".into(),
            ..Default::default()
        };
    }
    let mut wh = 0.0f64;
    for pair in samples.windows(2) {
        let dt = (pair[1].at - pair[0].at) as f64;
        // Пропуск больше получаса означает, что приложение не работало.
        if dt <= 0.0 || dt > 1800.0 {
            continue;
        }
        let avg_w = (pair[0].gpu_power_w as f64 + pair[1].gpu_power_w as f64) / 2.0;
        wh += avg_w * dt / 3600.0;
    }
    let span_days = (samples.last().unwrap().at - samples[0].at) as f64 / 86400.0;
    let avg_watts = samples.iter().map(|s| s.gpu_power_w as f64).sum::<f64>() / samples.len() as f64;
    let kwh = wh / 1000.0;
    EnergyReport {
        gpu_kwh: kwh,
        gpu_cost: kwh * tariff,
        tariff,
        span_days,
        avg_watts,
        note: "Считается только видеокарта: её мощность приходит с датчика. Процессор и \
               остальная система без внешнего измерителя не считаются, поэтому оценка их \
               потребления здесь не подставляется."
            .into(),
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct TrendReport {
    pub samples: usize,
    pub span_days: f64,
    pub cooling: CoolingTrend,
    pub energy: EnergyReport,
}

pub fn report(tariff: f64) -> TrendReport {
    let h = load();
    let span_days = if h.samples.len() >= 2 {
        (h.samples.last().unwrap().at - h.samples[0].at) as f64 / 86400.0
    } else {
        0.0
    };
    TrendReport {
        samples: h.samples.len(),
        span_days,
        cooling: cooling_trend(&h.samples, span_days),
        energy: energy(&h.samples, tariff),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: i64, temp: i32, watts: f32) -> Sample {
        Sample { at, gpu_temp_c: temp, gpu_power_w: watts, gpu_util: 80.0, gpu_fan_pct: 50.0, cpu_temp_c: 60, cpu_load: 50.0 }
    }

    #[test]
    fn short_history_refuses_to_guess() {
        let s: Vec<Sample> = (0..50).map(|i| sample(i * 300, 70, 200.0)).collect();
        let t = cooling_trend(&s, 0.5);
        assert!(!t.enough_data, "на полусуточной истории тренда быть не должно");
        assert!(t.verdict.contains("суток"));
    }

    #[test]
    fn detects_rising_temperature_at_same_power() {
        // Старая половина: 65 °C при 200 Вт. Новая: 73 °C при тех же 200 Вт.
        let mut s: Vec<Sample> = Vec::new();
        for i in 0..40 {
            s.push(sample(i * 300, 65, 200.0));
        }
        for i in 40..80 {
            s.push(sample(i * 300, 73, 200.0));
        }
        let t = cooling_trend(&s, 10.0);
        assert!(t.enough_data);
        assert_eq!(t.gpu_delta_c, Some(8.0), "рост должен быть виден как +8 °C");
        assert!(t.verdict.contains("горячее"));
    }

    #[test]
    fn ignores_noise() {
        let mut s: Vec<Sample> = Vec::new();
        for i in 0..40 {
            s.push(sample(i * 300, 70, 200.0));
        }
        for i in 40..80 {
            s.push(sample(i * 300, 71, 200.0));
        }
        let t = cooling_trend(&s, 10.0);
        assert!(t.verdict.contains("не изменилась"), "разница в градус — это шум");
    }

    #[test]
    fn energy_skips_gaps_when_app_was_closed() {
        // Час работы при 100 Вт, потом сутки простоя приложения, потом ещё час.
        let mut s: Vec<Sample> = Vec::new();
        for i in 0..12 {
            s.push(sample(i * 300, 70, 100.0));
        }
        let jump = 12 * 300 + 86400;
        for i in 0..12 {
            s.push(sample(jump + i * 300, 70, 100.0));
        }
        let e = energy(&s, 5.0);
        // Два отрезка по 55 минут при 100 Вт — около 0.18 кВт·ч, а не 2.4 за сутки.
        assert!(e.gpu_kwh < 0.25, "перерыв в работе не должен считаться потреблением: {}", e.gpu_kwh);
        assert!(e.gpu_kwh > 0.15, "фактические отрезки должны быть учтены: {}", e.gpu_kwh);
    }
}
