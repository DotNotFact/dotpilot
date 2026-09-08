//! Режим сосуществования с античитами.
//!
//! Смысл раздела не в том, чтобы успокоить, а в том, чтобы показать правду: вот
//! полный список того, что приложение делает с системой, и вот оценка каждого пункта
//! глазами защиты игры. Пользователь с игровым аккаунтом должен иметь возможность
//! судить сам, а не полагаться на обещание «мы безопасны».
//!
//! Когда режим включён и обнаружен работающий античит, приложение **действительно
//! перестаёт писать в железо** — это проверяется в самих командах записи, а не
//! отображается флажком в интерфейсе. Разница принципиальная: обещание, которое
//! не подкреплено проверкой в коде, не стоит ничего.

use crate::ps;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

/// Включён ли режим. Держим в памяти, чтобы проверка в путях записи была дешёвой:
/// читать конфиг с диска на каждую команду недопустимо.
static SAFE_MODE: AtomicBool = AtomicBool::new(false);
/// Последний результат обнаружения — обновляется сборщиком.
static DETECTED: AtomicBool = AtomicBool::new(false);

pub fn set_safe_mode(on: bool) {
    SAFE_MODE.store(on, Ordering::Relaxed);
}

pub fn safe_mode() -> bool {
    SAFE_MODE.load(Ordering::Relaxed)
}

/// Известные античиты: имя процесса и как он называется для человека.
///
/// Список заведомо неполон — новые появляются постоянно. Поэтому отсутствие
/// совпадения не означает «античита нет», и в интерфейсе это сказано прямо.
const KNOWN: &[(&str, &str)] = &[
    ("BEService", "BattlEye"),
    ("BEDaisy", "BattlEye (драйвер)"),
    ("EasyAntiCheat", "Easy Anti-Cheat"),
    ("EasyAntiCheat_EOS", "Easy Anti-Cheat (EOS)"),
    ("vgc", "Riot Vanguard"),
    ("vgtray", "Riot Vanguard"),
    ("FACEIT", "FACEIT Anti-Cheat"),
    ("faceitclient", "FACEIT Anti-Cheat"),
    ("ESEAClient", "ESEA"),
    ("mhyprot", "mhyprot"),
    ("GameGuard", "nProtect GameGuard"),
    ("npggsvc", "nProtect GameGuard"),
    ("xhunter1", "XIGNCODE"),
    ("Wellbia", "XIGNCODE / uncheater"),
];

#[derive(Serialize, Clone, Debug)]
pub struct DetectedAntiCheat {
    pub process: String,
    pub product: String,
    pub pid: u32,
}

