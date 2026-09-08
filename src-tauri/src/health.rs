//! Оценка состояния ПК: что измерено, что считается нормой и где расхождение.
//!
//! Нормы зашиты здесь, в коде, а не спрашиваются у модели каждый раз. Причина
//! простая: «70 °C — это нормально?» имеет разный ответ для разного железа, и
//! ответ должен быть воспроизводимым. Каждый порог снабжён пояснением, откуда он
//! взялся, чтобы вывод можно было оспорить, а не принимать на веру.
//!
//! Отдельно важен контекст нагрузки: 70 °C на простое и 70 °C под полной нагрузкой —
//! это разные новости, поэтому температура процессора оценивается вместе с загрузкой.

use crate::{fanctl, nvapi, platform, ps};
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Всё в порядке, вмешательство не требуется.
    Ok,
    /// Не проблема, но стоит знать.
    Notice,
    /// Работает, но хуже, чем должно.
    Warning,
    /// Требует внимания сейчас.
    Problem,
}

#[derive(Serialize, Clone, Debug)]
pub struct Finding {
    pub area: String,
    pub title: String,
    pub severity: Severity,
    /// Что именно измерено.
    pub measured: String,
    /// Что считается нормой и почему.
    pub reference: String,
    pub advice: Option<String>,
}

