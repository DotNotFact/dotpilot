//! Экспериментальное управление напряжениями через ACPI-интерфейс Gigabyte (`EZV*`).
//!
//! Тот же класс `GSA1_ACPIMethod`, что и вентиляторы, отдаёт семейство `EZV*` —
//! вендорское управление напряжениями, которым пользуется фирменная утилита платы.
//! Интерфейс **самоописывающийся**: `EZVGetUIInfo(Id)` возвращает имя регулятора,
//! значение по умолчанию, границы и шаг, поэтому список не зашит в код, а читается
//! с платы. На другой модели он будет другим — и это нормально.
//!
//! Почему это важно: смещение напряжения процессора — главный рычаг Zen 4 для цели
//! «холоднее и быстрее одновременно». Раньше считалось, что из Windows он доступен
//! только через кернел-драйвер Ryzen Master (см. ADR-002 в BUSINESS.md). На платах
//! Gigabyte с этим интерфейсом драйвер не нужен.
//!
//! # Почему это безопаснее, чем звучит
//!
//! Метода фиксации в NVRAM в классе нет, а `EZVGetItemIdx` для незаданных регуляторов
//! возвращает `0xFFFFFFFF`. Значит, изменения живут только до перезагрузки — ровно как
//! смещения NVAPI. Аварийная перезагрузка сама по себе является полным откатом.
//!
//! # Что здесь запрещено намеренно
//!
//! Разрешены **только смещения (DVID Offset) и только вниз**. Прямая установка
//! абсолютного напряжения отключена, и положительное смещение тоже.
//!
//! Причина в асимметрии последствий. Слишком глубокий андервольт приводит к зависанию,
//! которое лечится перезагрузкой, потому что настройка волатильна. Повышенное
//! напряжение ничего не ломает сразу, но ускоряет деградацию кристалла — то есть
//! наносит вред, который нельзя отменить и нельзя заметить. Продукт обещает, что
//! железо прослужит дольше, а не меньше, и эта асимметрия делает выбор однозначным.

use crate::ps;
use serde::Serialize;

// --- пределы, которые нельзя перешагнуть ------------------------------------
//
// Прошивка разрешает ±300 мВ. Мы намеренно уже: этого запаса хватает для любого
// разумного андервольта, а всё, что глубже, отлаживается вслепую через зависания.

/// Самое глубокое смещение напряжения ядра, которое мы разрешаем.
const CPU_OFFSET_FLOOR_MV: i32 = -150;
/// Самое глубокое смещение напряжения SOC.
const SOC_OFFSET_FLOOR_MV: i32 = -100;
/// Потолок для любого смещения: вверх не идём никогда.
const OFFSET_CEILING_MV: i32 = 0;

/// Идентификаторы регуляторов на платах Gigabyte AM5.
/// Проверять всё равно нужно по имени: нумерация может отличаться между моделями.
const MAX_PROBE_ID: i32 = 31;

