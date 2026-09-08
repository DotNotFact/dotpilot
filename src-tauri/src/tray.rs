//! System tray: live status lines + the most important actions. Closing the window hides it here.
use crate::{AppState, Snapshot};
use std::sync::Arc;
use std::thread;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "main";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, None)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("DotPilot")
        .on_menu_event(|app, event| handle_menu(app.clone(), event.id().as_ref().to_string()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn ms(v: Option<f32>) -> String {
    v.map(|x| format!("{x:.0} мс")).unwrap_or_else(|| "—".into())
}

pub fn build_menu(app: &AppHandle, snap: Option<&Snapshot>) -> tauri::Result<Menu<Wry>> {
    let st = app.try_state::<Arc<AppState>>();
    let cfg = st.as_ref().map(|s| s.cfg());
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "open", "Открыть DotPilot", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    // --- live status (disabled items act as text) ---
    let mut lines: Vec<String> = Vec::new();
    if let Some(s) = snap {
        let gw = s.ping.first();
        let inet = s.ping.get(1);
        let loss = s.ping.iter().map(|p| p.stats.loss_pct).fold(0.0f32, f32::max);
        lines.push(format!(
            "Пинг: роутер {} · интернет {} · потери {:.0}%",
            gw.map(|p| ms(p.stats.last)).unwrap_or("—".into()),
            inet.map(|p| ms(p.stats.last)).unwrap_or("—".into()),
            loss
        ));
        if let Some(w) = s.adapters.iter().find(|a| a.role == "wifi" && a.status == "Up") {
            lines.push(format!("Wi-Fi {} · {:.1} Мб/с ↓ {:.1} Мб/с ↑", w.link_speed, w.rx_bps / 1e6, w.tx_bps / 1e6));
        }
        let svc = |id: &str| s.services.iter().find(|x| x.id == id);
        lines.push(format!(
            "Happ: {} · Radmin: {} · zapret: {}",
            svc("happ").map(|x| match x.mode.as_str() { "tun" => "TUN", "proxy" => "прокси", "idle" => "ожидание", _ => "выкл" }).unwrap_or("?"),
            if s.adapters.iter().any(|a| a.role == "radmin" && a.status == "Up") { "вкл" } else { "выкл" },
            if svc("zapret").map(|x| x.gui_running).unwrap_or(false) { "вкл" } else { "выкл" }
        ));
        lines.push(format!(
            "CPU {:.0}% · RAM {:.1}/{:.0} ГБ · GPU {:.0}% {:.0}°C",
            s.cpu.usage,
            s.mem.used_mb as f32 / 1024.0,
            s.mem.total_mb as f32 / 1024.0,
            s.gpu.util_pct,
            s.gpu.temp_c
        ));
        if s.gpu.available {
            lines.push(format!("VRAM {:.1}/{:.0} ГБ · {:.0} Вт · DotPilot {:.1}% CPU, {} МБ", s.gpu.mem_used_mb / 1024.0, s.gpu.mem_total_mb / 1024.0, s.gpu.power_w, s.self_stats.cpu_total_pct, s.self_stats.mem_mb + s.self_stats.webview_mem_mb));
        }
        let running: Vec<&str> = cfg
            .as_ref()
            .map(|c| c.apps.iter().filter(|a| s.apps.iter().any(|x| x.id == a.id && x.running)).map(|a| a.name.as_str()).collect())
            .unwrap_or_default();
        if !running.is_empty() {
            lines.push(format!("Запущено: {}", running.join(", ")));
        }
    } else {
        lines.push("Сбор данных…".into());
    }
    for (i, l) in lines.iter().enumerate() {
        menu.append(&MenuItem::with_id(app, format!("status-{i}"), l, false, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    // --- actions ---
    let gm_on = snap.map(|s| s.game_mode.active).unwrap_or(false);
    menu.append(&CheckMenuItem::with_id(app, "gm", "Игровой режим (лимиты фоновых приложений)", true, gm_on, None::<&str>)?)?;
    if let Some(c) = &cfg {
        let sub = Submenu::with_id(app, "profiles", "Профиль", true)?;
        for p in &c.profiles {
            sub.append(&CheckMenuItem::with_id(app, format!("profile-{}", p.id), &p.name, true, c.active_profile == p.id, None::<&str>)?)?;
        }
        menu.append(&sub)?;
    }
    let zapret_on = snap.and_then(|s| s.services.iter().find(|x| x.id == "zapret").map(|x| x.gui_running)).unwrap_or(false);
    let happ_on = snap.and_then(|s| s.services.iter().find(|x| x.id == "happ").map(|x| x.service_state == "Running")).unwrap_or(false);
    menu.append(&CheckMenuItem::with_id(app, "zapret", "zapret (обход DPI)", true, zapret_on, None::<&str>)?)?;
    menu.append(&CheckMenuItem::with_id(app, "happ", "Happ VPN (служба)", true, happ_on, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "flushdns", "Очистить DNS-кэш", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "apply", "Применить сетевые политики", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Выход из DotPilot", true, None::<&str>)?)?;
    Ok(menu)
}

pub fn tooltip(snap: &Snapshot) -> String {
    let gw = snap.ping.first().map(|p| ms(p.stats.last)).unwrap_or("—".into());
    let inet = snap.ping.get(1).map(|p| ms(p.stats.last)).unwrap_or("—".into());
    format!(
        "DotPilot · роутер {gw} · интернет {inet} · игровой режим {}",
        if snap.game_mode.active { "вкл" } else { "выкл" }
    )
}

/// Called from the collector thread: rebuild the tray menu on the main thread.
pub fn refresh(app: &AppHandle, snap: Snapshot) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = app2.tray_by_id(TRAY_ID) {
            if let Ok(menu) = build_menu(&app2, Some(&snap)) {
                let _ = tray.set_menu(Some(menu));
            }
            let _ = tray.set_tooltip(Some(tooltip(&snap)));
        }
    });
}

fn handle_menu(app: AppHandle, id: String) {
    match id.as_str() {
        "open" => show_main(&app),
        "quit" => app.exit(0),
        _ => {
            thread::spawn(move || {
                let st = app.state::<Arc<AppState>>().inner().clone();
                let cfg = st.cfg();
                match id.as_str() {
                    "gm" => {
                        let mut g = st.game.lock().unwrap();
                        let now = g.active;
                        g.manual = Some(!now);
                        st.log("info", if now { "Трей: игровой режим выключен" } else { "Трей: игровой режим включён" });
                    }
                    "zapret" => {
                        let running = st.snap.lock().unwrap().services.iter().find(|x| x.id == "zapret").map(|x| x.gui_running).unwrap_or(false);
                        let r = if running { crate::services::zapret_stop(&cfg) } else { crate::services::zapret_start(&cfg) };
                        st.log(if r.is_ok() { "info" } else { "error" }, format!("Трей: zapret → {}", match r { Ok(s) => s.trim().to_string(), Err(e) => e.to_string() }));
                    }
                    "happ" => {
                        let running = st.snap.lock().unwrap().services.iter().find(|x| x.id == "happ").map(|x| x.service_state == "Running").unwrap_or(false);
                        let r = crate::services::set_service(&cfg.happ_service, !running);
                        st.log(if r.is_ok() { "info" } else { "error" }, format!("Трей: Happ служба → {}", match r { Ok(s) => s.trim().to_string(), Err(e) => e.to_string() }));
                    }
                    "flushdns" => {
                        let r = crate::policy::quick_action("flushdns");
                        st.log(if r.is_ok() { "info" } else { "error" }, format!("Трей: {}", match r { Ok(s) => s.trim().to_string(), Err(e) => e.to_string() }));
                    }
                    "apply" => {
                        let gm = st.game.lock().unwrap().active;
                        let rep = crate::policy::apply_all(&cfg, gm);
                        st.log(if rep.ok { "info" } else { "error" }, format!("Трей: политики применены: {}", rep.lines.first().cloned().unwrap_or_default()));
                    }
                    other => {
                        if let Some(pid) = other.strip_prefix("profile-") {
                            match crate::do_apply_profile(&st, pid) {
                                Ok(lines) => st.log("info", format!("Трей: профиль {pid}: {}", lines.join(" · "))),
                                Err(e) => st.log("error", format!("Трей: профиль {pid}: {e}")),
                            }
                        }
                    }
                }
            });
        }
    }
}