/// Что приложение делает с системой и как это выглядит со стороны защиты игры.
#[derive(Serialize, Clone, Debug)]
pub struct Activity {
    pub what: String,
    /// Через какой механизм.
    pub how: String,
    /// Оценка: `ordinary` — делают обычные приложения, `notable` — заметно, но законно,
    /// `paused` — приостановлено режимом.
    pub kind: String,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct AntiCheatStatus {
    pub safe_mode: bool,
    pub detected: Vec<DetectedAntiCheat>,
    /// Блокируется ли запись прямо сейчас.
    pub writes_blocked: bool,
    pub activities: Vec<Activity>,
    pub note: String,
}

/// Ищет работающие античиты среди процессов.
fn detect() -> Vec<DetectedAntiCheat> {
    let names: Vec<&str> = KNOWN.iter().map(|(n, _)| *n).collect();
    let filter = names.join("','");
    let script = format!(
        "@(Get-Process -ErrorAction SilentlyContinue | Where-Object {{ $_.ProcessName -in @('{filter}') }} | \
          Select-Object ProcessName, Id) | ConvertTo-Json -Compress"
    );
    let Ok(out) = ps::run_ps(&script) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) else {
        return Vec::new();
    };
    let arr = if v.is_array() { v } else { serde_json::Value::Array(vec![v]) };
    arr.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|x| {
                    let name = x.get("ProcessName").and_then(|n| n.as_str())?;
                    let product = KNOWN
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(name))
                        .map(|(_, p)| *p)
                        .unwrap_or(name);
                    Some(DetectedAntiCheat {
                        process: name.to_string(),
                        product: product.to_string(),
                        pid: x.get("Id").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Причина, по которой запись в железо сейчас запрещена, либо `None`.
///
/// Вызывается в начале каждой команды, меняющей состояние железа. Именно здесь
/// режим становится настоящим ограничением, а не пометкой в интерфейсе.
pub fn writes_blocked() -> Option<String> {
    if safe_mode() && DETECTED.load(Ordering::Relaxed) {
        Some(
            "Запись в железо приостановлена: включён режим сосуществования с античитом, \
             и античит сейчас работает. Закройте игру или выключите режим на странице «Доступ»."
                .into(),
        )
    } else {
        None
    }
}

/// Имена процессов античитов для передачи в поиск сборщика.
pub fn process_names() -> Vec<String> {
    KNOWN.iter().map(|(n, _)| format!("{n}.exe")).collect()
}

/// Обновляет признак присутствия античита. Вызывается сборщиком по списку процессов,
/// который он и так получает, — без отдельного обращения к системе.
pub fn note_running_processes(names: &[String]) {
    let found = names.iter().any(|n| {
        let stem = n.trim_end_matches(".exe");
        KNOWN.iter().any(|(k, _)| k.eq_ignore_ascii_case(stem))
    });
    DETECTED.store(found, Ordering::Relaxed);
}

/// Полный отчёт для интерфейса.
pub fn status() -> AntiCheatStatus {
    let detected = detect();
    DETECTED.store(!detected.is_empty(), Ordering::Relaxed);
    let blocked = writes_blocked().is_some();
    /// Пункты, которые приостанавливаются режимом, помечаются иначе, когда он активен.
    fn kind(blocked: bool, normal: &'static str) -> &'static str {
        if blocked {
            "paused"
        } else {
            normal
        }
    }
    let paused = |s: &'static str| kind(blocked, s);

    let activities = vec![
        Activity {
            what: "Чтение датчиков видеокарты".into(),
            how: "NVAPI, библиотека драйвера NVIDIA".into(),
            kind: "ordinary".into(),
            detail: "Тем же способом работает MSI Afterburner. Античиты к нему претензий не предъявляют.".into(),
        },
        Activity {
            what: "Изменение частот, лимита мощности и вентиляторов видеокарты".into(),
            how: "NVAPI".into(),
            kind: paused("ordinary").into(),
            detail: "Запись в драйвер видеокарты, не в процесс игры.".into(),
        },
        Activity {
            what: "Чтение и настройка вентиляторов платы".into(),
            how: "ACPI-интерфейс платы через WMI".into(),
            kind: paused("ordinary").into(),
            detail: "Штатный вендорский интерфейс, идёт через инбоксовый драйвер Microsoft.".into(),
        },
        Activity {
            what: "Смещение напряжения процессора".into(),
            how: "ACPI-интерфейс платы через WMI".into(),
            kind: paused("notable").into(),
            detail: "Тот же механизм, что у фирменной утилиты платы. Изменение затрагивает питание, а не игру.".into(),
        },
        Activity {
            what: "Список процессов: имена, пути, расход памяти".into(),
            how: "Обычный системный вызов перечисления процессов".into(),
            kind: "notable".into(),
            detail: "То же делает диспетчер задач. Память чужих процессов не читается.".into(),
        },
        Activity {
            what: "Завершение процессов по кнопке".into(),
            how: "Запрос на завершение процесса".into(),
            kind: paused("notable").into(),
            detail: "Выполняется только по явному нажатию. В режиме сосуществования запрещено.".into(),
        },
        Activity {
            what: "Правила приоритета сетевого трафика".into(),
            how: "Политики QoS Windows".into(),
            kind: "ordinary".into(),
            detail: "Помечают пакеты приложения, но не вмешиваются в их содержимое.".into(),
        },
    ];

    let note = if !detected.is_empty() {
        format!(
            "Обнаружено: {}. {}",
            detected.iter().map(|d| d.product.clone()).collect::<Vec<_>>().join(", "),
            if blocked {
                "Запись в железо приостановлена."
            } else {
                "Режим сосуществования выключен, ограничения не действуют."
            }
        )
    } else {
        "Работающих античитов из известного списка не найдено. Список неполон: новые \
         защиты появляются постоянно, поэтому отсутствие совпадения не гарантия."
            .into()
    };

    AntiCheatStatus { safe_mode: safe_mode(), detected, writes_blocked: blocked, activities, note }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Обнаружение и блокировка проверяются одним тестом намеренно.
    ///
    /// Оба признака живут в глобальных переменных, а тесты Rust идут параллельно:
    /// разделённые тесты сбрасывали бы состояние друг другу и падали через раз.
    #[test]
    fn detection_and_blocking() {
        note_running_processes(&["Game.exe".into(), "BEService.exe".into()]);
        assert!(DETECTED.load(Ordering::Relaxed), "BattlEye должен опознаваться");

        set_safe_mode(false);
        assert!(writes_blocked().is_none(), "без включённого режима запись не блокируется");

        set_safe_mode(true);
        assert!(writes_blocked().is_some(), "античит работает и режим включён — запись должна быть заблокирована");

        note_running_processes(&["notepad.exe".into()]);
        assert!(!DETECTED.load(Ordering::Relaxed), "посторонние процессы не должны срабатывать");
        assert!(writes_blocked().is_none(), "античита нет — блокировать нечего");

        set_safe_mode(false);
    }
}
