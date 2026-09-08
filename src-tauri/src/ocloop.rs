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
use base64::Engine;
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
    parse_suggestion(&v, &cfg.ai_model, ocsafe::load().bounds)
}

/// Разбор ответа API и приведение предложения в допустимый коридор.
///
/// Вынесено из `propose` намеренно: это самая хрупкая часть — форма ответа,
/// извлечение блока tool_use и обрезка границами, — и её нужно проверять
/// тестами, а не только живыми запросами.
fn parse_suggestion(v: &Value, fallback_model: &str, bounds: ocsafe::Bounds) -> Result<Suggestion> {
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
        .ok_or_else(|| anyhow!("Модель не вызвала инструмент propose_step."))?;

    let proposal: Proposal = serde_json::from_value(input)
        .map_err(|e| anyhow!("Не удалось разобрать предложение модели: {e}"))?;

    // Здесь заканчивается доверие к модели и начинается арифметика.
    let requested = ocsafe::GpuCandidate {
        core_offset_mhz: proposal.core_offset_mhz,
        mem_offset_mhz: proposal.mem_offset_mhz,
        power_percent: proposal.power_percent,
        fan_level: proposal.fan_level,
    };
    let candidate = bounds.clamp(requested);

    let usage = v.get("usage").cloned().unwrap_or(Value::Null);
    Ok(Suggestion {
        proposal,
        clamped: candidate != requested,
        candidate,
        model: v.get("model").and_then(|m| m.as_str()).unwrap_or(fallback_model).to_string(),
        input_tokens: usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
        output_tokens: usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

// --- второе мнение по состоянию ПК -----------------------------------------

const HEALTH_SYSTEM_PROMPT: &str = "Ты — инженер по железу ПК. Тебе дают результат автоматической проверки: что измерено и с какой нормой сравнено.

Твоя задача — дать второе мнение и контекст, которого нет в жёстко зашитых порогах:

- Подтверди или оспорь выводы. Если порог в программе слишком строгий или слишком мягкий для этой конкретной модели — скажи прямо.
- Сравни с тем, что обычно у владельцев такой же связки: та же модель процессора, видеокарты, платы.
- Расставь приоритеты: что действительно стоит делать, а что можно спокойно игнорировать.
- Назови то, чего проверка не увидела, но что стоит посмотреть при таких показаниях.

Не пересказывай измерения обратно — владелец их уже видит. Пиши по-русски, коротко, без списков ради списков. Используй Markdown. Не выдумывай данных, которых нет в отчёте.";

/// Свободный комментарий модели к отчёту о состоянии.
///
/// Здесь намеренно нет tool-use: от модели нужен связный разбор, а не структура
/// для машины, и любые её выводы носят рекомендательный характер — ничего
/// автоматически не применяется.
pub fn advise_health(cfg: &Config, report: &Value) -> Result<String> {
    if cfg.anthropic_api_key.trim().is_empty() {
        return Err(anyhow!("Не задан API-ключ Anthropic. Открой «Настройки» и вставь ключ."));
    }
    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(300));
    if !cfg.ai_proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(cfg.ai_proxy.trim())?);
    }
    let client = builder.build()?;

    let ctx = json!({
        "прошивка": crate::platform::firmware_info(),
        "память": crate::platform::memory_config(),
        "видеокарта": crate::nvapi::telemetry(),
        "отчёт_проверки": report,
    });

    let body = json!({
        "model": cfg.ai_model,
        "max_tokens": 4000,
        "system": HEALTH_SYSTEM_PROMPT,
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": cfg.ai_effort },
        "fallbacks": "default",
        "messages": [ { "role": "user", "content":
            format!("Вот результат проверки моего ПК. Дай второе мнение.\n\n<отчёт>\n{}\n</отчёт>",
                    serde_json::to_string_pretty(&ctx)?) } ]
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
        let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or(&text).to_string();
        return Err(anyhow!("API {}: {}", status.as_u16(), msg));
    }
    let mut out = String::new();
    if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
        for b in blocks {
            if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
        }
    }
    if out.trim().is_empty() {
        return Err(anyhow!("Пустой ответ от API."));
    }
    Ok(out)
}

// --- чтение экрана BIOS с фотографии ---------------------------------------

const PHOTO_SYSTEM_PROMPT: &str = "Ты читаешь фотографию экрана BIOS и перечисляешь то, что на ней видно.

