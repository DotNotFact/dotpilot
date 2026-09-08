//! Замер того, что настраивается в BIOS: память, эффективный буст, версия прошивки.
//!
//! Реальные рычаги Zen 4 — EXPO, PBO и Curve Optimizer — живут в BIOS, и из Windows
//! их не переключить без кернел-драйвера. Поэтому приложение берёт на себя то, что
//! умеет делать честно: **измерить до и после**. Пользователь меняет значения в BIOS,
//! а приложение говорит, стало ли лучше — и подтверждает это числами, а не ощущением.
//!
//! Всё здесь читается без драйверов: WMI и счётчики производительности.
//!
//! Температуру процессора отдаёт ACPI-интерфейс платы — см. `fanctl`, датчик,
//! к которому привязаны вентиляторы. Здесь её нет только потому, что это замер
//! производительности, а не термометр.

use crate::ps;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemoryModule {
    pub bank: String,
    pub capacity_gb: f32,
    /// Частота, на которой модуль работает сейчас, МТ/с.
    pub configured_mts: u32,
    /// Напряжение питания модуля, мВ.
    pub configured_mv: u32,
    pub part_number: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemoryConfig {
    pub modules: Vec<MemoryModule>,
    pub total_gb: f32,
    /// Включён ли профиль EXPO/XMP.
    pub profile_enabled: bool,
    /// Как именно это определено — чтобы вывод можно было перепроверить.
    pub verdict: String,
}

/// Определяет, работает ли память по профилю или по базовому JEDEC.
///
/// Главный признак — напряжение: JEDEC для DDR5 это 1.1 В, а любой профиль
/// EXPO или XMP поднимает его минимум до 1.25 В. Частота одна ненадёжна, потому
/// что базовые скорости JEDEC растут от поколения к поколению.
fn judge_profile(modules: &[MemoryModule]) -> (bool, String) {
    let Some(m) = modules.first() else {
        return (false, "Модули памяти не определены.".into());
    };
    if m.configured_mv >= 1250 {
        (
            true,
            format!(
                "Профиль включён: {} МТ/с при {:.3} В — это выше базового JEDEC.",
                m.configured_mts,
                m.configured_mv as f32 / 1000.0
            ),
        )
    } else {
        (
            false,
            format!(
                "Профиль выключен: {} МТ/с при {:.3} В — это базовый режим JEDEC. \
                 Включение EXPO в BIOS даст заметную прибавку, особенно в 1% low в играх, \
                 и разгоном в привычном смысле не является — это штатный профиль самих модулей.",
                m.configured_mts,
                m.configured_mv as f32 / 1000.0
            ),
        )
    }
}

pub fn memory_config() -> MemoryConfig {
    let script = "@(Get-CimInstance Win32_PhysicalMemory | Select-Object BankLabel, Capacity, ConfiguredClockSpeed, ConfiguredVoltage, PartNumber) | ConvertTo-Json -Compress -Depth 3";
    let modules: Vec<MemoryModule> = ps::run_ps(script)
        .ok()
        .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
        .map(|v| {
            let arr = if v.is_array() { v } else { serde_json::Value::Array(vec![v]) };
            arr.as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|m| MemoryModule {
                            bank: m.get("BankLabel").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                            capacity_gb: m.get("Capacity").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32
                                / 1024.0
                                / 1024.0
                                / 1024.0,
                            configured_mts: m.get("ConfiguredClockSpeed").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                            configured_mv: m.get("ConfiguredVoltage").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                            part_number: m
                                .get("PartNumber")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let (profile_enabled, verdict) = judge_profile(&modules);
    MemoryConfig {
        total_gb: modules.iter().map(|m| m.capacity_gb).sum(),
        profile_enabled,
        verdict,
        modules,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FirmwareInfo {
    pub board: String,
    pub bios_vendor: String,
    pub bios_version: String,
    pub bios_date: String,
    pub cpu: String,
    pub cpu_base_mhz: u32,
    pub cores: u32,
    pub threads: u32,
}

pub fn firmware_info() -> FirmwareInfo {
    let script = "$b=Get-CimInstance Win32_BaseBoard; $i=Get-CimInstance Win32_BIOS; $c=Get-CimInstance Win32_Processor | Select-Object -First 1; \
        [pscustomobject]@{ board=\"$($b.Manufacturer) $($b.Product)\"; vendor=$i.Manufacturer; version=$i.SMBIOSBIOSVersion; \
        date=$i.ReleaseDate.ToString('yyyy-MM-dd'); cpu=$c.Name.Trim(); base=$c.MaxClockSpeed; cores=$c.NumberOfCores; threads=$c.NumberOfLogicalProcessors } | ConvertTo-Json -Compress";
    let v: serde_json::Value = ps::run_ps(script)
        .ok()
        .and_then(|o| serde_json::from_str(&o).ok())
        .unwrap_or(serde_json::Value::Null);
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    FirmwareInfo {
        board: s("board"),
        bios_vendor: s("vendor"),
        bios_version: s("version"),
        bios_date: s("date"),
        cpu: s("cpu"),
        cpu_base_mhz: n("base"),
        cores: n("cores"),
        threads: n("threads"),
    }
}

/// Фактическая частота процессора в МГц по счётчику производительности.
///
/// `% Processor Performance` показывает, насколько процессор работает выше или ниже
/// паспортной частоты, поэтому по нему видно реальный буст — а он и меняется от PBO
/// и Curve Optimizer. `Win32_Processor.CurrentClockSpeed` для этого не годится:
/// он часто отдаёт паспортное значение независимо от происходящего.
pub fn effective_clock_mhz(base_mhz: u32) -> Option<f32> {
    let out = ps::run_ps(
        "(Get-Counter '\\Processor Information(_Total)\\% Processor Performance' -ErrorAction Stop).CounterSamples[0].CookedValue",
    )
    .ok()?;
    let pct: f32 = out.trim().replace(',', ".").parse().ok()?;
    Some(base_mhz as f32 * pct / 100.0)
}

/// Снимок платформы вместе с результатами встроенных тестов.
///
/// Именно его снимают до и после правки BIOS: сравнение двух таких снимков
/// показывает, что реально изменилось, а не что должно было измениться.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Baseline {
    pub at: i64,
    pub label: String,
    pub firmware: FirmwareInfo,
    pub memory: MemoryConfig,
    /// Частота под полной нагрузкой — показатель работы PBO и Curve Optimizer.
    pub loaded_clock_mhz: Option<f32>,
    /// Проходов цепочки в секунду на всех ядрах: сравнимая оценка производительности.
    pub cpu_passes_per_sec: f64,
    /// Мегабайт в секунду на записи и проверке: отражает эффект EXPO.
    pub memory_mb_per_sec: f64,
    pub cpu_mismatches: u64,
    pub memory_mismatches: u64,
}

/// Снимает замер: гоняет тесты и одновременно смотрит на фактическую частоту.
pub fn measure(label: &str, seconds: u64, memory_mb: usize) -> Baseline {
    let firmware = firmware_info();

    // Частоту снимаем во время нагрузки: в простое она ничего не говорит о PBO.
    let base = firmware.cpu_base_mhz;
    let clock = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        effective_clock_mhz(base)
    });
    let cpu = crate::bench::cpu_stress(seconds, 0);
    let loaded_clock_mhz = clock.join().ok().flatten();

    let mem = crate::bench::memory_test(memory_mb, seconds);
    let mb_per_sec = if mem.seconds > 0.0 {
        // За проход буфер полностью записывается и полностью читается.
        (mem.passes as f64 * memory_mb as f64 * 2.0) / mem.seconds
    } else {
        0.0
    };

    Baseline {
        at: chrono::Utc::now().timestamp(),
        label: label.to_string(),
        firmware,
        memory: memory_config(),
        loaded_clock_mhz,
        cpu_passes_per_sec: if cpu.seconds > 0.0 { cpu.passes as f64 / cpu.seconds } else { 0.0 },
        memory_mb_per_sec: mb_per_sec,
        cpu_mismatches: cpu.mismatches,
        memory_mismatches: mem.mismatches,
    }
}

/// Разница между двумя замерами в понятных числах.
#[derive(Serialize, Clone, Debug)]
pub struct Comparison {
    pub cpu_delta_percent: f64,
    pub memory_delta_percent: f64,
    pub clock_delta_mhz: Option<f32>,
    pub memory_profile_changed: bool,
    pub bios_version_changed: bool,
    pub stable: bool,
    pub summary: String,
}

pub fn compare(before: &Baseline, after: &Baseline) -> Comparison {
    let pct = |a: f64, b: f64| if a > 0.0 { (b - a) / a * 100.0 } else { 0.0 };
    let cpu_delta_percent = pct(before.cpu_passes_per_sec, after.cpu_passes_per_sec);
    let memory_delta_percent = pct(before.memory_mb_per_sec, after.memory_mb_per_sec);
    let stable = after.cpu_mismatches == 0 && after.memory_mismatches == 0;

    let mut parts = Vec::new();
    parts.push(format!("процессор {cpu_delta_percent:+.1} %, память {memory_delta_percent:+.1} %"));
    if let (Some(a), Some(b)) = (before.loaded_clock_mhz, after.loaded_clock_mhz) {
        parts.push(format!("частота под нагрузкой {:+.0} МГц", b - a));
    }
    if before.memory.profile_enabled != after.memory.profile_enabled {
        parts.push(if after.memory.profile_enabled {
            "профиль памяти включён".into()
        } else {
            "профиль памяти выключен".into()
        });
    }
    if !stable {
        parts.push(format!(
            "НАЙДЕНЫ ОШИБКИ: процессор {}, память {} — настройку надо откатить",
            after.cpu_mismatches, after.memory_mismatches
        ));
    }

    Comparison {
        cpu_delta_percent,
        memory_delta_percent,
        clock_delta_mhz: match (before.loaded_clock_mhz, after.loaded_clock_mhz) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        },
        memory_profile_changed: before.memory.profile_enabled != after.memory.profile_enabled,
        bios_version_changed: before.firmware.bios_version != after.firmware.bios_version,
        stable,
        summary: parts.join("; "),
    }
}

// --- хранение замеров ------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BaselineStore {
    pub items: Vec<Baseline>,
}

fn store_path() -> std::path::PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.baselines.json");
    p
}

