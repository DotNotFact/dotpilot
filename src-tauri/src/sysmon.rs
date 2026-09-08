//! CPU / RAM / GPU / network throughput / process discovery via sysinfo + nvidia-smi.
use crate::config::AppEntry;
use crate::ps::run_cmd;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;
use sysinfo::{Networks, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Serialize, Clone, Debug, Default)]
pub struct CpuInfo {
    pub name: String,
    pub cores: usize,
    pub usage: f32,
    pub per_core: Vec<f32>,
    pub freq_mhz: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct MemInfo {
    pub total_mb: u64,
    pub used_mb: u64,
    pub percent: f32,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct GpuInfo {
    pub available: bool,
    pub name: String,
    pub temp_c: f32,
    pub util_pct: f32,
    pub mem_used_mb: f32,
    pub mem_total_mb: f32,
    pub power_w: f32,
    pub clock_mhz: f32,
    pub mem_clock_mhz: f32,
    pub fan_pct: f32,
    /// Штатный лимит мощности в ваттах — база, к которой NVAPI даёт проценты.
    pub power_limit_w: f32,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct NetRate {
    pub name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_total: u64,
    pub tx_total: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub cpu: f32,
    pub mem_mb: u64,
}

pub struct SysCollector {
    sys: System,
    nets: Networks,
    last_net: Instant,
    pub cpu_name: String,
}

impl SysCollector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        let cpu_name = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
        Self {
            sys,
            nets: Networks::new_with_refreshed_list(),
            last_net: Instant::now(),
            cpu_name,
        }
    }

    pub fn cpu(&mut self) -> CpuInfo {
        self.sys.refresh_cpu_all();
        let per_core: Vec<f32> = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let usage = self.sys.global_cpu_usage();
        let freq = self.sys.cpus().iter().map(|c| c.frequency()).max().unwrap_or(0);
        CpuInfo {
            name: self.cpu_name.trim().to_string(),
            cores: per_core.len(),
            usage,
            per_core,
            freq_mhz: freq,
        }
    }

    pub fn mem(&mut self) -> MemInfo {
        self.sys.refresh_memory();
        let total = self.sys.total_memory() / 1024 / 1024;
        let used = self.sys.used_memory() / 1024 / 1024;
        MemInfo {
            total_mb: total,
            used_mb: used,
            percent: if total > 0 { used as f32 * 100.0 / total as f32 } else { 0.0 },
        }
    }

    pub fn net_rates(&mut self) -> Vec<NetRate> {
        let elapsed = self.last_net.elapsed().as_secs_f64().max(0.05);
        self.nets.refresh(true);
        self.last_net = Instant::now();
        let mut v: Vec<NetRate> = self
            .nets
            .iter()
            .map(|(name, d)| NetRate {
                name: name.clone(),
                rx_bps: d.received() as f64 * 8.0 / elapsed,
                tx_bps: d.transmitted() as f64 * 8.0 / elapsed,
                rx_total: d.total_received(),
                tx_total: d.total_transmitted(),
            })
            .collect();
        v.sort_by(|a, b| b.rx_total.cmp(&a.rx_total));
        v
    }

    /// Refresh processes and find which configured apps are running.
    pub fn find_apps(&mut self, apps: &[AppEntry]) -> HashMap<String, Vec<ProcInfo>> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet).with_cpu().with_memory(),
        );
        let mut out: HashMap<String, Vec<ProcInfo>> = HashMap::new();
        let needles: Vec<(String, Vec<String>)> = apps
            .iter()
            .map(|a| (a.id.clone(), a.exe_paths.iter().map(|p| p.to_lowercase()).collect()))
            .collect();
        for (pid, p) in self.sys.processes() {
            let exe = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            if exe.is_empty() {
                continue;
            }
            let exe_l = exe.to_lowercase();
            for (id, paths) in &needles {
                let hit = paths.iter().any(|needle| {
                    if needle.ends_with(".exe") {
                        exe_l == *needle
                    } else {
                        // directory prefix match
                        exe_l.starts_with(needle.trim_end_matches('\\')) && exe_l.len() > needle.len()
                    }
                });
                if hit {
                    out.entry(id.clone()).or_default().push(ProcInfo {
                        pid: pid.as_u32(),
                        name: p.name().to_string_lossy().to_string(),
                        exe: exe.clone(),
                        cpu: p.cpu_usage(),
                        mem_mb: p.memory() / 1024 / 1024,
                    });
                }
            }
        }
        out
    }

    pub fn find_by_name(&mut self, names: &[&str]) -> Vec<ProcInfo> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
        );
        let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        self.sys
            .processes()
            .iter()
            .filter(|(_, p)| lower.contains(&p.name().to_string_lossy().to_lowercase()))
            .map(|(pid, p)| ProcInfo {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().to_string(),
                exe: p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default(),
                cpu: p.cpu_usage(),
                mem_mb: p.memory() / 1024 / 1024,
            })
            .collect()
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct SelfStats {
    pub pid: u32,
    pub cpu_pct: f32,       // of one core, normalised to total machine % below
    pub cpu_total_pct: f32, // share of the whole CPU
    pub mem_mb: u64,
    pub webview_mem_mb: u64,
    pub webview_cpu_pct: f32,
    pub webview_procs: usize,
    pub ps_calls: u64,
    pub threads: usize,
    /// Accumulated CPU time (ms) of the host + WebView2 family: exact, sampling-independent.
    pub cpu_ms: u64,
    pub wall_ms: i64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ProcRow {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub exe: String,
    pub cpu: f32, // % of whole machine
    pub mem_mb: u64,
    pub gpu_mb: f32,
    pub services: Vec<String>,
    pub flags: Vec<String>,
    pub children: usize,
}

const MINER_NAMES: &[&str] = &["xmrig", "minerd", "nbminer", "t-rex", "lolminer", "phoenixminer", "ethminer", "cpuminer", "kryptex", "nicehash", "gminer", "srbminer", "teamredminer", "xmr-stak", "bminer", "claymore"];
const CRITICAL: &[&str] = &["system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "memory compression", "msmpeng.exe", "audiodg.exe", "dotpilot.exe"];

impl SysCollector {
    /// DotPilot's own footprint: the Rust host process plus its WebView2 children.
    pub fn self_stats(&self) -> SelfStats {
        let me = sysinfo::Pid::from_u32(std::process::id());
        let cores = self.sys.cpus().len().max(1) as f32;
        let mut st = SelfStats { pid: me.as_u32(), ps_calls: crate::ps::PS_CALLS.load(std::sync::atomic::Ordering::Relaxed), ..Default::default() };
        if let Some(p) = self.sys.process(me) {
            st.cpu_pct = p.cpu_usage();
            st.mem_mb = p.memory() / 1024 / 1024;
            st.threads = p.tasks().map(|t| t.len()).unwrap_or(0);
            st.cpu_ms = p.accumulated_cpu_time();
        }
        st.wall_ms = chrono::Utc::now().timestamp_millis();
        // WebView2 processes are (grand)children of our pid
        let mut family: Vec<sysinfo::Pid> = vec![me];
        for _ in 0..3 {
            let mut add = Vec::new();
            for (pid, p) in self.sys.processes() {
                if let Some(par) = p.parent() {
                    if family.contains(&par) && !family.contains(pid) {
                        add.push(*pid);
                    }
                }
            }
            if add.is_empty() {
                break;
            }
            family.extend(add);
        }
        for pid in family.iter().skip(1) {
            if let Some(p) = self.sys.process(*pid) {
                st.webview_mem_mb += p.memory() / 1024 / 1024;
                st.webview_cpu_pct += p.cpu_usage();
                st.webview_procs += 1;
                st.cpu_ms += p.accumulated_cpu_time();
            }
        }
        st.cpu_total_pct = (st.cpu_pct + st.webview_cpu_pct) / cores;
        st.webview_cpu_pct /= cores;
        st.cpu_pct /= cores;
        st
    }

    /// Top N processes by CPU from the last refresh (no extra refresh). `% of whole machine`.
    pub fn top_cpu(&self, n: usize) -> Vec<(String, f32)> {
        let cores = self.sys.cpus().len().max(1) as f32;
        let mut v: Vec<(String, f32)> = self
            .sys
            .processes()
            .values()
            .map(|p| (p.name().to_string_lossy().to_string(), p.cpu_usage() / cores))
            .filter(|(name, c)| *c > 0.5 && name != "Idle" && name != "System Idle Process")
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    /// Refresh only DotPilot's own process family and return its footprint (cheap, for the benchmark).
    pub fn self_sample(&mut self) -> SelfStats {
        let me = sysinfo::Pid::from_u32(std::process::id());
        let mut family: Vec<sysinfo::Pid> = vec![me];
        for _ in 0..3 {
            let mut add = Vec::new();
            for (pid, p) in self.sys.processes() {
                if let Some(par) = p.parent() {
                    if family.contains(&par) && !family.contains(pid) {
                        add.push(*pid);
                    }
                }
            }
            if add.is_empty() {
                break;
            }
            family.extend(add);
        }
        self.sys.refresh_processes_specifics(ProcessesToUpdate::Some(&family), false, ProcessRefreshKind::nothing().with_cpu().with_memory());
        self.self_stats()
    }

    /// Top processes for the task-manager page. `cpu` is % of the whole machine.
    pub fn top_processes(&mut self, limit: usize, services: &HashMap<u32, Vec<String>>, gpu: &HashMap<u32, f32>) -> Vec<ProcRow> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet).with_cpu().with_memory(),
        );
        let cores = self.sys.cpus().len().max(1) as f32;
        let mut child_count: HashMap<u32, usize> = HashMap::new();
        for p in self.sys.processes().values() {
            if let Some(par) = p.parent() {
                *child_count.entry(par.as_u32()).or_default() += 1;
            }
        }
        let mut rows: Vec<ProcRow> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| {
                let name = p.name().to_string_lossy().to_string();
                let exe = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                let cpu = p.cpu_usage() / cores;
                let mem_mb = p.memory() / 1024 / 1024;
                let lname = name.to_lowercase();
                let lexe = exe.to_lowercase();
                let mut flags = Vec::new();
                if MINER_NAMES.iter().any(|m| lname.contains(m) || lexe.contains(m)) {
                    flags.push("miner-name".into());
                }
                if (lexe.contains("\\temp\\") || lexe.contains("\\appdata\\local\\temp\\")) && cpu > 5.0 {
                    flags.push("temp-exe".into());
                }
                if cpu > 25.0 {
                    flags.push("high-cpu".into());
                }
                if mem_mb > 2048 {
                    flags.push("high-mem".into());
                }
                if lname == "svchost.exe" {
                    flags.push("svchost".into());
                }
                if CRITICAL.contains(&lname.as_str()) {
                    flags.push("critical".into());
                }
                ProcRow {
                    pid: pid.as_u32(),
                    parent: p.parent().map(|x| x.as_u32()).unwrap_or(0),
                    name,
                    exe,
                    cpu,
                    mem_mb,
                    gpu_mb: gpu.get(&pid.as_u32()).copied().unwrap_or(0.0),
                    services: services.get(&pid.as_u32()).cloned().unwrap_or_default(),
                    flags,
                    children: child_count.get(&pid.as_u32()).copied().unwrap_or(0),
                }
            })
            .collect();
        rows.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal).then(b.mem_mb.cmp(&a.mem_mb)));
        rows.truncate(limit);
        rows
    }

    pub fn kill(&mut self, pid: u32) -> anyhow::Result<String> {
        let p = self.sys.process(sysinfo::Pid::from_u32(pid)).ok_or_else(|| anyhow::anyhow!("Процесс {pid} не найден"))?;
        let name = p.name().to_string_lossy().to_string();
        if CRITICAL.contains(&name.to_lowercase().as_str()) {
            return Err(anyhow::anyhow!("{name}: системный процесс, завершать нельзя"));
        }
        if p.kill() {
            Ok(format!("{name} (#{pid}) завершён"))
        } else {
            Err(anyhow::anyhow!("Не удалось завершить {name} (#{pid}): нет прав или процесс защищён"))
        }
    }
}

