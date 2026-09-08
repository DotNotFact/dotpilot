//! Версии драйверов и прошивки: что стоит и насколько это старое.
//!
//! # Чего этот модуль не делает
//!
//! Он **не знает, какая версия последняя**, и не притворяется, что знает.
//!
//! Узнать это можно было бы, обратившись к недокументированным эндпоинтам вендоров —
//! тем, которыми пользуются их фирменные утилиты. Такие адреса меняются без
//! предупреждения, и приложение однажды начало бы молча сообщать «обновлений нет»
//! просто потому, что запрос перестал работать. Тихая ложь хуже честного отсутствия
//! ответа, поэтому здесь собирается то, что известно точно — установленные версии,
//! их даты и возраст, — и даётся прямая ссылка, где проверить.
//!
//! Ценность в том, что всё это собрано в одном месте с правильными ссылками под
//! конкретную плату и видеокарту, вместо блуждания по сайтам вендоров.

use crate::ps;
use serde::Serialize;

/// Драйвер старше этого срока стоит хотя бы проверить.
const DRIVER_STALE_DAYS: i64 = 180;
/// Прошивка на AM5 живёт дольше, но и её полезно освежать.
const FIRMWARE_STALE_DAYS: i64 = 400;

#[derive(Serialize, Clone, Debug)]
pub struct Component {
    pub name: String,
    pub installed: String,
    pub date: Option<String>,
    pub age_days: Option<i64>,
    /// Стоит ли посмотреть, есть ли новее.
    pub stale: bool,
    /// Где проверять — ссылка ведёт на страницу под это конкретное устройство.
    pub check_at: String,
    pub note: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct UpdateReport {
    pub components: Vec<Component>,
    pub disclaimer: String,
}

#[derive(serde::Deserialize, Default)]
struct DriverRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    provider: String,
    /// Видеодрайвер, а не компонент чипсета.
    #[serde(default)]
    is_display: bool,
}

fn age_days(date: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .map(|d| (chrono::Utc::now().date_naive() - d).num_days())
}

