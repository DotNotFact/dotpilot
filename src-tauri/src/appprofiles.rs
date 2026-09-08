//! Профили разгона по приложениям.
//!
//! Разные задачи хотят разного: игре полезен андервольт с высоким бустом, рендеру —
//! полный лимит мощности, а фоновой работе вообще ничего. Переключать это руками
//! никто не станет, поэтому профиль привязывается к приложению и применяется, когда
//! оно запускается.
//!
//! # Что здесь важнее удобства
//!
//! Автоматически применяется **только проверенный профиль**. Настройка, не прошедшая
//! длинную ступень, может уронить систему — и сделает это в момент запуска игры,
//! то есть ровно тогда, когда это больнее всего. Поэтому непроверенный профиль можно
//! применить вручную и прогнать через проверку, но автоматика его не тронет.
//!
//! Второе ограничение: пока идёт проверка другой настройки, переключение не
//! выполняется. Иначе вердикт по ступени относился бы уже не к тому, что проверялось.

use crate::ocsafe::GpuCandidate;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppProfile {
    pub id: String,
    pub name: String,
    pub candidate: GpuCandidate,
    /// Прошёл ли профиль длинную ступень проверки. Только такие применяются сами.
    pub verified: bool,
    /// Идентификаторы приложений из конфигурации, при запуске которых профиль включается.
    pub app_ids: Vec<String>,
    /// Профиль, к которому возвращаемся, когда ни одно привязанное приложение не запущено.
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProfileStore {
    pub profiles: Vec<AppProfile>,
    /// Какой профиль применён сейчас.
    pub active: Option<String>,
    /// Включено ли автоматическое переключение.
    pub auto: bool,
}

fn path() -> std::path::PathBuf {
    let mut p = crate::config::config_path();
    p.set_file_name("DotPilot.gpuprofiles.json");
    p
}

pub fn load() -> ProfileStore {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str::<ProfileStore>(&s).ok())
        .unwrap_or_else(|| ProfileStore {
            // Профиль по умолчанию — штатные значения. Он всегда проверен: это то,
            // с чем карта приехала с завода.
            profiles: vec![AppProfile {
                id: "stock".into(),
                name: "Штатный".into(),
                candidate: GpuCandidate::default(),
                verified: true,
                app_ids: vec![],
                is_default: true,
            }],
            active: None,
            auto: false,
        })
}

pub fn store(s: &ProfileStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path(), text).map_err(|e| e.to_string())
}

/// Какой профиль должен быть активен при данном наборе запущенных приложений.
///
/// Побеждает первый профиль в списке, у которого нашлось совпадение: порядок задаёт
/// приоритет, и это единственное разумное правило, когда запущены и игра, и рендер.
pub fn resolve<'a>(store: &'a ProfileStore, running: &[String]) -> Option<&'a AppProfile> {
    store
        .profiles
        .iter()
        .find(|p| !p.app_ids.is_empty() && p.app_ids.iter().any(|id| running.contains(id)))
        .or_else(|| store.profiles.iter().find(|p| p.is_default))
}

/// Причина, по которой автопереключение сейчас невозможно, либо `None`.
fn blocked(profile: &AppProfile) -> Option<String> {
    if let Some(r) = crate::anticheat::writes_blocked() {
        return Some(r);
    }
    if !profile.verified {
        return Some(format!(
            "Профиль «{}» не прошёл проверку и автоматически не применяется.",
            profile.name
        ));
    }
    if crate::ocsafe::load().pending.is_some() {
        return Some("Идёт проверка другой настройки: переключение отложено.".into());
    }
    None
}

#[derive(Serialize, Clone, Debug)]
pub struct SwitchResult {
    pub profile_id: String,
    pub profile_name: String,
    pub applied: bool,
    pub detail: String,
}

/// Переключает профиль, если нужный отличается от активного.
///
/// Возвращает `None`, когда менять нечего, — сборщик вызывает это часто, и молчание
/// в обычном случае здесь важнее подробностей.
pub fn maybe_switch(running: &[String]) -> Option<SwitchResult> {
    let mut s = load();
    if !s.auto {
        return None;
    }
    let want = resolve(&s, running)?.clone();
    if s.active.as_deref() == Some(want.id.as_str()) {
        return None;
    }

    if let Some(detail) = blocked(&want) {
        // Запоминаем намерение, чтобы не пытаться снова каждые несколько секунд.
        s.active = Some(want.id.clone());
        let _ = store(&s);
        return Some(SwitchResult { profile_id: want.id, profile_name: want.name, applied: false, detail });
    }

    let detail = match apply_now(&want.candidate) {
        Ok(()) => format!(
            "ядро {:+} МГц, память {:+} МГц, мощность {:.0} %",
            want.candidate.core_offset_mhz, want.candidate.mem_offset_mhz, want.candidate.power_percent
        ),
        Err(e) => {
            return Some(SwitchResult {
                profile_id: want.id,
                profile_name: want.name,
                applied: false,
                detail: format!("не удалось применить: {e}"),
            })
        }
    };

    s.active = Some(want.id.clone());
    let _ = store(&s);
    Some(SwitchResult { profile_id: want.id, profile_name: want.name, applied: true, detail })
}

