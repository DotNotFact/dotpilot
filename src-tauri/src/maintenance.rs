//! Планировщик обслуживания и уведомления.
//!
//! Часть проверок имеет смысл только если делать их регулярно. Ошибки памяти
//! накапливаются месяцами, износ накопителя виден на длинной дистанции, пыль
//! оседает незаметно. Приложение помнит, когда что делалось в последний раз,
//! и говорит, что пора.
//!
//! Тяжёлые проверки **не запускаются сами**: тест памяти займёт минуты и загрузит
//! машину, а решать, когда это уместно, должен человек. Планировщик только
//! напоминает.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Ledger {
    /// Идентификатор задачи → момент последнего выполнения.
    pub last_run: HashMap<String, i64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub every_days: u32,
    pub last_run: Option<i64>,
    pub days_since: Option<f64>,
    pub due: bool,
    /// Может ли приложение выполнить задачу само, или это ручная работа.
    pub automatic: bool,
    pub why: String,
    /// Куда идти, чтобы выполнить.
    pub where_to: String,
}

fn path() -> std::path::PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.maintenance.json");
    p
}

pub fn load() -> Ledger {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(l: &Ledger) -> Result<(), String> {
    let text = serde_json::to_string_pretty(l).map_err(|e| e.to_string())?;
    std::fs::write(path(), text).map_err(|e| e.to_string())
}

/// Отмечает задачу выполненной сейчас.
pub fn mark_done(id: &str) -> Result<(), String> {
    let mut l = load();
    l.last_run.insert(id.to_string(), chrono::Utc::now().timestamp());
    store(&l)
}

/// Список задач с расчётом, что уже пора.
///
/// Интервалы выбраны по характеру проблемы, а не «чтобы было»: ошибки памяти
/// накапливаются месяцами, поэтому месяц; состояние железа меняется быстрее,
/// поэтому неделя; пыль оседает за полгода.
pub fn tasks() -> Vec<Task> {
    let ledger = load();
    let now = chrono::Utc::now().timestamp();

    let defs: &[(&str, &str, u32, bool, &str, &str)] = &[
        (
            "health",
            "Проверка состояния ПК",
            7,
            true,
            "Ошибки WHEA, перегрев накопителей и износ появляются постепенно. Раз в неделю \
             достаточно, чтобы заметить их до того, как они станут проблемой.",
            "Страница «Здоровье ПК»",
        ),
        (
            "memory",
            "Тест памяти",
            30,
            true,
            "Нестабильная память портит данные молча, не роняя систему. Особенно важно после \
             включения EXPO или смены модулей.",
            "Страница «Процессор», кнопка «Снять замер»",
        ),
        (
            "gpu_stability",
            "Проверка стабильности видеокарты",
            90,
            true,
            "Настройка, стабильная зимой, может перестать быть таковой летом: запас по \
             температуре меняется вместе с комнатой.",
            "Страница «Видеокарта», проверка ступени",
        ),
        (
            "dust",
            "Осмотр и чистка от пыли",
            180,
            false,
            "Приложение видит деградацию охлаждения по росту температуры при той же мощности, \
             но убрать пыль может только человек.",
            "Вручную: продуть радиаторы и фильтры",
        ),
        (
            "thermal_paste",
            "Замена термопасты",
            1095,
            false,
            "Паста высыхает за два-четыре года. Срок ориентировочный: если температура при той \
             же мощности не растёт, торопиться некуда.",
            "Вручную или в сервисе",
        ),
    ];

    defs.iter()
        .map(|(id, name, every, automatic, why, where_to)| {
            let last = ledger.last_run.get(*id).copied();
            let days_since = last.map(|t| (now - t) as f64 / 86400.0);
            Task {
                id: (*id).to_string(),
                name: (*name).to_string(),
                every_days: *every,
                last_run: last,
                days_since,
                // Задача, которую ни разу не делали, считается назревшей: иначе
                // напоминание не появится никогда.
                due: days_since.map(|d| d >= *every as f64).unwrap_or(true),
                automatic: *automatic,
                why: (*why).to_string(),
                where_to: (*where_to).to_string(),
            }
        })
        .collect()
}

// --- уведомления -----------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct NotifyResult {
    pub sent: bool,
    pub detail: String,
}

/// Отправляет сообщение в Telegram.
///
/// Отправка наружу включается только вручную и только с токеном, который вводит
/// владелец. Состав отправляемого перечислен в интерфейсе: приложение не собирает
/// телеметрию и никуда её не шлёт помимо этого.
pub fn send(cfg: &crate::config::Config, text: &str) -> Result<NotifyResult, String> {
    if cfg.telegram_bot_token.trim().is_empty() || cfg.telegram_chat_id.trim().is_empty() {
        return Err("Не заданы токен бота и идентификатор чата.".into());
    }
    let mut builder = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20));
    // Telegram у части провайдеров недоступен напрямую — используем тот же прокси,
    // что настроен для обращений к модели.
    if !cfg.ai_proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(cfg.ai_proxy.trim()).map_err(|e| e.to_string())?);
    }
    let client = builder.build().map_err(|e| e.to_string())?;

    let url = format!("https://api.telegram.org/bot{}/sendMessage", cfg.telegram_bot_token.trim());
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": cfg.telegram_chat_id.trim(),
            "text": text,
            "disable_notification": false,
        }))
        .send()
        .map_err(|e| format!("Не удалось отправить: {e}"))?;

    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status.is_success() {
        Ok(NotifyResult { sent: true, detail: "Сообщение доставлено.".into() })
    } else {
        // Токен не должен попасть в сообщение об ошибке и в журнал.
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        let why = v
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("подробностей нет")
            .to_string();
        Err(format!("Telegram отклонил запрос ({}): {why}", status.as_u16()))
    }
}

/// Отправляет уведомление, если оно включено. Ошибки не пробрасываются:
/// неудачная отправка не должна ломать действие, ради которого она затевалась.
pub fn notify_if_enabled(cfg: &crate::config::Config, text: &str) {
    if !cfg.notify_enabled {
        return;
    }
    let _ = send(cfg, text);
}

#[cfg(test)]
mod tests {
    #[test]
    fn never_done_task_is_due() {
        // На чистой машине журнал пуст, и все задачи должны считаться назревшими:
        // иначе напоминание не появится никогда.
        let t = super::tasks();
        assert!(!t.is_empty());
        let health = t.iter().find(|x| x.id == "health").unwrap();
        if health.last_run.is_none() {
            assert!(health.due, "невыполненная ни разу задача должна быть назревшей");
        }
    }

    #[test]
    fn intervals_are_sane() {
        for t in super::tasks() {
            assert!(t.every_days > 0, "интервал задачи «{}» не задан", t.name);
            assert!(!t.why.is_empty(), "у задачи «{}» нет объяснения", t.name);
        }
    }
}
