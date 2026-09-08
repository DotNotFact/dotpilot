//! Петля подбора разгона с Claude.
//!
//! Модель получает снимок состояния карты и весь журнал подбора — что уже
//! проверено, что отклонено и почему — и предлагает **один** следующий шаг.
//!
//! Ответ забирается через tool-use, а не разбором свободного текста: так у нас
//! всегда структура, а не «почти JSON». Но структурность не означает доверия —
//! предложение всё равно проходит `Bounds::clamp`, и выйти за коридор оно не может.
//! Модель здесь советчик, а решение о допустимости принимает код.

use crate::config::Config;
use crate::{nvapi, ocsafe};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const SYSTEM_PROMPT: &str = "Ты — инженер по разгону видеокарт. Подбираешь настройки для NVIDIA GeForce RTX 5070 Ti (Blackwell) в связке с Ryzen 9 7900X.

Цель владельца дословно: больше производительности при стабильной работе, чтобы карта прослужила дольше, а не меньше, и меньше грелась. Это значит, что рост производительности любой ценой — неверная стратегия.

Как это достигается на практике: связка «пониженный лимит мощности плюс положительное смещение частоты ядра» даёт больше кадров на ватт, чем простое повышение лимита. Карта при этом холоднее и тише, а буст держится выше. Начинай именно с этого направления, а не с подъёма лимита до потолка.

Правила, которых держись строго:

1. Один шаг за раз. Меняй одну величину за итерацию, иначе при сбое непонятно, что виновато.
2. Шаги мелкие: ядро не более чем на 15–30 МГц за раз, память на 50–100 МГц, лимит мощности на 5 процентных пунктов.
3. Память трогай только после того, как ядро стабилизировалось: её ошибки тише всего и портят данные незаметно.
4. Если предыдущий шаг отклонён, откатись заметно ниже последней проверенной настройки, а не на один шаг назад.
5. Никогда не предлагай комбинацию из списка отклонённых.
6. Когда дальнейший рост даёт мало, а риск растёт, ставь stop и объясни, почему подбор закончен.

Приложение обрежет твоё предложение жёсткими границами, если ты выйдешь за коридор, так что предлагать заведомо запредельные значения бессмысленно.

В reasoning пиши коротко и по делу на русском: что видно в данных и почему выбран именно такой шаг. В expectation — что должно измениться и какой признак покажет, что шаг неудачен.";