impl Finding {
    fn new(area: &str, title: &str, severity: Severity, measured: String, reference: &str) -> Self {
        Self {
            area: area.into(),
            title: title.into(),
            severity,
            measured,
            reference: reference.into(),
            advice: None,
        }
    }
    fn advise(mut self, advice: &str) -> Self {
        self.advice = Some(advice.into());
        self
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct HealthReport {
    pub at: i64,
    pub findings: Vec<Finding>,
    pub problems: usize,
    pub warnings: usize,
    pub summary: String,
}

// --- нормы ------------------------------------------------------------------
//
// Значения относятся к конкретным поколениям железа и отмечены источником смысла,
// а не выдуманы: Zen 4 официально работает до 95 °C, и это рабочий режим, а не авария.

/// Zen 4 (Ryzen 7000) держит предел 95 °C штатно: процессор сам подбирает частоту так,
/// чтобы упереться в этот предел под нагрузкой. Это не перегрев, а рабочий режим.
const ZEN4_TJMAX_C: u8 = 95;
/// Выше этого в простое — уже повод посмотреть на охлаждение.
const CPU_IDLE_HIGH_C: u8 = 60;
/// Нагрузкой считаем загрузку выше этого процента.
const LOAD_THRESHOLD_PCT: f32 = 40.0;

/// Ada и Blackwell штатно работают примерно до 83 °C по ядру, дальше начинается
/// снижение частоты. Ниже 80 — комфортная зона.
const GPU_WARM_C: i32 = 80;
const GPU_HOT_C: i32 = 87;

/// NVMe обычно троттлит около 70 °C.
const SSD_WARM_C: u32 = 60;
const SSD_HOT_C: u32 = 70;

/// Меньше этого свободного места — Windows начинает вести себя странно.
const DISK_FREE_LOW_PCT: f64 = 10.0;

/// Прошивка старше этого срока на AM5 заметна: AGESA чинит совместимость памяти.
const BIOS_STALE_DAYS: i64 = 400;

#[derive(serde::Deserialize, Default)]
struct SystemFacts {
    #[serde(default)]
    disks: Vec<DiskFact>,
    #[serde(default)]
    volumes: Vec<VolumeFact>,
    #[serde(default)]
    whea: Vec<String>,
    #[serde(default)]
    uptime_hours: f64,
}

#[derive(serde::Deserialize, Default)]
struct DiskFact {
    #[serde(default)]
    name: String,
    #[serde(default)]
    media: String,
    #[serde(default)]
    health: String,
    #[serde(default)]
    temp: Option<u32>,
    #[serde(default)]
    wear: Option<u32>,
    #[serde(default)]
    hours: Option<u64>,
    #[serde(default)]
    read_err: Option<u64>,
    #[serde(default)]
    write_err: Option<u64>,
}

#[derive(serde::Deserialize, Default)]
struct VolumeFact {
    #[serde(default)]
    letter: String,
    #[serde(default)]
    size_gb: f64,
    #[serde(default)]
    free_gb: f64,
}

/// Всё, что требует PowerShell, собирается одним вызовом.
fn system_facts() -> SystemFacts {
    let script = r#"
$disks = @(Get-PhysicalDisk -ErrorAction SilentlyContinue | ForEach-Object {
  $r = $_ | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue
  [pscustomobject]@{
    name = $_.FriendlyName; media = [string]$_.MediaType; health = [string]$_.HealthStatus
    temp = $r.Temperature; wear = $r.Wear; hours = $r.PowerOnHours
    read_err = $r.ReadErrorsUncorrected; write_err = $r.WriteErrorsUncorrected
  }
})
$vols = @(Get-Volume -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter -and $_.Size -gt 0 } | ForEach-Object {
  [pscustomobject]@{ letter = [string]$_.DriveLetter; size_gb = [math]::Round($_.Size/1GB,1); free_gb = [math]::Round($_.SizeRemaining/1GB,1) }
})
$whea = @()
try {
  $whea = @(Get-WinEvent -FilterHashtable @{ LogName='System'; ProviderName='Microsoft-Windows-WHEA-Logger'; StartTime=(Get-Date).AddDays(-30) } -MaxEvents 20 -ErrorAction Stop |
    ForEach-Object { "$($_.Id) $($_.TimeCreated.ToString('dd.MM HH:mm'))" })
} catch {}
$up = ((Get-Date) - (Get-CimInstance Win32_OperatingSystem).LastBootUpTime).TotalHours
[pscustomobject]@{ disks = $disks; volumes = $vols; whea = $whea; uptime_hours = [math]::Round($up,1) } | ConvertTo-Json -Compress -Depth 5
"#;
    ps::run_ps(script)
        .ok()
        .and_then(|o| serde_json::from_str(&o).ok())
        .unwrap_or_default()
}

/// Полная проверка. `cpu_load_pct` нужен, чтобы правильно судить о температуре.
pub fn check(cpu_load_pct: f32) -> HealthReport {
    let mut f: Vec<Finding> = Vec::new();
    let facts = system_facts();
    let fans = fanctl::read_state();
    let gpu = nvapi::telemetry();
    let mem = platform::memory_config();
    let fw = platform::firmware_info();
    let under_load = cpu_load_pct >= LOAD_THRESHOLD_PCT;

    // --- процессор ---------------------------------------------------------
    // Датчик, к которому плата привязала вентиляторы, и есть процессорный.
    let cpu_sensor = fans
        .fans
        .first()
        .map(|x| x.target_sensor)
        .and_then(|id| fans.sensors.iter().find(|s| s.id == id && s.connected))
        .or_else(|| fans.sensors.iter().find(|s| s.id == 2 && s.connected));

    if let Some(s) = cpu_sensor {
        let t = s.celsius;
        let (sev, reference) = if t >= ZEN4_TJMAX_C {
            (
                Severity::Warning,
                "Достигнут штатный предел Zen 4 в 95 °C. Процессор снижает частоту, чтобы \
                 не идти выше. Это не авария, но выше уже не будет — упор в охлаждение.",
            )
        } else if under_load {
            (
                Severity::Ok,
                "Под нагрузкой для Ryzen 7000 нормальны 75–95 °C: процессор намеренно \
                 разгоняется до теплового предела в 95 °C и держится у него. Всё, что ниже, — запас.",
            )
        } else if t >= CPU_IDLE_HIGH_C {
            (
                Severity::Notice,
                "Для простоя многовато: обычно Ryzen 7000 держит 35–50 °C без нагрузки. \
                 Короткие скачки нормальны — Zen 4 реагирует на любую мелкую задачу.",
            )
        } else {
            (Severity::Ok, "Для простоя нормально: обычный диапазон Ryzen 7000 — 35–50 °C.")
        };
        f.push(
            Finding::new(
                "Процессор",
                "Температура",
                sev,
                format!(
                    "{t} °C при загрузке {:.0} % ({})",
                    cpu_load_pct,
                    if under_load { "под нагрузкой" } else { "в простое" }
                ),
                reference,
            )
            .advise(if t >= ZEN4_TJMAX_C {
                "Отрицательное смещение Curve Optimizer снизит температуру и поднимет буст одновременно — см. страницу «Процессор»."
            } else {
                "Ничего делать не нужно."
            }),
        );
    }

    // --- память ------------------------------------------------------------
    if !mem.profile_enabled {
        f.push(
            Finding::new(
                "Память",
                "Профиль EXPO выключен",
                Severity::Warning,
                mem.modules
                    .first()
                    .map(|m| format!("{} МТ/с при {:.3} В", m.configured_mts, m.configured_mv as f32 / 1000.0))
                    .unwrap_or_else(|| "модули не определены".into()),
                "Модули работают на базовой скорости JEDEC вместо своей паспортной. \
                 Для Ryzen это заметнее всего в минимальном FPS: контроллер памяти Zen 4 \
                 напрямую влияет на задержки в играх.",
            )
            .advise("Включите EXPO в BIOS и снимите замер до и после на странице «Процессор» — прибавка будет видна числом."),
        );
    } else {
        f.push(Finding::new(
            "Память",
            "Профиль EXPO включён",
            Severity::Ok,
            mem.modules
                .first()
                .map(|m| format!("{} МТ/с при {:.3} В", m.configured_mts, m.configured_mv as f32 / 1000.0))
                .unwrap_or_default(),
            "Модули работают по своему паспортному профилю.",
        ));
    }

    // --- видеокарта --------------------------------------------------------
    if let Some(g) = &gpu {
        if let Some(t) = g.temperatures.iter().find(|s| s.target == "gpu") {
            let (sev, reference) = if t.current_c >= GPU_HOT_C {
                (
                    Severity::Warning,
                    "Близко к тепловому пределу: карта начнёт снижать частоту. Обычно это забитый \
                     пылью радиатор или слишком тихая кривая вентиляторов.",
                )
            } else if t.current_c >= GPU_WARM_C {
                (Severity::Notice, "Тепло, но в рабочем диапазоне. Под нагрузкой такие значения обычны.")
            } else {
                (Severity::Ok, "В комфортной зоне: для игровой нагрузки нормальны 60–78 °C.")
            };
            f.push(Finding::new(
                "Видеокарта",
                "Температура",
                sev,
                format!("{} °C при загрузке {} %", t.current_c, g.util_gpu_percent.unwrap_or(0)),
                reference,
            ));
        }

        if let (Some(cur), Some(max)) = (g.power_current_percent, g.power_max_percent) {
            if cur < 99.0 {
                f.push(Finding::new(
                    "Видеокарта",
                    "Лимит мощности занижен",
                    Severity::Notice,
                    format!("{cur:.0} % от штатного при доступных {max:.0} %"),
                    "Карта ограничена ниже паспортного значения — это осознанная настройка, \
                     но если она не ваша, часть производительности не используется.",
                ));
            }
        }
    } else {
        f.push(Finding::new(
            "Видеокарта",
            "NVAPI не отвечает",
            Severity::Notice,
            "карта NVIDIA не найдена".into(),
            "Телеметрия и управление видеокартой доступны только для NVIDIA.",
        ));
    }

    // --- аппаратные ошибки -------------------------------------------------
    // WHEA — единственный прямой признак того, что железо ошибается на самом деле.
    if facts.whea.is_empty() {
        f.push(Finding::new(
            "Стабильность",
            "Аппаратных ошибок нет",
            Severity::Ok,
            "за 30 дней записей WHEA не найдено".into(),
            "WHEA фиксирует ошибки, которые железо обнаружило само: сбои памяти, шины, кэша. \
             Пустой журнал — лучший признак стабильности.",
        ));
    } else {
        f.push(
            Finding::new(
                "Стабильность",
                "Журнал WHEA не пуст",
                Severity::Problem,
                format!("{} записей за 30 дней: {}", facts.whea.len(), facts.whea.join(", ")),
                "WHEA фиксирует ошибки, которые железо обнаружило само. Их не бывает «просто так»: \
                 обычно это нестабильная память, слишком агрессивный разгон или деградация питания.",
            )
            .advise("Проверьте память тестом на странице «Процессор» и снимите разгон, если он применён."),
        );
    }

    // --- накопители --------------------------------------------------------
    for d in &facts.disks {
        if !d.health.is_empty() && !d.health.eq_ignore_ascii_case("Healthy") {
            f.push(Finding::new(
                "Накопители",
                "Диск сообщает о проблеме",
                Severity::Problem,
                format!("{}: состояние {}", d.name, d.health),
                "Windows считает диск неисправным по данным SMART.",
            ));
        }
        // Факты о накопителе: без них вопрос «что с моими комплектующими» остаётся без ответа.
        let mut facts_line = format!("{}{}", d.name, if d.media.is_empty() { String::new() } else { format!(" ({})", d.media) });
        if let Some(h) = d.hours {
            facts_line.push_str(&format!(", наработка {} ч ≈ {:.1} года", h, h as f64 / 24.0 / 365.0));
        }
        if let Some(w) = d.wear {
            facts_line.push_str(&format!(", израсходовано ресурса {w} %"));
        }
        if let Some(t) = d.temp {
            facts_line.push_str(&format!(", {t} °C"));
        }
        f.push(Finding::new(
            "Накопители",
            "Состояние диска",
            if d.health.eq_ignore_ascii_case("Healthy") { Severity::Ok } else { Severity::Notice },
            facts_line,
            "Ресурс записи у SSD измеряется в процентах: 100 % означает исчерпание гарантийного \
             объёма записи, а не немедленный отказ. Наработка сама по себе ни о чём не говорит — \
             важны ошибки и износ.",
        ));

        // У жёстких дисков нормы температуры другие, поэтому пороги NVMe к ним не применяем.
        let is_hdd = d.media.eq_ignore_ascii_case("HDD");
        if let Some(t) = d.temp.filter(|_| !is_hdd) {
            if t >= SSD_HOT_C {
                f.push(Finding::new(
                    "Накопители",
                    "Диск перегревается",
                    Severity::Warning,
                    format!("{}: {t} °C", d.name),
                    "NVMe обычно начинает снижать скорость около 70 °C. Помогает радиатор или обдув.",
                ));
            } else if t >= SSD_WARM_C {
                f.push(Finding::new(
                    "Накопители",
                    "Диск тёплый",
                    Severity::Notice,
                    format!("{}: {t} °C", d.name),
                    "Ещё не троттлинг, но запас до 70 °C небольшой.",
                ));
            }
        }
        let errors = d.read_err.unwrap_or(0) + d.write_err.unwrap_or(0);
        if errors > 0 {
            f.push(Finding::new(
                "Накопители",
                "Неисправленные ошибки чтения или записи",
                Severity::Problem,
                format!("{}: {errors}", d.name),
                "Такие ошибки означают потерянные данные, а не замедление.",
            ));
        }
        // Прогноз ресурса: пересчёт износа в оставшиеся годы при нынешнем темпе.
        // Работает только когда износ уже начал считаться — иначе делить не на что,
        // и честнее сказать об этом, чем показать бесконечность.
        match (d.wear, d.hours) {
            (Some(w), Some(h)) if w > 0 && h > 0 => {
                let years_used = h as f64 / 24.0 / 365.0;
                let years_left = years_used * (100.0 - w as f64) / w as f64;
                f.push(Finding::new(
                    "Накопители",
                    "Прогноз ресурса записи",
                    if years_left < 2.0 { Severity::Warning } else { Severity::Ok },
                    format!(
                        "{}: израсходовано {w} % за {years_used:.1} года — при нынешнем темпе хватит ещё примерно на {years_left:.1} года",
                        d.name
                    ),
                    "Прогноз линейный: он верен, пока характер нагрузки не меняется. \
                     Исчерпание ресурса означает переход в режим только для чтения, а не потерю данных.",
                ));
            }
            (Some(0), Some(h)) if h > 500 => {
                f.push(Finding::new(
                    "Накопители",
                    "Ресурс записи ещё не начал расходоваться",
                    Severity::Ok,
                    format!("{}: износ 0 % за {:.1} года работы", d.name, h as f64 / 24.0 / 365.0),
                    "Счётчик износа целочисленный: ноль означает, что не израсходован даже первый процент. \
                     Прогноз в годах при этом построить не из чего.",
                ));
            }
            _ => {}
        }

        if let Some(w) = d.wear {
            if w >= 80 {
                f.push(Finding::new(
                    "Накопители",
                    "Ресурс записи почти исчерпан",
                    Severity::Warning,
                    format!("{}: износ {w} %", d.name),
                    "После 100 % накопитель может перейти в режим только для чтения.",
                ));
            }
        }
    }

    for v in &facts.volumes {
        if v.size_gb <= 0.0 {
            continue;
        }
        let free_pct = v.free_gb / v.size_gb * 100.0;
        if free_pct < DISK_FREE_LOW_PCT {
            f.push(
                Finding::new(
                    "Накопители",
                    "Мало свободного места",
                    Severity::Warning,
                    format!("диск {}: свободно {:.0} ГБ из {:.0} ({free_pct:.0} %)", v.letter, v.free_gb, v.size_gb),
                    "Ниже 10 % Windows тормозит из-за нехватки места под файл подкачки и обновления.",
                )
                .advise("Освободите место или перенесите крупные файлы."),
            );
        }
    }

    // --- вентиляторы -------------------------------------------------------
    if let Some(s) = cpu_sensor {
        let stopped: Vec<u8> = fans.fans.iter().filter(|x| x.stop_enabled).map(|x| x.id).collect();
        if !stopped.is_empty() && s.celsius >= 70 {
            f.push(
                Finding::new(
                    "Охлаждение",
                    "Остановка вентиляторов разрешена при высокой температуре",
                    Severity::Warning,
                    format!("вентиляторы {stopped:?} могут стоять, сейчас {} °C", s.celsius),
                    "Остановка задумана для тишины в простое, а не под нагрузкой.",
                )
                .advise("Проверьте пороги на странице «Вентиляторы»."),
            );
        }
    }

    // --- прошивка ----------------------------------------------------------
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&fw.bios_date, "%Y-%m-%d") {
        let age = (chrono::Utc::now().date_naive() - date).num_days();
        if age > BIOS_STALE_DAYS {
            f.push(
                Finding::new(
                    "Плата",
                    "Прошивка давно не обновлялась",
                    Severity::Notice,
                    format!("{} от {} — это {} дней назад", fw.bios_version, fw.bios_date, age),
                    "На AM5 обновления AGESA заметно влияют на совместимость и стабильность памяти. \
                     Само по себе это не поломка.",
                )
                .advise("Если планируете включать EXPO, свежая прошивка повышает шансы, что профиль заработает с первого раза."),
            );
        }
    }