#[derive(Serialize, Clone, Debug)]
pub struct VoltageItem {
    pub id: i32,
    pub name: String,
    /// Текущее значение в милливольтах. Для смещений ноль означает «не задано».
    pub current_mv: i32,
    pub default_mv: i32,
    /// Границы, о которых сообщила прошивка.
    pub firmware_min_mv: i32,
    pub firmware_max_mv: i32,
    pub step_mv: i32,
    /// Смещение относительно штатного напряжения, а не абсолютная величина.
    pub is_offset: bool,
    /// Разрешаем ли мы менять этот регулятор.
    pub adjustable: bool,
    /// Наши границы — уже, чем у прошивки.
    pub allowed_min_mv: i32,
    pub allowed_max_mv: i32,
    /// Почему регулятор разрешён или запрещён.
    pub note: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct VoltageState {
    pub available: bool,
    pub interface_version: Option<String>,
    pub items: Vec<VoltageItem>,
    pub backend: String,
    pub note: String,
}

const PREAMBLE: &str = "$ErrorActionPreference='Stop'; \
    $i = Get-CimInstance -Namespace root\\WMI -ClassName GSA1_ACPIMethod -ErrorAction Stop; ";

/// Разбирает строку описания регулятора.
///
/// Формат: `Имя:флаги:тип:умолчание:минимум:максимум:шаг:масштаб`, числа в шестнадцатеричном
/// виде. Имя может содержать пробелы, поэтому поля отсчитываются с конца, а не с начала.
fn parse_ui_info(raw: &str) -> Option<(String, i32, i32, i32, i32, i32)> {
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() < 8 {
        return None;
    }
    let tail = &parts[parts.len() - 5..]; // умолчание, минимум, максимум, шаг, масштаб
    let name = parts[..parts.len() - 7].join(":").trim().to_string();

    // Значения приходят как беззнаковые: 0xFFFFFED4 — это −300.
    let hex = |s: &str| -> Option<i32> {
        let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
        u32::from_str_radix(t, 16).ok().map(|v| v as i32)
    };
    let default = hex(tail[0])?;
    let min = hex(tail[1])?;
    let max = hex(tail[2])?;
    let step = hex(tail[3])?;
    let scale = hex(tail[4])?;
    Some((name, default, min, max, step.max(1), scale.max(1)))
}

/// Решает, что мы разрешаем трогать, и в каких границах.
fn policy(name: &str, is_offset: bool) -> (bool, i32, i32, String) {
    if !is_offset {
        return (
            false,
            0,
            0,
            "Прямая установка напряжения отключена: она отменяет автоматическое управление \
             и при ошибке держит завышенное напряжение постоянно. Пользуйтесь смещением."
                .into(),
        );
    }
    let floor = if name.to_lowercase().contains("soc") { SOC_OFFSET_FLOOR_MV } else { CPU_OFFSET_FLOOR_MV };
    (
        true,
        floor,
        OFFSET_CEILING_MV,
        format!(
            "Разрешено только понижение, от {floor} мВ до нуля. Повышение напряжения ускоряет \
             деградацию кристалла необратимо, а слишком глубокое понижение лечится перезагрузкой."
        ),
    )
}

/// Читает список регуляторов и их текущие значения одним вызовом PowerShell.
pub fn read_state() -> VoltageState {
    let script = format!(
        "{PREAMBLE}\
         $call = {{ param($n, $a) try {{ Invoke-CimMethod -InputObject $i -MethodName $n -Arguments $a -ErrorAction Stop }} catch {{ $null }} }}; \
         $ver = & $call 'EZVGetVersion' @{{}}; \
         $items = @(); \
         foreach ($id in 0..{MAX_PROBE_ID}) {{ \
            $ui = & $call 'EZVGetUIInfo' @{{ Id = [int]$id }}; \
            if ($ui -and $ui.value) {{ \
                $v = & $call 'EZVGetVoltage' @{{ Id = [int]$id }}; \
                $idx = & $call 'EZVGetItemIdx' @{{ Id = [int]$id }}; \
                $items += [pscustomobject]@{{ id = $id; ui = [string]$ui.value; \
                    mv = if ($v) {{ [int]$v.Value }} else {{ 0 }}; \
                    idx = if ($idx) {{ [int64]$idx.Value }} else {{ -1 }} }} }} }}; \
         [pscustomobject]@{{ version = if ($ver) {{ [string]$ver.Value }} else {{ $null }}; items = $items }} | ConvertTo-Json -Compress -Depth 4"
    );

    let Ok(out) = ps::run_ps(&script) else {
        return VoltageState {
            backend: "ACPI Gigabyte (EZV)".into(),
            note: "ACPI-интерфейс напряжений не отвечает.".into(),
            ..Default::default()
        };
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) else {
        return VoltageState {
            backend: "ACPI Gigabyte (EZV)".into(),
            note: "Не удалось разобрать ответ ACPI-интерфейса.".into(),
            ..Default::default()
        };
    };

    let mut items = Vec::new();
    if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
        for it in arr {
            let id = it.get("id").and_then(|x| x.as_i64()).unwrap_or(-1) as i32;
            let raw = it.get("ui").and_then(|x| x.as_str()).unwrap_or("");
            let Some((name, default, min, max, step, _scale)) = parse_ui_info(raw) else {
                continue;
            };
            let is_offset = name.to_lowercase().contains("offset");
            let (adjustable, allowed_min, allowed_max, note) = policy(&name, is_offset);

            // Для незаданных регуляторов прошивка возвращает 0xFFFFFFFF в индексе;
            // текущим значением тогда считаем умолчание, а для смещений — ноль.
            let idx = it.get("idx").and_then(|x| x.as_i64()).unwrap_or(-1);
            let raw_mv = it.get("mv").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            let current = if idx == 0xFFFF_FFFF || idx == -1 {
                if is_offset { 0 } else { default }
            } else {
                raw_mv
            };

            items.push(VoltageItem {
                id,
                name,
                current_mv: current,
                default_mv: default,
                firmware_min_mv: min,
                firmware_max_mv: max,
                step_mv: step,
                is_offset,
                adjustable,
                allowed_min_mv: allowed_min,
                allowed_max_mv: allowed_max,
                note,
            });
        }
    }

    let adjustable = items.iter().filter(|x| x.adjustable).count();
    VoltageState {
        available: !items.is_empty(),
        interface_version: v.get("version").and_then(|x| x.as_str()).map(|s| s.to_string()),
        note: if items.is_empty() {
            "Интерфейс управления напряжениями на этой плате не найден.".into()
        } else {
            format!(
                "Найдено регуляторов: {}, из них разрешено менять: {}. Изменения живут только \
                 до перезагрузки — в прошивку они не записываются.",
                items.len(),
                adjustable
            )
        },
        backend: "ACPI Gigabyte (GSA1_ACPIMethod, EZV*)".into(),
        items,
    }
}