/// Предложение модели в сыром виде, до обрезки границами.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Proposal {
    pub core_offset_mhz: i32,
    pub mem_offset_mhz: i32,
    pub power_percent: f32,
    #[serde(default)]
    pub fan_level: Option<u32>,
    pub reasoning: String,
    pub expectation: String,
    /// Подбор закончен: дальше рост не оправдывает риск.
    pub stop: bool,
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Что вернуть в интерфейс: и предложение модели, и то, во что оно превратилось
/// после обрезки. Расхождение между ними полезно видеть.
#[derive(Serialize, Clone, Debug)]
pub struct Suggestion {
    pub proposal: Proposal,
    pub candidate: ocsafe::GpuCandidate,
    /// Предложение выходило за коридор и было урезано.
    pub clamped: bool,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

fn tool_schema() -> Value {
    json!({
        "name": "propose_step",
        "description": "Предложить следующий шаг подбора разгона видеокарты.",
        "input_schema": {
            "type": "object",
            "properties": {
                "core_offset_mhz": { "type": "integer", "description": "Смещение частоты ядра в МГц относительно штатной." },
                "mem_offset_mhz": { "type": "integer", "description": "Смещение частоты памяти в МГц." },
                "power_percent": { "type": "number", "description": "Лимит мощности в процентах от штатного." },
                "fan_level": { "type": ["integer", "null"], "description": "Уровень вентиляторов в процентах; null — оставить автоматике драйвера." },
                "reasoning": { "type": "string", "description": "Почему выбран именно этот шаг, по-русски и коротко." },
                "expectation": { "type": "string", "description": "Что должно измениться и какой признак укажет на неудачу." },
                "stop": { "type": "boolean", "description": "true — подбор закончен, дальнейший рост не оправдывает риск." },
                "confidence": { "type": "string", "enum": ["low", "medium", "high"] }
            },
            "required": ["core_offset_mhz", "mem_offset_mhz", "power_percent", "reasoning", "expectation", "stop"]
        }
    })
}

/// Собирает всё, что модели нужно знать для следующего шага.
fn context() -> Value {
    let journal = ocsafe::load();
    let bounds = journal.bounds.sanitized();

    json!({
        "видеокарта": nvapi::telemetry(),
        "возможности": nvapi::capabilities(),
        "коридор_значений": bounds,
        "температурный_потолок_c": ocsafe::TEMP_LIMIT_C,
        "последняя_проверенная_настройка": journal.last_known_good,
        "идёт_проверка": journal.pending,
        "отклонённые_комбинации": journal.rejected,
        // Хвоста журнала достаточно: полная история только зашумляет контекст.
        "журнал": journal.history.iter().rev().take(25).collect::<Vec<_>>(),
    })
}

/// Спрашивает у модели следующий шаг и сразу приводит его в допустимый коридор.
pub fn propose(cfg: &Config, note: &str) -> Result<Suggestion> {
    if cfg.anthropic_api_key.trim().is_empty() {
        return Err(anyhow!("Не задан API-ключ Anthropic. Открой «Настройки» и вставь ключ."));
    }

    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(300));
    if !cfg.ai_proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(cfg.ai_proxy.trim())?);
    }
    let client = builder.build()?;

    let user_text = format!(
        "{}\n\nСостояние подбора:\n<состояние>\n{}\n</состояние>\n\nПредложи следующий шаг через инструмент propose_step.",
        if note.trim().is_empty() {
            "Продолжи подбор разгона видеокарты."
        } else {
            note.trim()
        },
        serde_json::to_string_pretty(&context())?
    );

    let body = json!({
        "model": cfg.ai_model,
        "max_tokens": 4000,
        "system": SYSTEM_PROMPT,
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": cfg.ai_effort },
        "fallbacks": "default",
        "tools": [tool_schema()],
        // Ответ обязан прийти вызовом инструмента: свободный текст здесь не нужен.
        "tool_choice": { "type": "tool", "name": "propose_step" },
        "messages": [ { "role": "user", "content": user_text } ]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", cfg.anthropic_api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "server-side-fallback-2026-07-01")
        .json(&body)
        .send()?;

    let status = resp.status();
    let text = resp.text()?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or(&text)
            .to_string();
        return Err(anyhow!("API {}: {}", status.as_u16(), msg));
    }
    if v.get("stop_reason").and_then(|s| s.as_str()) == Some("refusal") {
        let why = v
            .pointer("/stop_details/explanation")
            .and_then(|s| s.as_str())
            .unwrap_or("запрос отклонён фильтром безопасности");
        return Err(anyhow!("Claude отказался отвечать: {why}"));
    }

    let input = v
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|blocks| {
            blocks.iter().find(|b| {
                b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    && b.get("name").and_then(|n| n.as_str()) == Some("propose_step")
            })
        })
        .and_then(|b| b.get("input").cloned())
        .ok_or_else(|| anyhow!("Модель не вызвала инструмент propose_step. Ответ: {text}"))?;

    let proposal: Proposal = serde_json::from_value(input)
        .map_err(|e| anyhow!("Не удалось разобрать предложение модели: {e}"))?;

    // Здесь заканчивается доверие к модели и начинается арифметика.
    let journal = ocsafe::load();
    let requested = ocsafe::GpuCandidate {
        core_offset_mhz: proposal.core_offset_mhz,
        mem_offset_mhz: proposal.mem_offset_mhz,
        power_percent: proposal.power_percent,
        fan_level: proposal.fan_level,
    };
    let candidate = journal.bounds.sanitized().clamp(requested);

    let usage = v.get("usage").cloned().unwrap_or(Value::Null);
    Ok(Suggestion {
        proposal,
        clamped: candidate != requested,
        candidate,
        model: v.get("model").and_then(|m| m.as_str()).unwrap_or(&cfg.ai_model).to_string(),
        input_tokens: usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
        output_tokens: usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    /// Контекст и схема инструмента должны собираться без паник и содержать
    /// то, на что опирается системный промпт.
    /// `cargo test --lib ocloop -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn context_shape() {
        let c = super::context();
        let text = serde_json::to_string_pretty(&c).unwrap();
        println!("--- контекст ({} символов) ---\n{}", text.chars().count(), text);

        for key in ["видеокарта", "коридор_значений", "отклонённые_комбинации", "температурный_потолок_c"] {
            assert!(c.get(key).is_some(), "в контексте нет ключа {key}");
        }

        let t = super::tool_schema();
        assert_eq!(t["name"], "propose_step");
        let req = t["input_schema"]["required"].as_array().unwrap();
        assert!(req.iter().any(|v| v == "reasoning"), "reasoning должен быть обязательным");
        println!("--- схема инструмента ---\n{}", serde_json::to_string_pretty(&t).unwrap());
    }

    /// Без ключа петля обязана останавливаться понятным сообщением, а не паникой.
    #[test]
    #[ignore]
    fn no_key_is_friendly() {
        let cfg = crate::config::Config::default();
        let e = super::propose(&cfg, "").unwrap_err().to_string();
        println!("без ключа: {e}");
        assert!(e.contains("API-ключ"), "сообщение должно объяснять, чего не хватает");
    }
}
