//! Встроенные тесты стабильности.
//!
//! Разгон редко ломается красиво. Гораздо чаще он не роняет систему, а начинает
//! тихо возвращать неверные числа — и обычный «поработало полчаса, не упало»
//! такого не замечает. Поэтому все тесты здесь построены на **детерминированных
//! целочисленных цепочках**: результат вычисляется один раз в спокойном состоянии
//! и дальше пересчитывается снова и снова под нагрузкой. Любое расхождение бита —
//! это ошибка железа, а не «плавающая точка немного другая».
//!
//! Целые числа выбраны сознательно: у них нет законных расхождений в последнем
//! разряде, в отличие от чисел с плавающей точкой.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Один шаг цепочки: xorshift плюс умножение с переносом.
///
/// Каждый бит результата зависит от всех битов входа, поэтому единичный сбой
/// не может «раствориться» и обязательно проявится в контрольной сумме.
#[inline(always)]
fn step(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xD1B5_4A32_D192_ED03)
}

#[inline(always)]
fn chain(seed: u64, rounds: u64) -> u64 {
    let mut x = seed;
    for _ in 0..rounds {
        x = step(x);
    }
    x
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct StressResult {
    pub kind: String,
    pub seconds: f64,
    pub threads: usize,
    /// Сколько раз цепочка была просчитана целиком.
    pub passes: u64,
    /// Несовпадения контрольной суммы — тихие ошибки вычислений.
    pub mismatches: u64,
    /// Какие расширения набора команд доступны на этом процессоре.
    pub features: Vec<String>,
    pub note: String,
}

fn cpu_features() -> Vec<String> {
    let mut v = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        for (name, present) in [
            ("avx2", is_x86_feature_detected!("avx2")),
            ("avx512f", is_x86_feature_detected!("avx512f")),
            ("avx512bw", is_x86_feature_detected!("avx512bw")),
            ("sha", is_x86_feature_detected!("sha")),
        ] {
            if present {
                v.push(name.to_string());
            }
        }
    }
    v
}

