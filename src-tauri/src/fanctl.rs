//! Управление вентиляторами платы через штатный ACPI-интерфейс Gigabyte.
//!
//! Плата выставляет WMI-класс `GSA1_ACPIMethod` (`ACPI\PNP0C14\GSADEV0_0`), а в нём —
//! семейство `ZFC*` для вентиляторов. Это идёт через инбоксовый ACPI-маппер Microsoft,
//! то есть **без уязвимого драйвера**: не спорит с блоклистом Microsoft и не выглядит
//! для анти-читов как вмешательство в ядро.
//!
//! Тот же класс отдаёт `PIOWrite8`, `MEMWrite64` и `PCIWrite32` — произвольную запись
//! в порты, память и конфигурацию PCI. Здесь они сознательно не используются: это ровно
//! тот класс возможностей, из-за которого WinRing0 и попал в блоклист. Мы берём только
//! методы, созданные для вентиляторов, — они принимают номер вентилятора, а не адрес.
//!
//! Прямой процент ШИМ этот интерфейс не даёт: скважность считает кривая Smart Fan в BIOS.
//! Управлять можно тем, что кривая получает на вход, — порогами остановки, привязкой
//! вентилятора к датчику и принудительным включением.

use crate::ps;
use serde::{Deserialize, Serialize};

// --- пределы, которые нельзя перешагнуть ------------------------------------
//
// Смысл тот же, что у Bounds в ocsafe: значения приходят из интерфейса или от модели,
// а решение о допустимости принимает код. Вентилятор, остановленный при высокой
// температуре, — это способ сжечь железо, поэтому пороги ограничены жёстко.

/// Выше этой температуры вентилятору нельзя разрешать останавливаться.
const MAX_STOP_TEMP_C: u8 = 55;
/// К этой температуре вентилятор обязан уже вращаться.
const MAX_START_TEMP_C: u8 = 70;
/// Порог включения должен быть выше порога выключения хотя бы на столько — иначе
/// вентилятор будет дёргаться на границе.
const MIN_HYSTERESIS_C: u8 = 3;

