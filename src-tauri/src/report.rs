//! "Лог для ИИ": a Markdown report of the session — ping incidents correlated with what the PC was doing.
use crate::ping::{Sample, TargetSeries};
use crate::{AppState, ContextEntry};
use chrono::{DateTime, Local};

fn t(ts: i64) -> String {
    DateTime::from_timestamp_millis(ts).map(|d| d.with_timezone(&Local).format("%H:%M:%S").to_string()).unwrap_or_default()
}
fn td(ts: i64) -> String {
    DateTime::from_timestamp_millis(ts).map(|d| d.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub struct Incident {
    pub target: String,
    pub start: i64,
    pub end: i64,
    pub kind: String, // "потери" | "пинг"
    pub detail: String,
}

fn median(v: &mut Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Incidents: ≥2 losses in a 5-sample window, or RTT above max(2×median, median+40) for ≥3 consecutive samples.
pub fn incidents(series: &TargetSeries) -> Vec<Incident> {
    let s: &Vec<Sample> = &series.samples;
    if s.len() < 10 {
        return vec![];
    }
    let mut ok: Vec<f32> = s.iter().filter_map(|x| x.rtt).collect();
    let med = median(&mut ok);
    let thr = (2.0 * med).max(med + 40.0);
    let mut out: Vec<Incident> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let win_end = (i + 5).min(s.len());
        let losses = s[i..win_end].iter().filter(|x| x.rtt.is_none()).count();
        let high = s[i..(i + 3).min(s.len())].iter().all(|x| x.rtt.map(|r| r > thr).unwrap_or(false));
        if losses >= 2 || high {
            let start = s[i].t;
            let mut j = i;
            let mut lost = 0usize;
            let mut worst = 0f32;
            let mut n = 0usize;
            // extend while the condition keeps holding
            while j < s.len() {
                let we = (j + 5).min(s.len());
                let l = s[j..we].iter().filter(|x| x.rtt.is_none()).count();
                let h = s[j].rtt.map(|r| r > thr).unwrap_or(false);
                if l < 2 && !h && j > i + 2 {
                    break;
                }
                if s[j].rtt.is_none() {
                    lost += 1;
                }
                if let Some(r) = s[j].rtt {
                    worst = worst.max(r);
                }
                n += 1;
                j += 1;
            }
            let end = s[(j.max(i + 1) - 1).min(s.len() - 1)].t;
            let kind = if lost * 2 >= n.max(1) { "потери" } else { "пинг" };
            out.push(Incident {
                target: series.name.clone(),
                start,
                end,
                kind: kind.into(),
                detail: format!("{} с, потеряно {} из {}, худший пинг {:.0} мс (медиана {:.0} мс)", n, lost, n, worst, med),
            });
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out.truncate(60);
    out
}

fn nearest_ctx<'a>(ctx: &'a [ContextEntry], ts: i64) -> Option<&'a ContextEntry> {
    ctx.iter().min_by_key(|c| (c.ts - ts).abs()).filter(|c| (c.ts - ts).abs() < 45_000)
}

pub fn build(st: &AppState) -> String {
    let snap = st.snap.lock().unwrap().clone();
    let cfg = st.cfg();
    let series = st.ping.all_samples();
    let ctx: Vec<ContextEntry> = st.context.lock().unwrap().iter().cloned().collect();
    let events = st.log.lock().unwrap().clone();
    let first_ts = series.iter().filter_map(|s| s.samples.first().map(|x| x.t)).min().unwrap_or(snap.ts);
    let mut md = String::new();
    md.push_str(&format!("# DotPilot: лог сети {} → {}\n\n", td(first_ts), td(snap.ts)));
    md.push_str("## Инструкция для ИИ\n\nТы сетевой инженер. Ниже данные с игрового/рабочего ПК на Windows 11 (Ryzen 9 7900X, RTX 5070 Ti, Wi-Fi 7 Realtek 8922AE). Пользователь играет в Warface/Minecraft, работает в Claude Code и Docker, использует Happ VPN (sing-box per-app), zapret (winws) и Radmin VPN. ");
    md.push_str("Задача: по инцидентам пинга/потерь и контексту (что было запущено, трафик, CPU) объяснить причину каждого инцидента, сказать, всё ли в порядке, и дать конкретные шаги (команды PowerShell, настройки роутера/приложений), чтобы в играх не было потерь при параллельной работе. Не выдумывай данных, которых нет.\n\n");

    md.push_str("## Итог по пингу за период\n\n| Цель | Хост | Среднее | Мин | Макс | Джиттер | Потери | Замеров | Инцидентов |\n|---|---|---|---|---|---|---|---|---|\n");
    let mut all_inc: Vec<Incident> = Vec::new();
    for s in &series {
        let ok: Vec<f32> = s.samples.iter().filter_map(|x| x.rtt).collect();
        let n = s.samples.len();
        let lost = n - ok.len();
        let avg = if ok.is_empty() { 0.0 } else { ok.iter().sum::<f32>() / ok.len() as f32 };
        let min = ok.iter().cloned().fold(f32::MAX, f32::min);
        let max = ok.iter().cloned().fold(0.0, f32::max);
        let jit = if ok.len() > 1 { ok.windows(2).map(|w| (w[0] - w[1]).abs()).sum::<f32>() / (ok.len() - 1) as f32 } else { 0.0 };
        let inc = incidents(s);
        md.push_str(&format!(
            "| {} | {} | {:.0} мс | {:.0} | {:.0} | {:.1} мс | {:.2}% ({}) | {} | {} |\n",
            s.name,
            s.host,
            avg,
            if ok.is_empty() { 0.0 } else { min },
            max,
            jit,
            if n > 0 { lost as f32 * 100.0 / n as f32 } else { 0.0 },
            lost,
            n,
            inc.len()
        ));
        all_inc.extend(inc);
    }
    all_inc.sort_by_key(|i| i.start);

    md.push_str("\n## Инциденты (потери или всплески пинга) и что происходило в этот момент\n\n");
    if all_inc.is_empty() {
        md.push_str("Инцидентов не зафиксировано: потерь и всплесков выше 2× медианы не было.\n");
    }
    for inc in all_inc.iter().take(60) {
        md.push_str(&format!("### {}–{} · {} · {}\n{}\n", t(inc.start), t(inc.end), inc.target, inc.kind, inc.detail));
        if let Some(c) = nearest_ctx(&ctx, inc.start) {
            md.push_str(&format!(
                "- Контекст ({}): CPU {:.0}%, RAM {:.0}%, трафик {:.1} Мбит/с ↓ {:.1} ↑, Wi-Fi линк {}, игровой режим {}, Happ {}, zapret {}\n- Запущено из списка: {}\n- Топ процессов по CPU: {}\n- Пинг за 30 с: {}\n\n",
                t(c.ts),
                c.cpu,
                c.mem_pct,
                c.rx_mbps,
                c.tx_mbps,
                c.wifi_link,
                if c.game_mode { "вкл" } else { "выкл" },
                c.happ,
                if c.zapret { "вкл" } else { "выкл" },
                if c.running.is_empty() { "ничего".into() } else { c.running.join(", ") },
                c.top.iter().map(|(n, v)| format!("{n} {v:.0}%")).collect::<Vec<_>>().join(", "),
                c.ping.iter().map(|(n, avg, loss)| format!("{n} {}/{loss:.0}%", avg.map(|a| format!("{a:.0} мс")).unwrap_or("—".into()))).collect::<Vec<_>>().join(", ")
            ));
        } else {
            md.push_str("- Контекст для этого момента не записан.\n\n");
        }
    }

    md.push_str("## Контекст по времени (каждые 30 с, последние 2 часа)\n\n| Время | CPU | RAM | ↓ Мбит/с | ↑ Мбит/с | Wi-Fi | Игра | Happ | zapret | Запущено | Пинг (ср/потери) |\n|---|---|---|---|---|---|---|---|---|---|---|\n");
    for c in ctx.iter().rev().take(240).collect::<Vec<_>>().into_iter().rev() {
        md.push_str(&format!(
            "| {} | {:.0}% | {:.0}% | {:.1} | {:.1} | {} | {} | {} | {} | {} | {} |\n",
            t(c.ts),
            c.cpu,
            c.mem_pct,
            c.rx_mbps,
            c.tx_mbps,
            c.wifi_link,
            if c.game_mode { "вкл" } else { "—" },
            c.happ,
            if c.zapret { "вкл" } else { "—" },
            c.running.join(", "),
            c.ping.iter().map(|(n, avg, loss)| format!("{}: {}/{loss:.0}%", n.chars().take(6).collect::<String>(), avg.map(|a| format!("{a:.0}")).unwrap_or("—".into()))).collect::<Vec<_>>().join(" ")
        ));
    }

    md.push_str("\n## Состояние сети на момент выгрузки\n\n");
    md.push_str(&format!("- Права администратора: {} · игровой режим: {} ({})\n", snap.admin, snap.game_mode.active, snap.game_mode.trigger));
    md.push_str(&format!("- Маршрут по умолчанию через: {} · системный прокси: {} {}\n", snap.default_via, snap.system_proxy_enabled, snap.system_proxy));
    md.push_str("\n### Адаптеры\n\n| Имя | Роль | Статус | IPv4 | Линк | Метрика | DNS |\n|---|---|---|---|---|---|---|\n");
    for a in snap.adapters.iter().filter(|a| a.status != "Not Present") {
        md.push_str(&format!("| {} | {} | {} | {} | {} | {} | {} |\n", a.name, a.role, a.status, a.ipv4.join(" "), a.link_speed, a.metric.map(|m| m.to_string()).unwrap_or("авто".into()), a.dns.join(" ")));
    }
    md.push_str("\n### Wi-Fi\n\n");
    if snap.wifi.iter().any(|(k, _)| k == "__error__") {
        md.push_str("Данные Wi-Fi недоступны: Windows требует разрешение «Расположение».\n");
    } else {
        for (k, v) in &snap.wifi {
            md.push_str(&format!("- {k}: {v}\n"));
        }
    }
    md.push_str("\n### Службы и туннели\n\n");
    for s in &snap.services {
        md.push_str(&format!("- {}: служба {} · процесс {} · режим {} · {}\n", s.name, s.service_state, if s.gui_running { "работает" } else { "нет" }, s.mode, s.detail));
    }
    md.push_str(&format!("- Happ config: TUN {} · strict_route {} · final {} · в туннеле: {}\n", snap.happ.tun_enabled, snap.happ.strict_route, snap.happ.final_outbound, snap.happ.proxied_paths.join(", ")));
    md.push_str("\n### Политики приложений DotPilot\n\n| Приложение | Канал | DSCP | Лимит в игре | Фоновое | Запущено | В списке Happ | QoS-правил |\n|---|---|---|---|---|---|---|---|\n");
    for a in &cfg.apps {
        let s = snap.apps.iter().find(|x| x.id == a.id);
        md.push_str(&format!(
            "| {} | {:?} | {} | {} | {} | {} | {} | {} |\n",
            a.name,
            a.policy,
            a.dscp.map(|d| d.to_string()).unwrap_or("—".into()),
            a.throttle_mbps.map(|t| format!("{t} Мбит/с")).unwrap_or("—".into()),
            a.background,
            s.map(|x| x.running).unwrap_or(false),
            s.map(|x| x.in_happ_list).unwrap_or(false),
            s.map(|x| x.qos_rules).unwrap_or(0)
        ));
    }
    md.push_str("\n### QoS-правила Windows\n\n");
    if snap.qos.is_empty() {
        md.push_str("нет\n");
    }
    for q in &snap.qos {
        md.push_str(&format!("- {} → {} · DSCP {:?} · throttle {:?}\n", q.name, q.app_path_name_match_condition.clone().unwrap_or_default(), q.dscp_action, q.throttle_rate_action_bits_per_second));
    }
    md.push_str("\n### Маршруты (первые 25)\n\n");
    for r in snap.routes.iter().filter(|r| !r.prefix.starts_with("224.") && !r.prefix.starts_with("255.") && !r.prefix.starts_with("127.")).take(25) {
        md.push_str(&format!("- {} via {} on {} metric {}+{}\n", r.prefix, r.next_hop, r.interface, r.route_metric, r.interface_metric));
    }
    md.push_str("\n## Журнал DotPilot\n\n");
    for e in events.iter().rev().take(80).collect::<Vec<_>>().into_iter().rev() {
        md.push_str(&format!("- {} [{}] {}\n", t(e.ts), e.level, e.msg));
    }
    md.push_str(&format!("\n_Отчёт создан DotPilot {}; собственная нагрузка приложения {:.1}% CPU, {} МБ._\n", td(snap.ts), snap.self_stats.cpu_total_pct, snap.self_stats.mem_mb + snap.self_stats.webview_mem_mb));
    md
}

// --- отчёт о состоянии железа ----------------------------------------------

/// Экранирование для вставки в HTML: имена дисков и моделей приходят из системы,
/// и в них теоретически может оказаться угловая скобка.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Самодостаточный HTML-отчёт о состоянии железа.
///
/// Задуман для продажи или передачи техники: покупателю нужно доказательство,
/// продавцу — обоснование цены. Файл не тянет ничего извне, поэтому открывается
/// на любой машине и печатается в PDF средствами браузера.
///
/// Отчёт намеренно не называет себя сертификатом: это снимок показаний машины
/// в конкретный момент, снятый программой, которую запускал сам владелец.
pub fn hardware_html(cpu_load: f32) -> String {
    let fw = crate::platform::firmware_info();
    let mem = crate::platform::memory_config();
    let gpu = crate::nvapi::telemetry();
    let health = crate::health::check(cpu_load);
    let now = chrono::Local::now().format("%d.%m.%Y %H:%M").to_string();

    let mut findings = String::new();
    for f in &health.findings {
        let (label, color) = match f.severity {
            crate::health::Severity::Problem => ("требует внимания", "#c0392b"),
            crate::health::Severity::Warning => ("можно улучшить", "#b7791f"),
            crate::health::Severity::Notice => ("к сведению", "#2b6cb0"),
            crate::health::Severity::Ok => ("в норме", "#276749"),
        };
        findings.push_str(&format!(
            "<tr><td style=\"white-space:nowrap\"><span class=\"tag\" style=\"background:{color}\">{label}</span></td>\
             <td><b>{}</b> — {}<div class=\"muted\">{}</div></td><td>{}</td></tr>",
            esc(&f.area),
            esc(&f.title),
            esc(&f.reference),
            esc(&f.measured)
        ));
    }

    let mem_rows: String = mem
        .modules
        .iter()
        .map(|m| {
            format!(
                "<tr><td>{}</td><td>{:.0} ГБ</td><td>{} МТ/с</td><td>{:.3} В</td></tr>",
                esc(&m.bank),
                m.capacity_gb,
                m.configured_mts,
                m.configured_mv as f32 / 1000.0
            )
        })
        .collect();

    let gpu_row = match &gpu {
        Some(g) => format!(
            "<tr><th>Видеокарта</th><td>{} · сейчас {} °C · лимит мощности {:.0} % от штатного</td></tr>",
            esc(&g.name),
            g.temperatures.iter().find(|t| t.target == "gpu").map(|t| t.current_c).unwrap_or(0),
            g.power_current_percent.unwrap_or(100.0)
        ),
        None => "<tr><th>Видеокарта</th><td>NVIDIA не обнаружена</td></tr>".to_string(),
    };

    format!(
        r#"<!doctype html>
<html lang="ru"><head><meta charset="utf-8">
<title>Состояние ПК — {now}</title>
<style>
 body {{ font: 13px/1.55 "Segoe UI", system-ui, sans-serif; color:#1a202c; max-width:900px; margin:24px auto; padding:0 20px; }}
 h1 {{ font-size:22px; margin:0 0 4px; }}
 h2 {{ font-size:15px; margin:24px 0 8px; border-bottom:1px solid #e2e8f0; padding-bottom:4px; }}
 table {{ border-collapse:collapse; width:100%; }}
 td, th {{ border-bottom:1px solid #edf2f7; padding:6px 8px; text-align:left; vertical-align:top; }}
 th {{ width:180px; color:#4a5568; font-weight:600; }}
 .muted {{ color:#718096; font-size:11.5px; margin-top:2px; }}
 .tag {{ color:#fff; border-radius:3px; padding:1px 6px; font-size:11px; }}
 .note {{ background:#f7fafc; border-left:3px solid #cbd5e0; padding:10px 12px; color:#4a5568; font-size:12px; margin-top:20px; }}
 @media print {{ body {{ margin:0; }} h2 {{ page-break-after:avoid; }} tr {{ page-break-inside:avoid; }} }}
</style></head><body>
<h1>Состояние ПК</h1>
<div class="muted">Снято {now} · {summary}</div>

<h2>Состав</h2>
<table>
<tr><th>Процессор</th><td>{cpu} — {cores} ядер / {threads} потоков</td></tr>
{gpu_row}
<tr><th>Плата</th><td>{board}</td></tr>
<tr><th>Прошивка</th><td>{bios_vendor} {bios_version} от {bios_date}</td></tr>
<tr><th>Память</th><td>{mem_total:.0} ГБ · профиль {profile}</td></tr>
</table>

<h2>Модули памяти</h2>
<table><tr><th>Слот</th><th>Объём</th><th>Частота</th><th>Напряжение</th></tr>{mem_rows}</table>

<h2>Проверка состояния</h2>
<table>{findings}</table>

<div class="note">
Это снимок показаний машины на указанный момент, снятый приложением DotPilot, а не сертификат
и не результат независимой экспертизы. Данные о накопителях взяты из SMART, сведения об
аппаратных ошибках — из журнала Windows за последние 30 дней. Проверить их может любой
покупатель на этой же машине.
</div>
</body></html>"#,
        now = esc(&now),
        summary = esc(&health.summary),
        cpu = esc(&fw.cpu),
        cores = fw.cores,
        threads = fw.threads,
        gpu_row = gpu_row,
        board = esc(&fw.board),
        bios_vendor = esc(&fw.bios_vendor),
        bios_version = esc(&fw.bios_version),
        bios_date = esc(&fw.bios_date),
        mem_total = mem.total_gb,
        profile = if mem.profile_enabled { "включён" } else { "выключен (базовый JEDEC)" },
        mem_rows = mem_rows,
        findings = findings,
    )
}

#[cfg(test)]
mod hw_tests {
    /// Живая сборка отчёта: `cargo test --lib report::hw_tests -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn builds_on_this_machine() {
        let html = super::hardware_html(10.0);
        let out = std::env::temp_dir().join("dotpilot-hw-report.html");
        std::fs::write(&out, &html).unwrap();
        println!("отчёт: {} символов -> {}", html.chars().count(), out.display());

        assert!(html.starts_with("<!doctype html>"), "не похоже на HTML");
        assert!(html.contains("</html>"), "документ не закрыт");
        for must in ["Состав", "Модули памяти", "Проверка состояния", "не сертификат"] {
            assert!(html.contains(must), "в отчёте нет раздела «{must}»");
        }
        // Внешних ссылок быть не должно: файл обязан открываться без сети.
        assert!(!html.contains("http://"), "во внешнем файле не должно быть внешних ссылок");
        assert!(!html.contains("src=\""), "во внешнем файле не должно быть подгружаемых ресурсов");
    }
}