    // --- время работы ------------------------------------------------------
    if facts.uptime_hours > 24.0 * 7.0 {
        f.push(Finding::new(
            "Система",
            "Давно без перезагрузки",
            Severity::Notice,
            format!("{:.0} дней непрерывной работы", facts.uptime_hours / 24.0),
            "Не поломка, но часть обновлений и утечек памяти лечится только перезагрузкой.",
        ));
    }

    let problems = f.iter().filter(|x| x.severity == Severity::Problem).count();
    let warnings = f.iter().filter(|x| x.severity == Severity::Warning).count();
    let summary = if problems > 0 {
        format!("Требует внимания: {problems}. Замечаний: {warnings}.")
    } else if warnings > 0 {
        format!("Серьёзных проблем нет. Есть что улучшить: {warnings}.")
    } else {
        "Всё в норме: проблем и замечаний не найдено.".to_string()
    };

    HealthReport {
        at: chrono::Utc::now().timestamp(),
        findings: f,
        problems,
        warnings,
        summary,
    }
}

#[cfg(test)]
mod tests {
    /// Живая проверка: `cargo test --lib health -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn check_live() {
        let r = super::check(15.0);
        println!("{}\n", r.summary);
        for x in &r.findings {
            println!("[{:?}] {} — {}", x.severity, x.area, x.title);
            println!("   измерено: {}", x.measured);
            println!("   норма:    {}", x.reference);
            if let Some(a) = &x.advice {
                println!("   совет:    {a}");
            }
            println!();
        }
        assert!(!r.findings.is_empty(), "проверка не дала ни одного вывода");
    }
}