const MAX_FAN_ID: u8 = 5;
const MAX_SENSOR_ID: u8 = 5;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BoardSensor {
    pub id: u8,
    pub celsius: u8,
    /// Подключён ли датчик: ноль означает, что разъём пуст.
    pub connected: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BoardFan {
    pub id: u8,
    /// Разрешена ли полная остановка вентилятора при низкой температуре.
    pub stop_enabled: bool,
    /// Номер датчика, по которому плата управляет этим вентилятором.
    pub target_sensor: u8,
    /// Температура, ниже которой вентилятор останавливается.
    pub off_limit_c: u8,
    /// Температура, при которой вентилятор снова запускается.
    pub on_limit_c: u8,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct FanControllerState {
    pub available: bool,
    pub hw_id: Option<u8>,
    pub sensors: Vec<BoardSensor>,
    pub fans: Vec<BoardFan>,
    /// Через что идёт управление — чтобы в интерфейсе было видно, что не через драйвер.
    pub backend: String,
    pub note: String,
}

/// Общая часть скрипта: получить экземпляр ACPI-класса.
const PREAMBLE: &str = "$ErrorActionPreference='Stop'; \
    $i = Get-CimInstance -Namespace root\\WMI -ClassName GSA1_ACPIMethod -ErrorAction Stop; ";

/// Читает всё состояние одним вызовом PowerShell.
///
/// Один вызов вместо полутора десятков — принципиально: каждый запуск PowerShell
/// стоит сотни миллисекунд, а приложение обещает держать свою нагрузку около процента.
pub fn read_state() -> FanControllerState {
    let script = format!(
        "{PREAMBLE}\
         $call = {{ param($n, $a) try {{ Invoke-CimMethod -InputObject $i -MethodName $n -Arguments $a -ErrorAction Stop }} catch {{ $null }} }}; \
         $hw = (& $call 'ZFCGetHwId' @{{}}); \
         $sensors = @(); foreach ($s in 0..{MAX_SENSOR_ID}) {{ \
            $r = & $call 'ZFCGetCurrentTemp' @{{ id = [byte]$s }}; \
            if ($r) {{ $sensors += [pscustomobject]@{{ id = $s; celsius = [int]$r.value }} }} }}; \
         $fans = @(); foreach ($f in 0..{MAX_FAN_ID}) {{ \
            $st = & $call 'ZFCGetFanStopStatus' @{{ id = [byte]$f }}; \
            $tt = & $call 'ZFCGetFanTargetTemp' @{{ id = [byte]$f }}; \
            $lim = & $call 'ZFCGetFanTempLimit' @{{ id = [byte]$f }}; \
            if ($st -and $tt -and $lim) {{ $fans += [pscustomobject]@{{ \
                id = $f; stop = [int]$st.ison; target = [int]$tt.tempid; off = [int]$lim.off; on = [int]$lim.on }} }} }}; \
         [pscustomobject]@{{ hw = if ($hw) {{ [int]$hw.value }} else {{ $null }}; sensors = $sensors; fans = $fans }} | ConvertTo-Json -Compress -Depth 4"
    );

    let Ok(out) = ps::run_ps(&script) else {
        return FanControllerState {
            available: false,
            backend: "ACPI Gigabyte".into(),
            note: "ACPI-интерфейс платы не отвечает. Управление вентиляторами платы недоступно.".into(),
            ..Default::default()
        };
    };

    let v: serde_json::Value = match serde_json::from_str(&out) {
        Ok(v) => v,
        Err(e) => {
            return FanControllerState {
                available: false,
                backend: "ACPI Gigabyte".into(),
                note: format!("Не удалось разобрать ответ ACPI-интерфейса: {e}"),
                ..Default::default()
            }
        }
    };

    let hw_id = v.get("hw").and_then(|x| x.as_u64()).map(|x| x as u8);
    let sensors: Vec<BoardSensor> = v
        .get("sensors")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|s| {
                    let c = s.get("celsius").and_then(|x| x.as_u64()).unwrap_or(0) as u8;
                    BoardSensor {
                        id: s.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as u8,
                        celsius: c,
                        // Ноль на этом интерфейсе означает пустой разъём, а не ноль градусов.
                        connected: c > 0,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let fans: Vec<BoardFan> = v
        .get("fans")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|f| {
                    let n = |k: &str| f.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u8;
                    BoardFan {
                        id: n("id"),
                        stop_enabled: n("stop") != 0,
                        target_sensor: n("target"),
                        off_limit_c: n("off"),
                        on_limit_c: n("on"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let available = hw_id.is_some() && !fans.is_empty();
    FanControllerState {
        available,
        hw_id,
        note: if available {
            format!(
                "Управление идёт через ACPI-интерфейс платы, без кернел-драйвера: \
                 блоклист Microsoft ему не мешает, анти-читам он не виден. \
                 Найдено вентиляторов: {}, датчиков с показаниями: {}.",
                fans.len(),
                sensors.iter().filter(|s| s.connected).count()
            )
        } else {
            "ACPI-интерфейс управления вентиляторами на этой плате не найден.".into()
        },
        backend: "ACPI Gigabyte (GSA1_ACPIMethod, ZFC*)".into(),
        sensors,
        fans,
    }
}

fn check_fan(id: u8) -> Result<(), String> {
    if id > MAX_FAN_ID {
        return Err(format!("Вентилятора с номером {id} нет: допустимы 0…{MAX_FAN_ID}."));
    }
    Ok(())
}

/// Вызов метода записи.
///
/// У методов `ZFCSet*` все параметры входные, возвращаемого значения нет — успехом
/// считается сам факт вызова без исключения. Проверять здесь `ReturnValue` нельзя:
/// у пустого метода его попросту не существует, и такая проверка объявляла бы
/// отказом любую успешную запись. Явное `False` — единственный признак отказа.
fn invoke(method: &str, args: &[(&str, u8)]) -> Result<(), String> {
    let arg_list = args
        .iter()
        .map(|(k, v)| format!("{k} = [byte]{v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let script = format!(
        "{PREAMBLE}\
         $r = Invoke-CimMethod -InputObject $i -MethodName '{method}' -Arguments @{{ {arg_list} }} -ErrorAction Stop; \
         if ($null -ne $r.ReturnValue -and -not $r.ReturnValue) {{ 'плата отклонила вызов' }} else {{ 'ok' }}"
    );
    match ps::run_ps(&script) {
        Ok(out) if out.trim() == "ok" => Ok(()),
        Ok(out) => Err(format!("{method}: {}", out.trim())),
        Err(e) => Err(format!("{method}: {e}")),
    }
}

/// Разрешить или запретить полную остановку вентилятора на низких температурах.
///
/// Включать остановку без заданных порогов нельзя: пороги 0/0 означают, что плата
/// не знает, когда вентилятор вернуть, и он может остаться стоять под нагрузкой.
pub fn set_zero_fan(id: u8, enabled: bool) -> Result<(), String> {
    check_fan(id)?;
    if enabled {
        let state = read_state();
        let fan = state
            .fans
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| format!("Вентилятор {id} не найден."))?;
        if fan.on_limit_c == 0 {
            return Err(
                "Сначала задайте пороги остановки и запуска: без них вентилятор может остаться \
                 стоять под нагрузкой."
                    .into(),
            );
        }
    }
    invoke("ZFCSetFanStopStatus", &[("id", id), ("ison", u8::from(enabled))])
}

/// Пороги, при которых вентилятор останавливается и снова запускается.
///
/// Значения зажимаются пределами: остановка не выше 55 °C, обязательный запуск
/// не выше 70 °C, и между порогами держится гистерезис, иначе вентилятор будет
/// дёргаться на границе.
pub fn set_temp_limits(id: u8, off_c: u8, on_c: u8) -> Result<(u8, u8), String> {
    check_fan(id)?;
    let off = off_c.min(MAX_STOP_TEMP_C);
    let on = on_c.min(MAX_START_TEMP_C).max(off.saturating_add(MIN_HYSTERESIS_C));
    if on > MAX_START_TEMP_C {
        return Err(format!(
            "Порог выключения {off_c} °C слишком высок: вентилятор обязан работать к {MAX_START_TEMP_C} °C."
        ));
    }
    invoke("ZFCSetFanTempLimit", &[("id", id), ("off", off), ("on", on)])?;
    Ok((off, on))
}

/// Привязать вентилятор к другому датчику температуры.
pub fn set_target_sensor(id: u8, sensor: u8) -> Result<(), String> {
    check_fan(id)?;
    if sensor > MAX_SENSOR_ID {
        return Err(format!("Датчика с номером {sensor} нет: допустимы 0…{MAX_SENSOR_ID}."));
    }
    invoke("ZFCSetFanTargetTemp", &[("id", id), ("tempid", sensor)])
}

/// Принудительно включить вентилятор, отменив остановку.
pub fn force_on(id: u8) -> Result<(), String> {
    check_fan(id)?;
    invoke("ZFCFanOnOff", &[("flag", 1), ("id", id)])
}

/// Вернуть все вентиляторы под управление кривой Smart Fan из BIOS.
///
/// Выполняет все шаги даже при ошибке одного: вернуть максимум вентиляторов
/// под штатное управление важнее, чем прерваться на первом отказе.
pub fn restore_bios_control() -> Result<(), String> {
    let mut problems = Vec::new();
    for id in 0..=MAX_FAN_ID {
        if let Err(e) = invoke("ZFCSetFanStopStatus", &[("id", id), ("ison", 0)]) {
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
    /// Пороги должны зажиматься так, чтобы вентилятор не мог остаться стоять на жаре.
    #[test]
    fn limits_are_clamped_in_code() {
        // Проверяем саму арифметику ограничения, не трогая железо.
        let clamp = |off: u8, on: u8| -> (u8, u8) {
            let o = off.min(super::MAX_STOP_TEMP_C);
            let n = on.min(super::MAX_START_TEMP_C).max(o.saturating_add(super::MIN_HYSTERESIS_C));
            (o, n)
        };
        assert_eq!(clamp(200, 200), (55, 70), "остановка не должна разрешаться выше предела");
        assert_eq!(clamp(40, 40), (40, 43), "между порогами обязан быть гистерезис");
        assert_eq!(clamp(30, 50), (30, 50), "разумные значения проходят без изменений");
    }

    /// Живое чтение состояния: `cargo test --lib fanctl -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn read_live() {
        let s = super::read_state();
        println!("доступно: {} | {}", s.available, s.backend);
        println!("{}", s.note);
        for x in &s.sensors {
            println!("  датчик {}: {} °C{}", x.id, x.celsius, if x.connected { "" } else { " (не подключён)" });
        }
        for f in &s.fans {
            println!(
                "  вентилятор {}: остановка {}, датчик {}, пороги {}/{} °C",
                f.id,
                if f.stop_enabled { "разрешена" } else { "запрещена" },
                f.target_sensor,
                f.off_limit_c,
                f.on_limit_c
            );
        }
        assert!(s.available, "ACPI-интерфейс вентиляторов не отвечает");
    }
}

#[cfg(test)]
mod write_tests {
    /// Проверка пути записи вхолостую: вентилятору назначается тот же датчик,
    /// что у него уже стоит. Поведение не меняется, но факт приёма записи платой
    /// подтверждается. `cargo test --lib fanctl::write_tests -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn write_roundtrip_noop() {
        let before = super::read_state();
        assert!(before.available, "ACPI-интерфейс недоступен");
        let fan = before.fans.first().expect("вентиляторов не найдено");
        let sensor = fan.target_sensor;
        println!("вентилятор {} слушает датчик {} — записываем то же значение", fan.id, sensor);

        super::set_target_sensor(fan.id, sensor).expect("запись не прошла");

        let after = super::read_state();
        let same = after.fans.iter().find(|f| f.id == fan.id).expect("вентилятор пропал");
        println!("после записи: датчик {}", same.target_sensor);
        assert_eq!(same.target_sensor, sensor, "значение изменилось, хотя запись была холостой");

        // Холостая запись доказывает только отсутствие ошибки. Чтобы убедиться, что
        // плата действительно принимает значения, меняем датчик и сразу возвращаем.
        let other = if sensor == 4 { 3 } else { 4 };
        super::set_target_sensor(fan.id, other).expect("запись другого значения не прошла");
        let changed = super::read_state();
        let moved = changed.fans.iter().find(|f| f.id == fan.id).unwrap().target_sensor;
        super::set_target_sensor(fan.id, sensor).expect("возврат исходного значения не прошёл");
        let restored = super::read_state().fans.iter().find(|f| f.id == fan.id).unwrap().target_sensor;
        println!("смена: {sensor} -> {moved}, возврат -> {restored}");
        assert_eq!(moved, other, "плата не приняла новое значение");
        assert_eq!(restored, sensor, "исходное значение не восстановлено");

        // Заодно проверяем, что защита не пускает опасные пороги.
        let e = super::set_zero_fan(fan.id, true).unwrap_err();
        println!("попытка разрешить остановку без порогов отклонена: {e}");
        assert!(e.contains("пороги"), "защита от остановки без порогов не сработала");
    }
}
