//! NVAPI: чтение телеметрии и лимитов NVIDIA GPU.
//!
//! nvapi64.dll не экспортирует функции по именам — есть единственный экспорт
//! `nvapi_QueryInterface(id)`, который отдаёт указатель по числовому идентификатору.
//! Идентификаторы стабильны между версиями драйвера.
//!
//! Каждая структура несёт поле `version` = `sizeof(struct) | (версия << 16)`.
//! Драйвер сверяет его с собственной раскладкой, поэтому неверный размер даёт
//! NVAPI_INCOMPATIBLE_STRUCT_VERSION (-5), а не порчу памяти. На этом построен
//! `try_versions`: пробуем версии от новых к старым и берём первую, которую драйвер принял.
//!
//! Проверено на RTX 5070 Ti (Blackwell), драйвер 610.88.

#![allow(dead_code)]

use serde::Serialize;
use std::ffi::c_void;
use std::sync::OnceLock;

// --- загрузка библиотеки ---------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

type QueryInterfaceFn = unsafe extern "C" fn(u32) -> *mut c_void;

struct Lib {
    query: QueryInterfaceFn,
}

// Указатели из nvapi64.dll живут столько же, сколько процесс.
unsafe impl Send for Lib {}
unsafe impl Sync for Lib {}

static LIB: OnceLock<Option<Lib>> = OnceLock::new();

fn lib() -> Option<&'static Lib> {
    LIB.get_or_init(|| unsafe {
        let module = LoadLibraryA(b"nvapi64.dll\0".as_ptr());
        if module.is_null() {
            return None;
        }
        let q = GetProcAddress(module, b"nvapi_QueryInterface\0".as_ptr());
        if q.is_null() {
            return None;
        }
        let query: QueryInterfaceFn = std::mem::transmute(q);
        // NvAPI_Initialize обязателен до любого другого вызова.
        let init = query(0x0150_E828);
        if init.is_null() {
            return None;
        }
        let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(init);
        if init() != 0 {
            return None;
        }
        Some(Lib { query })
    })
    .as_ref()
}

fn resolve(id: u32) -> Option<*mut c_void> {
    let l = lib()?;
    let p = unsafe { (l.query)(id) };
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

/// Доступна ли NVAPI и инициализировалась ли она.
pub fn available() -> bool {
    lib().is_some()
}

// --- идентификаторы функций ------------------------------------------------

const ID_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
const ID_GET_FULL_NAME: u32 = 0xCEEE_8E9F;
const ID_GET_THERMAL_SETTINGS: u32 = 0xE364_0A56;
const ID_GET_ALL_CLOCK_FREQUENCIES: u32 = 0xDCB6_16C3;
const ID_GET_DYNAMIC_PSTATES_INFO_EX: u32 = 0x60DE_D2ED;
const ID_GET_PSTATES20: u32 = 0x6FF8_1213;
const ID_POWER_POLICIES_GET_INFO: u32 = 0x3420_6D86;
const ID_POWER_POLICIES_GET_STATUS: u32 = 0x7091_6171;
const ID_FAN_COOLERS_GET_STATUS: u32 = 0x35AE_D5E8;
const ID_FAN_COOLERS_GET_CONTROL: u32 = 0x814B_209F;

const NVAPI_OK: i32 = 0;
const NVAPI_INCOMPATIBLE_STRUCT_VERSION: i32 = -5;

const MAX_PHYSICAL_GPUS: usize = 64;

/// `sizeof(struct) | (версия << 16)` — то, что ждёт драйвер в поле `version`.
const fn sver(size: usize, ver: u32) -> u32 {
    (size as u32) | (ver << 16)
}

/// Пробует версии структуры от новой к старой; возвращает первую принятую драйвером.
///
/// `call` получает номер версии, заполняет структуру и возвращает код NVAPI.
fn try_versions<T, F>(versions: &[u32], mut call: F) -> Result<(T, u32), i32>
where
    T: Default,
    F: FnMut(u32, &mut T) -> i32,
{
    let mut last = NVAPI_INCOMPATIBLE_STRUCT_VERSION;
    for &v in versions {
        let mut val = T::default();
        let rc = call(v, &mut val);
        if rc == NVAPI_OK {
            return Ok((val, v));
        }
        last = rc;
        if rc != NVAPI_INCOMPATIBLE_STRUCT_VERSION {
            break; // ошибка не про версию — перебор дальше бессмыслен
        }
    }
    Err(last)
}

// --- перечисление GPU ------------------------------------------------------

fn enum_gpus() -> Vec<*mut c_void> {
    let Some(p) = resolve(ID_ENUM_PHYSICAL_GPUS) else {
        return Vec::new();
    };
    let f: unsafe extern "C" fn(*mut *mut c_void, *mut u32) -> i32 = unsafe { std::mem::transmute(p) };
    let mut handles = [std::ptr::null_mut::<c_void>(); MAX_PHYSICAL_GPUS];
    let mut count: u32 = 0;
    let rc = unsafe { f(handles.as_mut_ptr(), &mut count) };
    if rc != NVAPI_OK {
        return Vec::new();
    }
    handles[..count as usize].to_vec()
}

fn gpu_name(h: *mut c_void) -> Option<String> {
    let p = resolve(ID_GET_FULL_NAME)?;
    let f: unsafe extern "C" fn(*mut c_void, *mut u8) -> i32 = unsafe { std::mem::transmute(p) };
    let mut buf = [0u8; 64];
    if unsafe { f(h, buf.as_mut_ptr()) } != NVAPI_OK {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).trim().to_string())
}

