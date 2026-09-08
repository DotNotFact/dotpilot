//! "Спросить Claude": send a telemetry snapshot + question to the Claude API (raw HTTP).
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

const SYSTEM_PROMPT: &str = "Ты — сетевой инженер и специалист по Windows 11, помогаешь владельцу игрового/рабочего ПК (Ryzen 9 7900X, RTX 5070 Ti, Wi-Fi 7 Realtek 8922AE). \
Он играет в Warface и Minecraft (по Radmin VPN с друзьями), работает в Claude Code, Docker, Flutter, пользуется Happ VPN (sing-box, per-app), zapret (winws, обход DPI) и Яндекс Браузером. \
Проблема: во время работы (Claude Code, Docker) в играх растёт пинг и появляются потери пакетов; VPN-туннели мешают друг другу. \
Ниже приложен JSON-снимок состояния сети и системы, снятый приложением DotPilot. \
Отвечай по-русски, конкретно и по делу: сначала краткий диагноз (что видно в данных), затем нумерованный список действий по приоритету с точными командами PowerShell или настройками, затем что проверить после. \
Не выдумывай данных, которых нет в снимке; если чего-то не хватает — скажи, что именно измерить. Используй Markdown.";

pub fn ask(api_key: &str, model: &str, proxy: &str, effort: &str, question: &str, snapshot: &Value) -> Result<String> {
    if api_key.trim().is_empty() {
        return Err(anyhow!("Не задан API-ключ Anthropic. Открой «Настройки» и вставь ключ."));
    }
    let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(300));
    if !proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(proxy.trim())?);
    }
    let client = builder.build()?;
    let user_text = format!(
        "{}\n\n<snapshot>\n{}\n</snapshot>",
        if question.trim().is_empty() { "Проанализируй состояние сети и скажи, что исправить, чтобы в играх не было пинга и потерь, когда параллельно работают Claude Code и Docker." } else { question.trim() },
        serde_json::to_string_pretty(snapshot)?
    );
    let body = json!({
        "model": model,
        "max_tokens": 8000,
        "system": SYSTEM_PROMPT,
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": effort },
        "fallbacks": "default",
        "messages": [ { "role": "user", "content": user_text } ]
    });
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", api_key.trim())
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
    if out.is_empty() {
        return Err(anyhow!("Пустой ответ от API: {text}"));
    }
    let served = v.get("model").and_then(|m| m.as_str()).unwrap_or(model);
    let usage = v.get("usage").cloned().unwrap_or(Value::Null);
    out.push_str(&format!(
        "\n\n---\n_модель: {} · токены: {} вход / {} выход_",
        served,
        usage.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0),
        usage.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0)
    ));
    Ok(out)
}