pub fn load_baselines() -> BaselineStore {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_baseline(b: Baseline) -> Result<BaselineStore, String> {
    let mut store = load_baselines();
    store.items.push(b);
    // Больше десятка замеров не нужно: сравниваются соседние.
    let len = store.items.len();
    if len > 12 {
        store.items.drain(0..len - 12);
    }
    let text = serde_json::to_string_pretty(&store).map_err(|e| e.to_string())?;
    std::fs::write(store_path(), text).map_err(|e| e.to_string())?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    /// Живой замер: `cargo test --lib platform -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn measure_now() {
        let fw = super::firmware_info();
        println!("плата: {} | BIOS {} {} от {}", fw.board, fw.bios_vendor, fw.bios_version, fw.bios_date);
        println!("процессор: {} — {} ядер / {} потоков, паспортная {} МГц", fw.cpu, fw.cores, fw.threads, fw.cpu_base_mhz);

        let mem = super::memory_config();
        println!("память: {:.0} ГБ, профиль включён: {}", mem.total_gb, mem.profile_enabled);
        println!("вердикт: {}", mem.verdict);
        for m in &mem.modules {
            println!("  {} — {:.0} ГБ, {} МТ/с, {} мВ, {}", m.bank, m.capacity_gb, m.configured_mts, m.configured_mv,
                     if m.part_number.is_empty() { "без маркировки" } else { &m.part_number });
        }

        let b = super::measure("проверка", 4, 256);
        println!("\nзамер:");
        println!("  процессор: {:.0} проходов/с, расхождений {}", b.cpu_passes_per_sec, b.cpu_mismatches);
        println!("  память:    {:.0} МБ/с, расхождений {}", b.memory_mb_per_sec, b.memory_mismatches);
        println!("  частота под нагрузкой: {}", b.loaded_clock_mhz.map(|c| format!("{c:.0} МГц")).unwrap_or("не определена".into()));

        assert!(b.cpu_passes_per_sec > 0.0, "замер процессора пуст");
        assert!(b.memory_mb_per_sec > 0.0, "замер памяти пуст");
    }
}