/// Нагрузка на процессор с постоянной сверкой результата.
///
/// `threads = 0` — занять все логические ядра.
pub fn cpu_stress(seconds: u64, threads: usize) -> StressResult {
    const ROUNDS: u64 = 200_000;

    let n = if threads == 0 {
        std::thread::available_parallelism().map(|v| v.get()).unwrap_or(4)
    } else {
        threads
    };

    let passes = Arc::new(AtomicU64::new(0));
    let mismatches = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds.max(1));

    let mut handles = Vec::with_capacity(n);
    for t in 0..n {
        let passes = passes.clone();
        let mismatches = mismatches.clone();
        let stop = stop.clone();
        handles.push(std::thread::spawn(move || {
            // Эталон снимается один раз: с ним сверяется всё остальное время.
            let seed = 0x1234_5678_9ABC_DEF0u64 ^ (t as u64).wrapping_mul(0x9E37_79B9);
            let reference = chain(seed, ROUNDS);
            while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                if chain(seed, ROUNDS) != reference {
                    mismatches.fetch_add(1, Ordering::Relaxed);
                }
                passes.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    let m = mismatches.load(Ordering::Relaxed);
    StressResult {
        kind: "cpu".into(),
        seconds: started.elapsed().as_secs_f64(),
        threads: n,
        passes: passes.load(Ordering::Relaxed),
        mismatches: m,
        features: cpu_features(),
        note: if m == 0 {
            "Расхождений нет: процессор считает стабильно.".into()
        } else {
            format!("Найдено {m} расхождений — процессор считает неверно под нагрузкой.")
        },
    }
}

/// Тест памяти: заполнение цепочкой и проверка при чтении.
///
/// Ловит нестабильность после включения EXPO и слишком агрессивных таймингов:
/// такие ошибки почти никогда не роняют систему сразу, но портят данные.
pub fn memory_test(megabytes: usize, seconds: u64) -> StressResult {
    let words = (megabytes.max(16) * 1024 * 1024) / std::mem::size_of::<u64>();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds.max(1));

    let mut buf: Vec<u64> = Vec::new();
    if buf.try_reserve_exact(words).is_err() {
        return StressResult {
            kind: "memory".into(),
            seconds: 0.0,
            threads: 1,
            passes: 0,
            mismatches: 0,
            features: cpu_features(),
            note: format!("Не удалось выделить {megabytes} МБ под тест памяти."),
        };
    }

    let mut passes = 0u64;
    let mut mismatches = 0u64;
    let seed = 0x0F1E_2D3C_4B5A_6978u64;

    while Instant::now() < deadline {
        // Запись: каждая ячейка получает своё значение цепочки.
        buf.clear();
        let mut x = seed;
        for _ in 0..words {
            x = step(x);
            buf.push(x);
        }
        // Чтение: пересчитываем ту же цепочку и сверяем.
        let mut y = seed;
        for &cell in buf.iter() {
            y = step(y);
            if cell != y {
                mismatches += 1;
            }
        }
        passes += 1;
    }

    StressResult {
        kind: "memory".into(),
        seconds: started.elapsed().as_secs_f64(),
        threads: 1,
        passes,
        mismatches,
        features: cpu_features(),
        note: if mismatches == 0 {
            format!("Проверено {megabytes} МБ, расхождений нет.")
        } else {
            format!("Найдено {mismatches} испорченных ячеек — память нестабильна.")
        },
    }
}

/// Сбои видеодрайвера в журнале Windows после указанного момента.
///
/// 4101 — драйвер дисплея перестал отвечать и был восстановлен (TDR). Это главный
/// признак того, что видеокарте плохо: система при этом обычно выживает, поэтому
/// без журнала такой сбой легко принять за успешный проход теста.
pub fn gpu_faults_since(ts: i64) -> Vec<String> {
    let script = format!(
        "$t=[DateTimeOffset]::FromUnixTimeSeconds({}).LocalDateTime; \
         $e=Get-WinEvent -FilterHashtable @{{LogName='System'; Id=4101,14,13; StartTime=$t}} -MaxEvents 10 -ErrorAction SilentlyContinue | \
            Where-Object {{ $_.ProviderName -match 'Display|nvlddmkm|nvidia' }}; \
         if($e){{ ($e | ForEach-Object {{ \"$($_.Id) $($_.TimeCreated.ToString('HH:mm:ss')): $($_.ProviderName)\" }}) -join \"`n\" }}",
        ts
    );
    match crate::ps::run_ps(&script) {
        Ok(out) => out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Наибольшая температура видеокарты за короткое наблюдение.
pub fn gpu_peak_temp(samples: u32, interval_ms: u64) -> Option<i32> {
    let mut peak: Option<i32> = None;
    for i in 0..samples.max(1) {
        if let Some(t) = crate::nvapi::telemetry() {
            if let Some(gpu) = t.temperatures.iter().find(|s| s.target == "gpu") {
                peak = Some(peak.map_or(gpu.current_c, |p: i32| p.max(gpu.current_c)));
            }
        }
        if i + 1 < samples {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    /// Короткий прогон на исправном железе: расхождений быть не должно.
    /// `cargo test --lib bench -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn smoke() {
        let cpu = super::cpu_stress(3, 0);
        println!("CPU: {} потоков, {} проходов за {:.1} с, расхождений {}, наборы команд: {:?}",
                 cpu.threads, cpu.passes, cpu.seconds, cpu.mismatches, cpu.features);
        assert_eq!(cpu.mismatches, 0, "на исправном процессоре расхождений быть не должно");
        assert!(cpu.passes > 0, "тест не сделал ни одного прохода");

        let mem = super::memory_test(256, 3);
        println!("Память: {} проходов за {:.1} с, расхождений {} — {}",
                 mem.passes, mem.seconds, mem.mismatches, mem.note);
        assert_eq!(mem.mismatches, 0, "на исправной памяти расхождений быть не должно");

        println!("Пиковая температура GPU: {:?}", super::gpu_peak_temp(3, 200));
    }
}