/// Применяет смещение напряжения.
///
/// Возвращает фактически применённое значение после обрезки — оно может отличаться
/// от запрошенного. Абсолютные регуляторы отвергаются независимо от того, что просили.
pub fn set_offset(id: i32, millivolts: i32) -> Result<i32, String> {
    let state = read_state();
    let item = state
        .items
        .iter()
        .find(|x| x.id == id)
        .ok_or_else(|| format!("Регулятор {id} не найден на этой плате."))?;

    if !item.adjustable {
        return Err(format!("«{}» менять запрещено. {}", item.name, item.note));
    }

    // Зажимаем нашими границами, потом границами прошивки, потом округляем по шагу.
    let mut value = millivolts
        .clamp(item.allowed_min_mv, item.allowed_max_mv)
        .clamp(item.firmware_min_mv, item.firmware_max_mv);
    if item.step_mv > 1 {
        value = (value / item.step_mv) * item.step_mv;
    }

    let script = format!(
        "{PREAMBLE}\
         Invoke-CimMethod -InputObject $i -MethodName 'EZVSetVoltage' -Arguments @{{ Id = [int]{id}; Value = [int]{value} }} -ErrorAction Stop | Out-Null; \
         'ok'"
    );
    match ps::run_ps(&script) {
        Ok(o) if o.trim() == "ok" => Ok(value),
        Ok(o) => Err(format!("Плата не приняла смещение: {}", o.trim())),
        Err(e) => Err(format!("Не удалось применить смещение: {e}")),
    }
}

/// Возвращает все смещения к нулю.
///
/// Выполняет все шаги даже при ошибке одного: снять максимум смещений важнее,
/// чем прерваться на первой неудаче.
pub fn reset_all() -> Result<(), String> {
    let state = read_state();
    let mut problems = Vec::new();
    for item in state.items.iter().filter(|x| x.adjustable && x.current_mv != 0) {
        if let Err(e) = set_offset(item.id, 0) {
            problems.push(e);
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_firmware_description() {
        // Строка снята с живой платы: CPU VCore, 0.800–1.550 В, умолчание 1.200, шаг 5 мВ.
        let (name, def, min, max, step, scale) =
            super::parse_ui_info("CPU VCore:0x1:I:0x4B0:0x320:0x60E:0x5:0x3E8").unwrap();
        assert_eq!(name, "CPU VCore");
        assert_eq!((def, min, max, step, scale), (1200, 800, 1550, 5, 1000));
    }

    #[test]
    fn parses_negative_bounds() {
        // Смещение: минимум приходит как 0xFFFFFED4, это −300.
        let (name, def, min, max, _, _) =
            super::parse_ui_info("CPU VCore DVID Offset:0x1:I:0x0:0xFFFFFED4:0x12C:0x5:0x3E8").unwrap();
        assert_eq!(name, "CPU VCore DVID Offset");
        assert_eq!((def, min, max), (0, -300, 300));
    }

    #[test]
    fn absolute_voltage_is_forbidden() {
        let (adjustable, _, _, note) = super::policy("CPU VCore", false);
        assert!(!adjustable, "прямая установка напряжения должна быть запрещена");
        assert!(note.contains("смещение"));
    }

    #[test]
    fn offsets_may_only_go_down() {
        let (adjustable, min, max, _) = super::policy("CPU VCore DVID Offset", true);
        assert!(adjustable);
        assert_eq!(max, 0, "повышение напряжения не должно разрешаться");
        assert_eq!(min, super::CPU_OFFSET_FLOOR_MV);

        let (_, soc_min, soc_max, _) = super::policy("VCORE SOC DVID Offset", true);
        assert_eq!(soc_max, 0);
        assert_eq!(soc_min, super::SOC_OFFSET_FLOOR_MV);
    }

    /// Живое чтение: `cargo test --lib voltage -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn read_live() {
        let s = super::read_state();
        println!("{} | {}", s.backend, s.note);
        println!("версия интерфейса: {:?}", s.interface_version);
        for it in &s.items {
            println!(
                "  [{}] {} — сейчас {} мВ, умолчание {}, прошивка допускает {}…{} мВ шагом {}",
                it.id, it.name, it.current_mv, it.default_mv, it.firmware_min_mv, it.firmware_max_mv, it.step_mv
            );
            println!(
                "       {} · наши границы {}…{} мВ",
                if it.adjustable { "разрешено" } else { "ЗАПРЕЩЕНО" },
                it.allowed_min_mv,
                it.allowed_max_mv
            );
        }
        assert!(s.available, "интерфейс напряжений не отвечает");
    }
}