/// Собирает установленные версии драйверов и прошивки.
pub fn report() -> UpdateReport {
    let fw = crate::platform::firmware_info();
    let mut components = Vec::new();

    // Драйверы графики и чипсета: их дата — самый надёжный признак того,
    // что систему давно не обновляли.
    let script = "@(Get-CimInstance Win32_PnPSignedDriver -ErrorAction SilentlyContinue | \
         Where-Object { $_.DeviceClass -in 'DISPLAY','SYSTEM' -and $_.DriverProviderName -match 'NVIDIA|Advanced Micro|AMD' } | \
         Sort-Object DeviceName -Unique | \
         ForEach-Object { [pscustomobject]@{ name = $_.DeviceName; version = $_.DriverVersion; \
            date = if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') } else { '' }; provider = $_.DriverProviderName; \
            is_display = ($_.DeviceClass -eq 'DISPLAY') } }) | \
         ConvertTo-Json -Compress -Depth 3";

    let rows: Vec<DriverRow> = ps::run_ps(script)
        .ok()
        .and_then(|o| serde_json::from_str::<serde_json::Value>(&o).ok())
        .map(|v| {
            let arr = if v.is_array() { v } else { serde_json::Value::Array(vec![v]) };
            serde_json::from_value(arr).unwrap_or_default()
        })
        .unwrap_or_default();

    // Видеодрайверы показываются по отдельности: это самостоятельные пакеты,
    // которые обновляют осознанно и по отдельности.
    for r in rows.iter().filter(|r| !r.version.is_empty() && r.is_display) {
        let age = age_days(&r.date);
        let is_nvidia = r.provider.to_lowercase().contains("nvidia");
        components.push(Component {
            name: r.name.clone(),
            installed: r.version.clone(),
            date: (!r.date.is_empty()).then(|| r.date.clone()),
            age_days: age,
            stale: age.map(|d| d > DRIVER_STALE_DAYS).unwrap_or(false),
            check_at: if is_nvidia {
                "https://www.nvidia.ru/Download/index.aspx".into()
            } else {
                "https://www.amd.com/ru/support".into()
            },
            note: if is_nvidia {
                "Версия в свойствах Windows выглядит иначе, чем на сайте NVIDIA: 32.0.16.xxxx \
                 против 6xx.xx. Сопоставляются последние пять цифр."
                    .into()
            } else {
                "Встроенная графика процессора. Обновляется вместе с драйверами чипсета.".into()
            },
        });
    }

    // Компоненты чипсета ставятся одним пакетом и по отдельности бессмысленны:
    // перечислять «AMD GPIO Controller» и «AMD I2C Controller» отдельными строками
    // значит зашумлять вывод. Показываем одной записью по самому старому из них —
    // именно он говорит, когда пакет ставили в последний раз.
    let chipset: Vec<&DriverRow> = rows.iter().filter(|r| !r.version.is_empty() && !r.is_display).collect();
    if !chipset.is_empty() {
        let oldest = chipset.iter().filter(|r| !r.date.is_empty()).min_by_key(|r| r.date.clone());
        let date = oldest.map(|r| r.date.clone());
        let age = date.as_deref().and_then(age_days);
        components.push(Component {
            name: format!("Драйверы чипсета AMD · компонентов: {}", chipset.len()),
            installed: oldest.map(|r| r.version.clone()).unwrap_or_else(|| "—".into()),
            date,
            age_days: age,
            stale: age.map(|d| d > DRIVER_STALE_DAYS).unwrap_or(false),
            check_at: "https://www.amd.com/ru/support/download/drivers.html".into(),
            note: "Ставятся одним пакетом, поэтому показаны одной строкой по самому старому \
                   компоненту. Обновляются реже видеодрайверов, но влияют на управление питанием \
                   и работу шин."
                .into(),
        });
    }

    // Прошивка платы.
    let fw_age = age_days(&fw.bios_date);
    components.push(Component {
        name: format!("Прошивка платы · {}", fw.board),
        installed: fw.bios_version.clone(),
        date: (!fw.bios_date.is_empty()).then(|| fw.bios_date.clone()),
        age_days: fw_age,
        stale: fw_age.map(|d| d > FIRMWARE_STALE_DAYS).unwrap_or(false),
        // Поиск, а не прямая ссылка: собранный из названия адрес легко приводит
        // на несуществующую страницу, а поиск по модели работает всегда.
        check_at: format!(
            "https://www.gigabyte.com/Search?kw={}",
            urlencode(fw.board.split_whitespace().skip_while(|w| w.contains("Gigabyte") || w.contains(',') || w.contains("Ltd") || w.contains("Technology") || w.contains("Co")).collect::<Vec<_>>().join(" ").trim())
        ),
        note: "На AM5 обновления AGESA заметно влияют на совместимость и стабильность памяти. \
               Перед включением EXPO свежая прошивка повышает шансы, что профиль заработает сразу."
            .into(),
    });

    // Сборка Windows.
    if let Ok(build) = ps::run_ps("(Get-CimInstance Win32_OperatingSystem).Version + ' сборка ' + (Get-CimInstance Win32_OperatingSystem).BuildNumber") {
        components.push(Component {
            name: "Windows".into(),
            installed: build.trim().to_string(),
            date: None,
            age_days: None,
            stale: false,
            check_at: "ms-settings:windowsupdate".into(),
            note: "Обновления системы приходят сами; ссылка открывает Центр обновления.".into(),
        });
    }

    UpdateReport {
        disclaimer: "Приложение показывает, что установлено и насколько это старое, но **не знает, \
                     какая версия последняя**. Узнать это можно было бы через недокументированные \
                     адреса вендоров, которыми пользуются их фирменные утилиты, — но такие адреса \
                     меняются без предупреждения, и приложение начало бы молча сообщать «обновлений \
                     нет» просто потому, что запрос перестал работать. Ссылки ведут туда, где версия \
                     указана достоверно."
            .into(),
        components,
    }
}

/// Кодирование строки для подстановки в адрес.
///
/// Своя реализация вместо зависимости: нужен ровно один вызов, и тянуть ради него
/// крейт в портативный exe незачем.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn urlencode_handles_spaces_and_symbols() {
        assert_eq!(super::urlencode("X870 EAGLE WIFI7"), "X870+EAGLE+WIFI7");
        assert_eq!(super::urlencode("B650M/K"), "B650M%2FK");
        assert_eq!(super::urlencode("Z790-A"), "Z790-A");
    }

    #[test]
    fn age_is_counted_from_date() {
        // Дата в далёком прошлом обязана дать положительный возраст в днях.
        let d = super::age_days("2020-01-01").expect("дата должна разбираться");
        assert!(d > 2000, "возраст посчитан неверно: {d}");
        assert!(super::age_days("не дата").is_none(), "мусор не должен разбираться как дата");
    }

    /// Живой сбор: `cargo test --lib updates -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn collect_live() {
        let r = super::report();
        for c in &r.components {
            println!(
                "{} — {} {}{}",
                c.name,
                c.installed,
                c.date.clone().unwrap_or_else(|| "без даты".into()),
                if c.stale { " · СТОИТ ПРОВЕРИТЬ" } else { "" }
            );
            println!("    {}", c.check_at);
        }
        assert!(!r.components.is_empty(), "не найдено ни одного компонента");
    }
}