/// pid → VRAM MB for CUDA/graphics apps known to nvidia-smi.
pub fn gpu_apps() -> HashMap<u32, f32> {
    let mut out = HashMap::new();
    if let Ok(o) = run_cmd("nvidia-smi", &["--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"]) {
        for line in o.lines() {
            let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if f.len() >= 2 {
                if let (Ok(pid), Ok(mb)) = (f[0].parse::<u32>(), f[1].parse::<f32>()) {
                    out.insert(pid, mb);
                }
            }
        }
    }
    out
}

pub fn gpu() -> GpuInfo {
    let out = run_cmd(
        "nvidia-smi",
        &[
            "--query-gpu=name,temperature.gpu,utilization.gpu,memory.used,memory.total,power.draw,clocks.gr,clocks.mem,fan.speed,power.default_limit",
            "--format=csv,noheader,nounits",
        ],
    );
    let Ok(out) = out else { return GpuInfo::default() };
    let line = out.lines().next().unwrap_or("");
    let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if f.len() < 8 {
        return GpuInfo::default();
    }
    let num = |i: usize| f.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    GpuInfo {
        available: true,
        name: f[0].to_string(),
        temp_c: num(1),
        util_pct: num(2),
        mem_used_mb: num(3),
        mem_total_mb: num(4),
        power_w: num(5),
        clock_mhz: num(6),
        mem_clock_mhz: num(7),
        fan_pct: num(8),
        power_limit_w: num(9),
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

pub fn power_plans() -> Vec<PowerPlan> {
    let Ok(out) = run_cmd("powercfg", &["/list"]) else { return vec![] };
    let mut v = Vec::new();
    for line in out.lines() {
        // "... GUID: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx  (Name) *"
        if let Some(idx) = line.find(": ") {
            let rest = &line[idx + 2..];
            let guid: String = rest.chars().take(36).collect();
            if guid.len() == 36 && guid.chars().filter(|c| *c == '-').count() == 4 {
                let name = rest
                    .find('(')
                    .and_then(|s| rest.rfind(')').map(|e| rest[s + 1..e].to_string()))
                    .unwrap_or_default();
                v.push(PowerPlan { guid, name, active: rest.trim_end().ends_with('*') });
            }
        }
    }
    v
}

pub fn set_power_plan(guid: &str) -> anyhow::Result<()> {
    run_cmd("powercfg", &["/setactive", guid])?;
    Ok(())
}