Главное правило: **не додумывай**. Если значение нечитаемо — смазано, засвечено, обрезано краем кадра или закрыто бликом, — так и скажи в problems, а в settings его не вписывай. Владелец будет опираться на твой ответ, решая, применена настройка или нет; выдуманное значение здесь хуже, чем признание «не видно».

Не описывай интерфейс и не пересказывай подписи целиком. Нужны пары «параметр — значение» в том виде, как они названы на экране.

Если тебе сказано, какие значения ожидались, сравни увиденное с ожиданием и скажи в verdict, совпало ли, и что именно расходится. Если ожидание не задано — просто перечисли увиденное.

Отвечай по-русски.";

/// Одна пара «параметр — значение», прочитанная с экрана.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SeenSetting {
    pub name: String,
    pub value: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BiosReading {
    pub settings: Vec<SeenSetting>,
    /// Удалось ли вообще разобрать снимок.
    pub readable: bool,
    /// Что помешало прочитать: блики, смазанность, обрезанный кадр.
    #[serde(default)]
    pub problems: Vec<String>,
    /// Сравнение с ожидаемыми значениями, если они были заданы.
    pub verdict: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct BiosPhotoResult {
    pub reading: BiosReading,
    /// Во что превратилось фото перед отправкой.
    pub sent_pixels: String,
    pub sent_kb: usize,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Готовит фотографию к отправке: уменьшает и перекодирует в JPEG.
///
/// Снимок с телефона легко занимает восемь мегабайт и три тысячи пикселей по
/// стороне. Для чтения текста с экрана этого избыточно, а ограничение API на
/// размер изображения такой файл превышает. Полторы тысячи пикселей достаточно,
/// чтобы прочитать строки меню, и укладывается в лимит с большим запасом.
fn prepare_photo(path: &str) -> Result<(String, u32, u32, usize)> {
    const MAX_SIDE: u32 = 1600;

    let img = image::open(path).map_err(|e| anyhow!("Не удалось открыть изображение: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let img = if w.max(h) > MAX_SIDE {
        let scale = MAX_SIDE as f32 / w.max(h) as f32;
        img.resize(
            ((w as f32) * scale) as u32,
            ((h as f32) * scale) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let (out_w, out_h) = (img.width(), img.height());

    let mut buf: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 82);
    let rgb = img.to_rgb8();
    encoder
        .encode(rgb.as_raw(), out_w, out_h, image::ExtendedColorType::Rgb8)
        .map_err(|e| anyhow!("Не удалось перекодировать изображение: {e}"))?;

    let kb = buf.len() / 1024;
    let data = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok((data, out_w, out_h, kb))
}

fn photo_tool_schema() -> Value {
    json!({
        "name": "read_bios_screen",
        "description": "Перечислить настройки, видимые на фотографии экрана BIOS.",
        "input_schema": {
            "type": "object",
            "properties": {
                "settings": {
                    "type": "array",
                    "description": "Только те пары, которые действительно читаются на снимке.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Название параметра, как на экране." },
                            "value": { "type": "string", "description": "Значение, как на экране." }
                        },
                        "required": ["name", "value"]
                    }
                },
                "readable": { "type": "boolean", "description": "Удалось ли разобрать снимок вообще." },
                "problems": {
                    "type": "array",
                    "description": "Что помешало прочитать: блики, смазанность, обрезанный кадр.",
                    "items": { "type": "string" }
                },
                "verdict": { "type": "string", "description": "Сравнение с ожидаемыми значениями либо краткая сводка увиденного." }
            },
            "required": ["settings", "readable", "verdict"]
        }
    })
}

/// Читает настройки с фотографии экрана BIOS и сравнивает с ожиданием.
pub fn read_bios_photo(cfg: &Config, path: &str, expectation: &str) -> Result<BiosPhotoResult> {
    if cfg.anthropic_api_key.trim().is_empty() {
        return Err(anyhow!("Не задан API-ключ Anthropic. Открой «Настройки» и вставь ключ."));
    }
    let (data, w, h, kb) = prepare_photo(path)?;

    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(300));
    if !cfg.ai_proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(cfg.ai_proxy.trim())?);
    }
    let client = builder.build()?;

    let ask = if expectation.trim().is_empty() {
        "Перечисли настройки, которые видно на этом экране BIOS.".to_string()
    } else {
        format!(
            "Перечисли настройки, которые видно на этом экране BIOS, и сравни с ожидаемым:\n\n{}",
            expectation.trim()
        )
    };

    let body = json!({
        "model": cfg.ai_model,
        "max_tokens": 3000,
        "system": PHOTO_SYSTEM_PROMPT,
        "output_config": { "effort": cfg.ai_effort },
        "fallbacks": "default",
        "tools": [photo_tool_schema()],
        "tool_choice": { "type": "tool", "name": "read_bios_screen" },
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": data } },
                { "type": "text", "text": ask }
            ]
        }]
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
        let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or(&text).to_string();
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
                    && b.get("name").and_then(|n| n.as_str()) == Some("read_bios_screen")
            })
        })
        .and_then(|b| b.get("input").cloned())
        .ok_or_else(|| anyhow!("Модель не вызвала инструмент read_bios_screen."))?;

    let reading: BiosReading =
        serde_json::from_value(input).map_err(|e| anyhow!("Не удалось разобрать ответ модели: {e}"))?;
    let usage = v.get("usage").cloned().unwrap_or(Value::Null);

    Ok(BiosPhotoResult {
        reading,
        sent_pixels: format!("{w}×{h}"),
        sent_kb: kb,
        model: v.get("model").and_then(|m| m.as_str()).unwrap_or(&cfg.ai_model).to_string(),
        input_tokens: usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
        output_tokens: usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

// --- советник по BIOS ------------------------------------------------------

const BIOS_SYSTEM_PROMPT: &str = "Ты — инженер по настройке платформ AMD AM5. Владелец собирается менять параметры в BIOS сам, а приложение снимает замеры до и после и проверяет стабильность встроенными тестами.

Ты не можешь ничего применить — ты даёшь точные значения и путь в меню. Поэтому пиши так, чтобы человек нашёл пункт и ввёл число, не догадываясь.

Порядок, которого держись:

1. Профиль памяти EXPO — первое, что нужно включить, если он выключен. Это не разгон, а штатный профиль модулей, и на Zen 4 он даёт больше, чем любая правка частот, особенно в 1% low.
2. Curve Optimizer с отрицательным смещением — главный рычаг для цели «холоднее и быстрее одновременно»: ниже напряжение означает выше буст, ниже температуру и меньше деградацию. Начинай с умеренного общего смещения, а не с предельного.
3. Лимиты PBO (PPT, TDC, EDC) — только после того, как Curve Optimizer устоялся.
4. Понижение предельной температуры имеет смысл, если владельца беспокоит нагрев, но за него платят частотой — говори об этом прямо.

Правила:

- Один параметр за раз. После каждого изменения владелец перезагружается и снимает замер.
- Никогда не предлагай ничего, что требует физического сброса CMOS для восстановления, без явного предупреждения, как его сделать.
- Признаки нестабильности Curve Optimizer называй конкретно: вылеты в простое, а не под нагрузкой; ошибки в тесте памяти; WHEA в журнале.
- Если данных для совета не хватает, скажи, какой замер снять, вместо того чтобы гадать.

Отвечай по-русски, коротко и по делу.";

/// Один конкретный пункт BIOS.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BiosSetting {
    /// Путь в меню, например «Tweaker → Advanced Memory Settings → Extreme Memory Profile».
    pub path: String,
    pub value: String,
    pub why: String,
    /// Чем рискуем и как откатить.
    pub risk: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BiosAdvice {
    pub settings: Vec<BiosSetting>,
    /// В каком порядке применять и что делать между шагами.
    pub order: String,
    /// Что проверить после перезагрузки.
    pub verify: String,
    pub expected_gain: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BiosSuggestion {
    pub advice: BiosAdvice,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

fn bios_tool_schema() -> Value {
    json!({
        "name": "advise_bios",
        "description": "Дать точные значения для BIOS с путями в меню.",
        "input_schema": {
            "type": "object",
            "properties": {
                "settings": {
                    "type": "array",
                    "description": "Пункты BIOS, которые нужно изменить, в порядке применения.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Путь в меню BIOS." },
                            "value": { "type": "string", "description": "Что выставить." },
                            "why": { "type": "string", "description": "Зачем это нужно." },
                            "risk": { "type": "string", "description": "Чем рискуем и как откатить." }
                        },
                        "required": ["path", "value", "why", "risk"]
                    }
                },
                "order": { "type": "string", "description": "В каком порядке применять и что делать между шагами." },
                "verify": { "type": "string", "description": "Что проверить после перезагрузки." },
                "expected_gain": { "type": "string", "description": "Какого прироста ждать и в чём он проявится." },
                "warnings": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["settings", "order", "verify", "expected_gain"]
        }
    })
}

/// Просит у модели план правок BIOS на основе замеров.
pub fn advise_bios(cfg: &Config, question: &str) -> Result<BiosSuggestion> {
    if cfg.anthropic_api_key.trim().is_empty() {
        return Err(anyhow!("Не задан API-ключ Anthropic. Открой «Настройки» и вставь ключ."));
    }

    let store = crate::platform::load_baselines();
    let comparison = if store.items.len() >= 2 {
        let n = store.items.len();
        Some(crate::platform::compare(&store.items[n - 2], &store.items[n - 1]))
    } else {
        None
    };

    let ctx = json!({
        "прошивка": crate::platform::firmware_info(),
        "память": crate::platform::memory_config(),
        "замеры": store.items,
        "сравнение_двух_последних": comparison,
    });

    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(300));
    if !cfg.ai_proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(cfg.ai_proxy.trim())?);
    }
    let client = builder.build()?;

    let user_text = format!(
        "{}\n\nЗамеры приложения:\n<состояние>\n{}\n</состояние>\n\nДай план через инструмент advise_bios.",
        if question.trim().is_empty() {
            "Что изменить в BIOS, чтобы получить больше производительности при меньшем нагреве?"
        } else {
            question.trim()
        },
        serde_json::to_string_pretty(&ctx)?
    );

    let body = json!({
        "model": cfg.ai_model,
        "max_tokens": 6000,
        "system": BIOS_SYSTEM_PROMPT,
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": cfg.ai_effort },
        "fallbacks": "default",
        "tools": [bios_tool_schema()],
        "tool_choice": { "type": "tool", "name": "advise_bios" },
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
        let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or(&text).to_string();
        return Err(anyhow!("API {}: {}", status.as_u16(), msg));
    }
    parse_bios_advice(&v, &cfg.ai_model)
}