// --- температуры -----------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct ThermalSensor {
    controller: u32,
    default_min_temp: i32,
    default_max_temp: i32,
    current_temp: i32,
    target: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ThermalSettings {
    version: u32,
    count: u32,
    sensor: [ThermalSensor; 3],
}

impl Default for ThermalSettings {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

/// Значения NV_THERMAL_TARGET.
fn thermal_target_name(t: u32) -> &'static str {
    match t {
        1 => "gpu",
        2 => "memory",
        4 => "power_supply",
        8 => "board",
        9 => "vcd_board",
        10 => "vcd_inlet",
        11 => "vcd_outlet",
        _ => "unknown",
    }
}

/// NVAPI_THERMAL_TARGET_ALL — попросить драйвер заполнить все датчики разом.
const THERMAL_TARGET_ALL: u32 = 15;

fn read_thermals(h: *mut c_void) -> Vec<(String, i32, i32, i32)> {
    let Some(p) = resolve(ID_GET_THERMAL_SETTINGS) else {
        return Vec::new();
    };
    let f: unsafe extern "C" fn(*mut c_void, u32, *mut ThermalSettings) -> i32 =
        unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<ThermalSettings>();
    let versions = [sver(SIZE, 2), sver(SIZE, 1)];

    // Сначала просим все датчики разом; если драйвер это не принял — берём нулевой.
    let res = try_versions::<ThermalSettings, _>(&versions, |v, st| {
        st.version = v;
        unsafe { f(h, THERMAL_TARGET_ALL, st) }
    })
    .or_else(|_| {
        try_versions::<ThermalSettings, _>(&versions, |v, st| {
            st.version = v;
            unsafe { f(h, 0, st) }
        })
    });

    match res {
        Ok((st, _)) => st
            .sensor
            .iter()
            .take(st.count.min(3) as usize)
            .map(|s| {
                (
                    thermal_target_name(s.target).to_string(),
                    s.current_temp,
                    s.default_min_temp,
                    s.default_max_temp,
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// --- частоты ---------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct ClockDomain {
    /// бит 0 — присутствует ли домен
    flags: u32,
    frequency_khz: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ClockFrequencies {
    version: u32,
    /// биты 0..1 — тип (0 текущие, 1 базовые, 2 boost), остальное зарезервировано
    clock_type: u32,
    domain: [ClockDomain; 32],
}

impl Default for ClockFrequencies {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

const CLOCK_GRAPHICS: usize = 0;
const CLOCK_MEMORY: usize = 4;
const CLOCK_PROCESSOR: usize = 7;
const CLOCK_VIDEO: usize = 8;

/// `kind`: 0 — текущие, 1 — базовые, 2 — boost.
fn read_clocks(h: *mut c_void, kind: u32) -> Option<ClockFrequencies> {
    let p = resolve(ID_GET_ALL_CLOCK_FREQUENCIES)?;
    let f: unsafe extern "C" fn(*mut c_void, *mut ClockFrequencies) -> i32 =
        unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<ClockFrequencies>();
    let versions = [sver(SIZE, 3), sver(SIZE, 2), sver(SIZE, 1)];

    try_versions::<ClockFrequencies, _>(&versions, |v, cf| {
        cf.version = v;
        cf.clock_type = kind;
        unsafe { f(h, cf) }
    })
    .ok()
    .map(|(cf, _)| cf)
}

fn domain_mhz(cf: &ClockFrequencies, idx: usize) -> Option<u32> {
    let d = cf.domain[idx];
    if d.flags & 1 == 0 {
        return None;
    }
    Some(d.frequency_khz / 1000)
}

// --- загрузка доменов ------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct DynamicPstateUtil {
    is_present: u32,
    percentage: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DynamicPstatesInfoEx {
    version: u32,
    flags: u32,
    utilization: [DynamicPstateUtil; 8],
}

impl Default for DynamicPstatesInfoEx {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

/// Индексы: 0 — GPU, 1 — framebuffer, 2 — video engine, 3 — шина.
fn read_utilization(h: *mut c_void) -> Option<DynamicPstatesInfoEx> {
    let p = resolve(ID_GET_DYNAMIC_PSTATES_INFO_EX)?;
    let f: unsafe extern "C" fn(*mut c_void, *mut DynamicPstatesInfoEx) -> i32 =
        unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<DynamicPstatesInfoEx>();
    try_versions::<DynamicPstatesInfoEx, _>(&[sver(SIZE, 1)], |v, u| {
        u.version = v;
        unsafe { f(h, u) }
    })
    .ok()
    .map(|(u, _)| u)
}

// --- лимит мощности --------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct PowerPolicyStatusEntry {
    pstate: u32,
    unknown1: u32,
    /// в промилле от штатного лимита: 100000 = 100 %
    power_pcm: u32,
    unknown2: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PowerPolicyStatus {
    version: u32,
    count: u32,
    entries: [PowerPolicyStatusEntry; 4],
}

impl Default for PowerPolicyStatus {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PowerPolicyInfoEntry {
    pstate: u32,
    unknown1: [u32; 2],
    min_power_pcm: u32,
    unknown2: [u32; 2],
    def_power_pcm: u32,
    unknown3: [u32; 2],
    max_power_pcm: u32,
    unknown4: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PowerPolicyInfo {
    version: u32,
    /// количество валидных записей — в младшем байте
    valid: u8,
    count: u8,
    padding: [u8; 2],
    entries: [PowerPolicyInfoEntry; 4],
}

impl Default for PowerPolicyInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

fn read_power_status(h: *mut c_void) -> Option<u32> {
    let p = resolve(ID_POWER_POLICIES_GET_STATUS)?;
    let f: unsafe extern "C" fn(*mut c_void, *mut PowerPolicyStatus) -> i32 =
        unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<PowerPolicyStatus>();
    try_versions::<PowerPolicyStatus, _>(&[sver(SIZE, 1)], |v, st| {
        st.version = v;
        unsafe { f(h, st) }
    })
    .ok()
    .and_then(|(st, _)| {
        if st.count == 0 {
            None
        } else {
            Some(st.entries[0].power_pcm)
        }
    })
}

fn read_power_info(h: *mut c_void) -> Option<(u32, u32, u32)> {
    let p = resolve(ID_POWER_POLICIES_GET_INFO)?;
    let f: unsafe extern "C" fn(*mut c_void, *mut PowerPolicyInfo) -> i32 =
        unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<PowerPolicyInfo>();
    try_versions::<PowerPolicyInfo, _>(&[sver(SIZE, 1)], |v, inf| {
        inf.version = v;
        unsafe { f(h, inf) }
    })
    .ok()
    .and_then(|(inf, _)| {
        if inf.valid == 0 {
            return None;
        }
        let e = inf.entries[0];
        Some((e.min_power_pcm, e.def_power_pcm, e.max_power_pcm))
    })
}

// --- вентиляторы -----------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct FanCoolerStatusItem {
    cooler_id: u32,
    current_rpm: u32,
    current_min_level: u32,
    current_max_level: u32,
    current_level: u32,
    reserved: [u32; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FanCoolersStatus {
    version: u32,
    count: u32,
    reserved: [u32; 8],
    items: [FanCoolerStatusItem; 32],
}

impl Default for FanCoolersStatus {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FanCoolerControlItem {
    cooler_id: u32,
    level: u32,
    /// 0 — автоматика драйвера, 1 — ручной уровень
    mode: u32,
    reserved: [u32; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FanCoolersControl {
    version: u32,
    unknown: u32,
    count: u32,
    reserved: [u32; 8],
    items: [FanCoolerControlItem; 32],
}

impl Default for FanCoolersControl {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct FanState {
    pub cooler_id: u32,
    pub rpm: u32,
    pub level_percent: u32,
    pub min_level: u32,
    pub max_level: u32,
    /// true — уровнем управляет драйвер, false — задан вручную
    pub automatic: bool,
}

fn read_fans(h: *mut c_void) -> Vec<FanState> {
    let Some(ps) = resolve(ID_FAN_COOLERS_GET_STATUS) else {
        return Vec::new();
    };
    let fs: unsafe extern "C" fn(*mut c_void, *mut FanCoolersStatus) -> i32 =
        unsafe { std::mem::transmute(ps) };

    const SSIZE: usize = std::mem::size_of::<FanCoolersStatus>();
    let status = try_versions::<FanCoolersStatus, _>(&[sver(SSIZE, 1)], |v, st| {
        st.version = v;
        unsafe { fs(h, st) }
    });
    let Ok((status, _)) = status else {
        return Vec::new();
    };

    // Режим (авто/ручной) лежит в отдельной структуре control.
    let mut modes: Vec<(u32, u32)> = Vec::new();
    if let Some(pc) = resolve(ID_FAN_COOLERS_GET_CONTROL) {
        let fc: unsafe extern "C" fn(*mut c_void, *mut FanCoolersControl) -> i32 =
            unsafe { std::mem::transmute(pc) };
        const CSIZE: usize = std::mem::size_of::<FanCoolersControl>();
        if let Ok((ctl, _)) = try_versions::<FanCoolersControl, _>(&[sver(CSIZE, 1)], |v, c| {
            c.version = v;
            unsafe { fc(h, c) }
        }) {
            modes = ctl
                .items
                .iter()
                .take(ctl.count.min(32) as usize)
                .map(|i| (i.cooler_id, i.mode))
                .collect();
        }
    }

    status
        .items
        .iter()
        .take(status.count.min(32) as usize)
        .map(|it| {
            let mode = modes.iter().find(|(id, _)| *id == it.cooler_id).map(|(_, m)| *m);
            FanState {
                cooler_id: it.cooler_id,
                rpm: it.current_rpm,
                level_percent: it.current_level,
                min_level: it.current_min_level,
                max_level: it.current_max_level,
                automatic: mode.map(|m| m == 0).unwrap_or(true),
            }
        })
        .collect()
}

// --- P-states / кривая V-F -------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ParamDelta {
    value: i32,
    value_min: i32,
    value_max: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pstate20ClockEntry {
    domain_id: u32,
    type_id: u32,
    /// бит 0 — редактируемо ли
    flags: u32,
    freq_delta_khz: ParamDelta,
    /// union: single { freq_khz } | range { min, max, domain, minVolt_uV, maxVolt_uV }
    data: [u32; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pstate20BaseVoltageEntry {
    domain_id: u32,
    flags: u32,
    volt_uv: u32,
    volt_delta_uv: ParamDelta,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pstate20Entry {
    pstate_id: u32,
    flags: u32,
    clocks: [Pstate20ClockEntry; 8],
    base_voltages: [Pstate20BaseVoltageEntry; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pstates20InfoOv {
    num_voltages: u32,
    voltages: [Pstate20BaseVoltageEntry; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pstates20Info {
    version: u32,
    flags: u32,
    num_pstates: u32,
    num_clocks: u32,
    num_base_voltages: u32,
    pstates: [Pstate20Entry; 16],
    ov: Pstates20InfoOv,
}

impl Default for Pstates20Info {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

const PSTATE_DOMAIN_GRAPHICS: u32 = 0;
const PSTATE_DOMAIN_MEMORY: u32 = 4;

/// Текущие смещения частот (offset) в МГц и допустимый диапазон.
#[derive(Serialize, Clone, Debug, Default)]
pub struct ClockOffset {
    pub current_mhz: i32,
    pub min_mhz: i32,
    pub max_mhz: i32,
    pub editable: bool,
}

fn read_pstates(h: *mut c_void) -> Option<(ClockOffset, ClockOffset)> {
    let p = resolve(ID_GET_PSTATES20)?;
    let f: unsafe extern "C" fn(*mut c_void, *mut Pstates20Info) -> i32 =
        unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<Pstates20Info>();
    // V3 и V2 совпадают по размеру с V2-раскладкой; V1 короче на блок ov.
    const SIZE_V1: usize = SIZE - std::mem::size_of::<Pstates20InfoOv>();
    let versions = [sver(SIZE, 3), sver(SIZE, 2), sver(SIZE_V1, 1)];

    let (info, _) = try_versions::<Pstates20Info, _>(&versions, |v, st| {
        st.version = v;
        unsafe { f(h, st) }
    })
    .ok()?;

    // P0 — состояние максимальной производительности, именно его смещения правит разгон.
    let p0 = info.pstates.iter().find(|p| p.pstate_id == 0)?;

    let pick = |domain: u32| -> ClockOffset {
        p0.clocks
            .iter()
            .take(info.num_clocks.min(8) as usize)
            .find(|c| c.domain_id == domain)
            .map(|c| ClockOffset {
                current_mhz: c.freq_delta_khz.value / 1000,
                min_mhz: c.freq_delta_khz.value_min / 1000,
                max_mhz: c.freq_delta_khz.value_max / 1000,
                editable: c.flags & 1 != 0,
            })
            .unwrap_or_default()
    };

    Some((pick(PSTATE_DOMAIN_GRAPHICS), pick(PSTATE_DOMAIN_MEMORY)))
}

// --- публичный снимок ------------------------------------------------------

#[derive(Serialize, Clone, Debug, Default)]
pub struct GpuTelemetry {
    pub name: String,
    pub temperatures: Vec<TempSensor>,
    pub clock_current_mhz: Option<u32>,
    pub clock_base_mhz: Option<u32>,
    pub clock_boost_mhz: Option<u32>,
    pub mem_clock_current_mhz: Option<u32>,
    pub util_gpu_percent: Option<u32>,
    pub util_fb_percent: Option<u32>,
    /// Текущий лимит мощности в процентах от штатного.
    pub power_current_percent: Option<f32>,
    pub power_min_percent: Option<f32>,
    pub power_default_percent: Option<f32>,
    pub power_max_percent: Option<f32>,
    pub fans: Vec<FanState>,
    pub core_offset: ClockOffset,
    pub mem_offset: ClockOffset,
}

#[derive(Serialize, Clone, Debug)]
pub struct TempSensor {
    pub target: String,
    pub current_c: i32,
    pub max_c: i32,
}

/// Снимок первой NVIDIA-карты. `None`, если NVAPI недоступна.
pub fn telemetry() -> Option<GpuTelemetry> {
    let gpus = enum_gpus();
    let h = *gpus.first()?;

    let pcm_to_percent = |v: u32| v as f32 / 1000.0;
    let (pmin, pdef, pmax) = read_power_info(h).unzip3();

    let cur = read_clocks(h, 0);
    let base = read_clocks(h, 1);
    let boost = read_clocks(h, 2);
    let util = read_utilization(h);
    let (core_offset, mem_offset) = read_pstates(h).unwrap_or_default();

    Some(GpuTelemetry {
        name: gpu_name(h).unwrap_or_else(|| "NVIDIA GPU".into()),
        temperatures: read_thermals(h)
            .into_iter()
            .map(|(target, current_c, _, max_c)| TempSensor { target, current_c, max_c })
            .collect(),
        clock_current_mhz: cur.as_ref().and_then(|c| domain_mhz(c, CLOCK_GRAPHICS)),
        clock_base_mhz: base.as_ref().and_then(|c| domain_mhz(c, CLOCK_GRAPHICS)),
        clock_boost_mhz: boost.as_ref().and_then(|c| domain_mhz(c, CLOCK_GRAPHICS)),
        mem_clock_current_mhz: cur.as_ref().and_then(|c| domain_mhz(c, CLOCK_MEMORY)),
        util_gpu_percent: util.as_ref().and_then(|u| {
            (u.utilization[0].is_present & 1 != 0).then_some(u.utilization[0].percentage)
        }),
        util_fb_percent: util.as_ref().and_then(|u| {
            (u.utilization[1].is_present & 1 != 0).then_some(u.utilization[1].percentage)
        }),
        power_current_percent: read_power_status(h).map(pcm_to_percent),
        power_min_percent: pmin.map(pcm_to_percent),
        power_default_percent: pdef.map(pcm_to_percent),
        power_max_percent: pmax.map(pcm_to_percent),
        fans: read_fans(h),
        core_offset,
        mem_offset,
    })
}

/// Что именно из управления доступно на этой машине — для честного отчёта в интерфейсе.
#[derive(Serialize, Clone, Debug, Default)]
pub struct GpuCapabilities {
    pub nvapi: bool,
    pub gpu_count: usize,
    pub read_thermals: bool,
    pub read_clocks: bool,
    pub read_pstates: bool,
    pub set_clock_offsets: bool,
    pub set_power_limit: bool,
    pub set_fan_curve: bool,
    pub set_voltage_direct: bool,
}

pub fn capabilities() -> GpuCapabilities {
    if !available() {
        return GpuCapabilities::default();
    }
    GpuCapabilities {
        nvapi: true,
        gpu_count: enum_gpus().len(),
        read_thermals: resolve(ID_GET_THERMAL_SETTINGS).is_some(),
        read_clocks: resolve(ID_GET_ALL_CLOCK_FREQUENCIES).is_some(),
        read_pstates: resolve(ID_GET_PSTATES20).is_some(),
        set_clock_offsets: resolve(0x0F4D_AE6B).is_some(),
        set_power_limit: resolve(0xAD95_F5ED).is_some(),
        set_fan_curve: resolve(0xA589_71A5).is_some(),
        // Прямое управление напряжением на Blackwell не экспортируется:
        // андервольт делается смещением точек V/F через SetPstates20.
        set_voltage_direct: resolve(0x8C98_2440).is_some(),
    }
}

// Мелкий помощник: Option<(a,b,c)> -> (Option<a>, Option<b>, Option<c>)
trait Unzip3<A, B, C> {
    fn unzip3(self) -> (Option<A>, Option<B>, Option<C>);
}

impl<A, B, C> Unzip3<A, B, C> for Option<(A, B, C)> {
    fn unzip3(self) -> (Option<A>, Option<B>, Option<C>) {
        match self {
            Some((a, b, c)) => (Some(a), Some(b), Some(c)),
            None => (None, None, None),
        }
    }
}

// --- запись ----------------------------------------------------------------
//
// Общий принцип: read-modify-write. Структуры NVAPI содержат поля с неизвестным
// назначением, поэтому мы читаем текущее состояние, меняем ровно одно поле и пишем
// обратно, а не собираем структуру с нуля. Все значения дополнительно зажимаются
// границами, которые сообщил сам драйвер.

const ID_SET_PSTATES20: u32 = 0x0F4D_AE6B;
const ID_POWER_POLICIES_SET_STATUS: u32 = 0xAD95_F5ED;
const ID_FAN_COOLERS_SET_CONTROL: u32 = 0xA589_71A5;

fn first_gpu() -> Result<*mut c_void, String> {
    enum_gpus()
        .into_iter()
        .next()
        .ok_or_else(|| "Карта NVIDIA не найдена".to_string())
}

fn nv_err(what: &str, rc: i32) -> String {
    let hint = match rc {
        -5 => " (несовпадение версии структуры — вероятно, изменился драйвер)",
        -6 => " (недостаточно прав: нужен запуск от администратора)",
        -104 => " (операция не поддерживается этой картой)",
        _ => "",
    };
    format!("NVAPI: {} не выполнено, код {}{}", what, rc, hint)
}

fn read_pstates_raw(h: *mut c_void) -> Result<(Pstates20Info, u32), String> {
    let p = resolve(ID_GET_PSTATES20).ok_or_else(|| "NvAPI_GPU_GetPstates20 недоступна".to_string())?;
    let f: unsafe extern "C" fn(*mut c_void, *mut Pstates20Info) -> i32 = unsafe { std::mem::transmute(p) };

    const SIZE: usize = std::mem::size_of::<Pstates20Info>();
    const SIZE_V1: usize = SIZE - std::mem::size_of::<Pstates20InfoOv>();
    let versions = [sver(SIZE, 3), sver(SIZE, 2), sver(SIZE_V1, 1)];

    try_versions::<Pstates20Info, _>(&versions, |v, st| {
        st.version = v;
        unsafe { f(h, st) }
    })
    .map_err(|rc| nv_err("чтение таблицы P-states", rc))
}

fn read_power_status_raw(h: *mut c_void) -> Result<(PowerPolicyStatus, u32), String> {
    let p = resolve(ID_POWER_POLICIES_GET_STATUS)
        .ok_or_else(|| "NvAPI_GPU_ClientPowerPoliciesGetStatus недоступна".to_string())?;
    let f: unsafe extern "C" fn(*mut c_void, *mut PowerPolicyStatus) -> i32 = unsafe { std::mem::transmute(p) };
    const SIZE: usize = std::mem::size_of::<PowerPolicyStatus>();
    try_versions::<PowerPolicyStatus, _>(&[sver(SIZE, 1)], |v, st| {
        st.version = v;
        unsafe { f(h, st) }
    })
    .map_err(|rc| nv_err("чтение лимита мощности", rc))
}

fn read_fan_control_raw(h: *mut c_void) -> Result<(FanCoolersControl, u32), String> {
    let p = resolve(ID_FAN_COOLERS_GET_CONTROL)
        .ok_or_else(|| "NvAPI_GPU_ClientFanCoolersGetControl недоступна".to_string())?;
    let f: unsafe extern "C" fn(*mut c_void, *mut FanCoolersControl) -> i32 = unsafe { std::mem::transmute(p) };
    const SIZE: usize = std::mem::size_of::<FanCoolersControl>();
    try_versions::<FanCoolersControl, _>(&[sver(SIZE, 1)], |v, c| {
        c.version = v;
        unsafe { f(h, c) }
    })
    .map_err(|rc| nv_err("чтение режима вентиляторов", rc))
}

/// Смещения частот ядра и памяти в МГц. Возвращает фактически применённые значения
/// после обрезки по границам драйвера — они могут отличаться от запрошенных.
pub fn set_clock_offsets(core_mhz: i32, mem_mhz: i32) -> Result<(i32, i32), String> {
    let h = first_gpu()?;
    let (info, version) = read_pstates_raw(h)?;

    let p0 = *info
        .pstates
        .iter()
        .find(|p| p.pstate_id == 0)
        .ok_or_else(|| "В таблице P-states нет состояния P0".to_string())?;

    let num_clocks = info.num_clocks.min(8) as usize;
    let source_for = |domain: u32| p0.clocks[..num_clocks].iter().find(|c| c.domain_id == domain).copied();

    // Отправляем минимальную структуру: только P0 и только те домены, что меняем.
    let mut out: Pstates20Info = unsafe { std::mem::zeroed() };
    out.version = version;
    out.num_pstates = 1;
    out.num_base_voltages = 0;
    out.pstates[0].pstate_id = 0;

    let mut written = 0usize;
    let mut applied = (0i32, 0i32);

    for (domain, want) in [(PSTATE_DOMAIN_GRAPHICS, core_mhz), (PSTATE_DOMAIN_MEMORY, mem_mhz)] {
        let Some(src) = source_for(domain) else { continue };
        if src.flags & 1 == 0 {
            continue; // драйвер пометил домен как нередактируемый
        }
        let lo = src.freq_delta_khz.value_min / 1000;
        let hi = src.freq_delta_khz.value_max / 1000;
        let value = want.clamp(lo, hi);

        let mut entry: Pstate20ClockEntry = unsafe { std::mem::zeroed() };
        entry.domain_id = domain;
        entry.type_id = src.type_id;
        entry.freq_delta_khz.value = value * 1000;

        out.pstates[0].clocks[written] = entry;
        written += 1;
        if domain == PSTATE_DOMAIN_GRAPHICS {
            applied.0 = value;
        } else {
            applied.1 = value;
        }
    }

    if written == 0 {
        return Err("Драйвер не разрешает менять смещения частот на этой карте".into());
    }
    out.num_clocks = written as u32;

    let p = resolve(ID_SET_PSTATES20).ok_or_else(|| "NvAPI_GPU_SetPstates20 недоступна".to_string())?;
    let f: unsafe extern "C" fn(*mut c_void, *mut Pstates20Info) -> i32 = unsafe { std::mem::transmute(p) };
    let rc = unsafe { f(h, &mut out) };
    if rc != NVAPI_OK {
        return Err(nv_err("смещение частот", rc));
    }
    Ok(applied)
}

/// Лимит мощности в процентах от штатного. Возвращает фактически применённое значение.
pub fn set_power_limit_percent(percent: f32) -> Result<f32, String> {
    let h = first_gpu()?;
    let (min_pcm, _def, max_pcm) =
        read_power_info(h).ok_or_else(|| "Не удалось прочитать границы лимита мощности".to_string())?;
    let value = percent.clamp(min_pcm as f32 / 1000.0, max_pcm as f32 / 1000.0);

    let (mut status, version) = read_power_status_raw(h)?;
    status.version = version;
    if status.count == 0 {
        status.count = 1;
    }
    status.entries[0].power_pcm = (value * 1000.0).round() as u32;

    let p = resolve(ID_POWER_POLICIES_SET_STATUS)
        .ok_or_else(|| "NvAPI_GPU_ClientPowerPoliciesSetStatus недоступна".to_string())?;
    let f: unsafe extern "C" fn(*mut c_void, *mut PowerPolicyStatus) -> i32 = unsafe { std::mem::transmute(p) };
    let rc = unsafe { f(h, &mut status) };
    if rc != NVAPI_OK {
        return Err(nv_err("лимит мощности", rc));
    }
    Ok(value)
}

/// Уровень вентиляторов в процентах; `None` — вернуть управление драйверу.
pub fn set_fan_level(level: Option<u32>) -> Result<(), String> {
    let h = first_gpu()?;
    let (mut control, version) = read_fan_control_raw(h)?;
    control.version = version;

    let count = control.count.min(32) as usize;
    if count == 0 {
        return Err("Драйвер не сообщил ни одного управляемого вентилятора".into());
    }

    // Границы уровня берём из status: у каждого кулера свой минимум оборотов.
    let status_levels: Vec<(u32, u32, u32)> = read_fans(h)
        .iter()
        .map(|f| (f.cooler_id, f.min_level, f.max_level))
        .collect();

    for item in control.items[..count].iter_mut() {
        match level {
            None => {
                item.mode = 0;
            }
            Some(l) => {
                let (lo, hi) = status_levels
                    .iter()
                    .find(|(id, _, _)| *id == item.cooler_id)
                    .map(|(_, lo, hi)| (*lo, *hi))
                    .unwrap_or((30, 100));
                item.mode = 1;
                item.level = l.clamp(lo, hi);
            }
        }
    }

    let p = resolve(ID_FAN_COOLERS_SET_CONTROL)
        .ok_or_else(|| "NvAPI_GPU_ClientFanCoolersSetControl недоступна".to_string())?;
    let f: unsafe extern "C" fn(*mut c_void, *mut FanCoolersControl) -> i32 = unsafe { std::mem::transmute(p) };
    let rc = unsafe { f(h, &mut control) };
    if rc != NVAPI_OK {
        return Err(nv_err("режим вентиляторов", rc));
    }
    Ok(())
}

/// Вернуть карту к штатному состоянию: нулевые смещения, штатный лимит, авто-вентиляторы.
///
/// Выполняет все три шага даже при ошибке одного из них — сбросить максимум
/// возможного важнее, чем прерваться на первой неудаче.
pub fn reset_all() -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();

    if let Err(e) = set_clock_offsets(0, 0) {
        problems.push(e);
    }
    match first_gpu().and_then(|h| {
        read_power_info(h).ok_or_else(|| "нет границ лимита мощности".to_string())
    }) {
        Ok((_, def_pcm, _)) => {
            if let Err(e) = set_power_limit_percent(def_pcm as f32 / 1000.0) {
                problems.push(e);
            }
        }
        Err(e) => problems.push(e),
    }
    if let Err(e) = set_fan_level(None) {
        problems.push(e);
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// Диагностика для проверки раскладки control-структуры перед записью в вентиляторы.
pub fn fan_control_dump() -> Result<Vec<(u32, u32, u32)>, String> {
    let h = first_gpu()?;
    let (c, _) = read_fan_control_raw(h)?;
    Ok(c.items[..c.count.min(32) as usize]
        .iter()
        .map(|i| (i.cooler_id, i.level, i.mode))
        .collect())
}

#[cfg(test)]
mod tests {
    /// Диагностический дамп: `cargo test --lib nvapi -- --nocapture --ignored`
    /// Значения сверяются с `nvidia-smi` вручную — так проверяются раскладки структур.
    #[test]
    #[ignore]
    fn dump() {
        println!("--- capabilities ---");
        println!("{}", serde_json::to_string_pretty(&super::capabilities()).unwrap());
        println!("--- telemetry ---");
        match super::telemetry() {
            Some(t) => println!("{}", serde_json::to_string_pretty(&t).unwrap()),
            None => println!("NVAPI недоступна"),
        }
        // Сверка control-структуры со status: id кулеров должны совпасть,
        // иначе раскладка неверна и писать в вентиляторы нельзя.
        println!("--- fan control (cooler_id, level, mode) ---");
        match super::fan_control_dump() {
            Ok(v) => println!("{:?}", v),
            Err(e) => println!("ошибка: {}", e),
        }
    }

    /// Проверка пути записи на самом безопасном рычаге: лимит мощности вниз и обратно.
    /// Понижение лимита не может ничего повредить и мгновенно обратимо.
    /// `cargo test --lib nvapi::tests::write -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn write_roundtrip() {
        let before = super::telemetry().expect("NVAPI недоступна");
        let start = before.power_current_percent.unwrap_or(100.0);
        println!("было: лимит {start:.1} %, смещения {:+}/{:+} МГц",
                 before.core_offset.current_mhz, before.mem_offset.current_mhz);

        let applied = super::set_power_limit_percent(90.0).expect("запись лимита не удалась");
        println!("записали: {applied:.1} %");

        let mid = super::telemetry().expect("чтение после записи");
        let now = mid.power_current_percent.unwrap_or(0.0);
        println!("прочитали обратно: {now:.1} %");
        assert!((now - applied).abs() < 0.5, "карта не приняла новый лимит: ждали {applied}, получили {now}");

        let restored = super::set_power_limit_percent(start).expect("возврат лимита не удался");
        let after = super::telemetry().expect("чтение после возврата");
        println!("вернули: {restored:.1} %, карта сообщает {:.1} %", after.power_current_percent.unwrap_or(0.0));
        assert!((after.power_current_percent.unwrap_or(0.0) - start).abs() < 0.5, "не удалось вернуть исходный лимит");
    }
}