/// Применяет значения профиля к карте, минуя журнал проверок.
///
/// Журнал ступеней здесь не нужен: профиль уже проверен, а его повторное применение
/// не является новой попыткой подбора. Границы всё равно соблюдаются — их накладывает
/// сам `nvapi`, отказываясь выходить за пределы, разрешённые драйвером.
fn apply_now(c: &GpuCandidate) -> Result<(), String> {
    crate::nvapi::set_power_limit_percent(c.power_percent)?;
    crate::nvapi::set_clock_offsets(c.core_offset_mhz, c.mem_offset_mhz)?;
    crate::nvapi::set_fan_level(c.fan_level)?;
    Ok(())
}

/// Ручное применение профиля по идентификатору.
pub fn apply(id: &str) -> Result<SwitchResult, String> {
    let mut s = load();
    let p = s
        .profiles
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| format!("Профиль «{id}» не найден."))?;
    if let Some(r) = crate::anticheat::writes_blocked() {
        return Err(r);
    }
    apply_now(&p.candidate)?;
    s.active = Some(p.id.clone());
    store(&s)?;
    Ok(SwitchResult {
        profile_id: p.id,
        profile_name: p.name,
        applied: true,
        detail: "Применён вручную.".into(),
    })
}

/// Создаёт профиль из текущей проверенной настройки.
///
/// Источник — `last_known_good` из журнала подбора: только он гарантированно прошёл
/// длинную ступень. Взять текущие значения с карты было бы неверно: они могут быть
/// серединой незаконченной проверки.
pub fn create_from_verified(name: &str, app_ids: Vec<String>) -> Result<ProfileStore, String> {
    let good = crate::ocsafe::load()
        .last_known_good
        .ok_or_else(|| "Проверенной настройки пока нет: сначала пройдите подбор до длинной ступени.".to_string())?;
    let mut s = load();
    let id = format!("p{}", chrono::Utc::now().timestamp());
    s.profiles.push(AppProfile {
        id,
        name: name.trim().to_string(),
        candidate: good,
        verified: true,
        app_ids,
        is_default: false,
    });
    store(&s)?;
    Ok(s)
}

pub fn remove(id: &str) -> Result<ProfileStore, String> {
    let mut s = load();
    if s.profiles.iter().any(|p| p.id == id && p.is_default) {
        return Err("Профиль по умолчанию удалить нельзя: к нему возвращается автоматика.".into());
    }
    s.profiles.retain(|p| p.id != id);
    if s.active.as_deref() == Some(id) {
        s.active = None;
    }
    store(&s)?;
    Ok(s)
}

pub fn set_auto(on: bool) -> Result<ProfileStore, String> {
    let mut s = load();
    s.auto = on;
    store(&s)?;
    Ok(s)
}

/// Привязывает профиль к набору приложений.
pub fn bind(id: &str, app_ids: Vec<String>) -> Result<ProfileStore, String> {
    let mut s = load();
    let p = s.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| format!("Профиль «{id}» не найден."))?;
    p.app_ids = app_ids;
    store(&s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, apps: &[&str], default: bool, verified: bool) -> AppProfile {
        AppProfile {
            id: id.into(),
            name: id.into(),
            candidate: GpuCandidate::default(),
            verified,
            app_ids: apps.iter().map(|s| s.to_string()).collect(),
            is_default: default,
        }
    }

    #[test]
    fn picks_first_matching_profile() {
        let s = ProfileStore {
            profiles: vec![
                profile("game", &["warface"], false, true),
                profile("render", &["blender"], false, true),
                profile("stock", &[], true, true),
            ],
            active: None,
            auto: true,
        };
        assert_eq!(resolve(&s, &["warface".into()]).unwrap().id, "game");
        assert_eq!(resolve(&s, &["blender".into()]).unwrap().id, "render");
        // Порядок задаёт приоритет, когда запущено и то и другое.
        assert_eq!(resolve(&s, &["blender".into(), "warface".into()]).unwrap().id, "game");
    }

    #[test]
    fn falls_back_to_default_when_nothing_matches() {
        let s = ProfileStore {
            profiles: vec![profile("game", &["warface"], false, true), profile("stock", &[], true, true)],
            active: None,
            auto: true,
        };
        assert_eq!(resolve(&s, &["notepad".into()]).unwrap().id, "stock");
        assert_eq!(resolve(&s, &[]).unwrap().id, "stock");
    }

    #[test]
    fn unverified_profile_is_not_applied_automatically() {
        let p = profile("risky", &["warface"], false, false);
        let why = blocked(&p).expect("непроверенный профиль не должен применяться сам");
        assert!(why.contains("не прошёл проверку"), "непонятная причина: {why}");
    }
}