fn parse_bios_advice(v: &Value, fallback_model: &str) -> Result<BiosSuggestion> {
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
                    && b.get("name").and_then(|n| n.as_str()) == Some("advise_bios")
            })
        })
        .and_then(|b| b.get("input").cloned())
        .ok_or_else(|| anyhow!("Модель не вызвала инструмент advise_bios."))?;

    let advice: BiosAdvice =
        serde_json::from_value(input).map_err(|e| anyhow!("Не удалось разобрать совет модели: {e}"))?;
    let usage = v.get("usage").cloned().unwrap_or(Value::Null);
    Ok(BiosSuggestion {
        advice,
        model: v.get("model").and_then(|m| m.as_str()).unwrap_or(fallback_model).to_string(),
        input_tokens: usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
        output_tokens: usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

#[cfg(test)]
mod parse_tests {
    use super::*;
    use crate::ocsafe::Bounds;

    fn response(input: Value) -> Value {
        json!({
            "model": "claude-opus-5",
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1200, "output_tokens": 300 },
            "content": [
                // Блок размышления идёт первым — разбор обязан его пропустить.
                { "type": "thinking", "thinking": "…" },
                { "type": "tool_use", "id": "tu_1", "name": "propose_step", "input": input }
            ]
        })
    }

    /// Заведомо запредельное предложение обязано быть урезано, а не применено.
    #[test]
    fn clamps_absurd_proposal() {
        let v = response(json!({
            "core_offset_mhz": 5000,
            "mem_offset_mhz": -9000,
            "power_percent": 250.0,
            "fan_level": 5,
            "reasoning": "проверка обрезки",
            "expectation": "значения должны быть урезаны",
            "stop": false
        }));
        let b = Bounds::default();
        let s = parse_suggestion(&v, "запасная", b).unwrap();

        assert!(s.clamped, "предложение вышло за коридор, но не помечено как урезанное");
        assert_eq!(s.candidate.core_offset_mhz, b.sanitized().core_max_mhz);
        assert_eq!(s.candidate.mem_offset_mhz, b.sanitized().mem_min_mhz);
        assert_eq!(s.candidate.power_percent, b.sanitized().power_max_percent);
        assert_eq!(s.candidate.fan_level, Some(b.sanitized().fan_min_percent));
        // Сырое предложение сохраняется, чтобы расхождение было видно в интерфейсе.
        assert_eq!(s.proposal.core_offset_mhz, 5000);
        assert_eq!(s.model, "claude-opus-5");
        assert_eq!(s.input_tokens, 1200);
    }

    /// Предложение внутри коридора проходит без изменений.
    #[test]
    fn keeps_sane_proposal() {
        let v = response(json!({
            "core_offset_mhz": 30,
            "mem_offset_mhz": 0,
            "power_percent": 90.0,
            "fan_level": null,
            "reasoning": "шаг вверх по ядру",
            "expectation": "частота вырастет",
            "stop": false,
            "confidence": "medium"
        }));
        let s = parse_suggestion(&v, "запасная", Bounds::default()).unwrap();
        assert!(!s.clamped);
        assert_eq!(s.candidate.core_offset_mhz, 30);
        assert_eq!(s.candidate.power_percent, 90.0);
        assert_eq!(s.candidate.fan_level, None);
        assert_eq!(s.proposal.confidence.as_deref(), Some("medium"));
    }

    /// Ответ без вызова инструмента — это ошибка, а не молчаливый ноль.
    #[test]
    fn rejects_response_without_tool_call() {
        let v = json!({
            "model": "claude-opus-5",
            "content": [ { "type": "text", "text": "не могу" } ]
        });
        let e = parse_suggestion(&v, "запасная", Bounds::default()).unwrap_err().to_string();
        assert!(e.contains("propose_step"), "непонятное сообщение: {e}");
    }

    /// Отказ модели должен объясняться, а не выглядеть как пустой ответ.
    #[test]
    fn reports_refusal() {
        let v = json!({
            "stop_reason": "refusal",
            "stop_details": { "explanation": "причина" },
            "content": []
        });
        let e = parse_suggestion(&v, "запасная", Bounds::default()).unwrap_err().to_string();
        assert!(e.contains("причина"), "объяснение отказа потерялось: {e}");
    }
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

#[cfg(test)]
mod photo_tests {
    /// Снимок с телефона должен ужиматься до предсказуемого размера.
    ///
    /// Это единственная часть чтения фотографии, которую можно проверить без сети,
    /// и самая вероятная точка отказа: файл на восемь мегабайт не уложился бы в
    /// ограничение API, а без уменьшения запрос просто отклонили бы.
    #[test]
    fn large_photo_is_scaled_down() {
        // Кадр размером с телефонный: 4032 × 3024.
        let img = image::RgbImage::from_fn(4032, 3024, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 128])
        });
        let path = std::env::temp_dir().join("dotpilot-photo-test.png");
        img.save(&path).expect("не удалось сохранить тестовый снимок");
        let original_kb = std::fs::metadata(&path).unwrap().len() / 1024;

        let (data, w, h, kb) = super::prepare_photo(path.to_str().unwrap()).expect("подготовка снимка сорвалась");
        let _ = std::fs::remove_file(&path);

        println!("было {original_kb} КБ и 4032×3024, стало {kb} КБ и {w}×{h}");
        assert_eq!(w.max(h), 1600, "длинная сторона должна быть ровно 1600");
        assert_eq!(h, 1200, "пропорции должны сохраняться");
        assert!(kb < 1024, "после сжатия снимок обязан укладываться в мегабайт, получилось {kb} КБ");
        assert!(!data.is_empty(), "base64 пустой");
        // Кодирование в JPEG обязано было действительно произойти.
        assert!(data.starts_with("/9j/"), "данные не похожи на JPEG");
    }

    /// Небольшое изображение не должно растягиваться.
    #[test]
    fn small_photo_is_left_alone() {
        let img = image::RgbImage::from_fn(800, 600, |_, _| image::Rgb([10, 20, 30]));
        let path = std::env::temp_dir().join("dotpilot-photo-small.png");
        img.save(&path).unwrap();
        let (_, w, h, _) = super::prepare_photo(path.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((w, h), (800, 600), "маленький снимок увеличивать не нужно");
    }
}
