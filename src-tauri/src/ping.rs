//! Continuous ICMP monitor: one thread per target, ring buffer of samples.
use crate::config::PingTarget;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const HISTORY: usize = 7200; // 2 hours at 1 s

#[derive(Serialize, Clone, Debug)]
pub struct Sample {
    pub t: i64,          // unix ms
    pub rtt: Option<f32>, // None = timeout / loss
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Stats {
    pub last: Option<f32>,
    pub avg: Option<f32>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub jitter: Option<f32>,
    pub loss_pct: f32,
    pub sent: usize,
    pub lost: usize,
    pub resolved_ip: String,
    pub reachable: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct TargetSeries {
    pub id: String,
    pub name: String,
    pub host: String,
    pub color: String,
    pub stats: Stats,
    pub samples: Vec<Sample>,
}

struct TargetState {
    target: PingTarget,
    samples: VecDeque<Sample>,
    resolved: String,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct PingMonitor {
    targets: Mutex<HashMap<String, Arc<Mutex<TargetState>>>>,
    order: Mutex<Vec<String>>,
}

impl PingMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_targets(&self, targets: &[PingTarget]) {
        let mut map = self.targets.lock().unwrap();
        // stop removed / changed
        let keep: Vec<String> = targets.iter().map(|t| t.id.clone()).collect();
        let existing: Vec<String> = map.keys().cloned().collect();
        for id in existing {
            let changed = match targets.iter().find(|t| t.id == id) {
                Some(t) => {
                    let st = map[&id].lock().unwrap();
                    st.target.host != t.host
                }
                None => true,
            };
            if changed || !keep.contains(&id) {
                if let Some(st) = map.remove(&id) {
                    st.lock().unwrap().stop.store(true, Ordering::Relaxed);
                }
            }
        }
        for t in targets {
            if map.contains_key(&t.id) {
                // update cosmetic fields
                let st = map[&t.id].clone();
                st.lock().unwrap().target = t.clone();
                continue;
            }
            let stop = Arc::new(AtomicBool::new(false));
            let state = Arc::new(Mutex::new(TargetState {
                target: t.clone(),
                samples: VecDeque::with_capacity(HISTORY),
                resolved: String::new(),
                stop: stop.clone(),
            }));
            map.insert(t.id.clone(), state.clone());
            let host = t.host.clone();
            thread::Builder::new()
                .name(format!("ping-{}", t.id))
                .spawn(move || ping_loop(host, state, stop))
                .ok();
        }
        *self.order.lock().unwrap() = keep;
    }

    pub fn snapshot(&self, last_n: usize) -> Vec<TargetSeries> {
        let map = self.targets.lock().unwrap();
        let order = self.order.lock().unwrap();
        let mut out = Vec::new();
        for id in order.iter() {
            let Some(st) = map.get(id) else { continue };
            let st = st.lock().unwrap();
            let samples: Vec<Sample> = st.samples.iter().rev().take(last_n).cloned().collect::<Vec<_>>().into_iter().rev().collect();
            out.push(TargetSeries {
                id: st.target.id.clone(),
                name: st.target.name.clone(),
                host: st.target.host.clone(),
                color: st.target.color.clone(),
                stats: compute_stats(&st.samples, &st.resolved),
                samples,
            });
        }
        out
    }

    pub fn all_samples(&self) -> Vec<TargetSeries> {
        self.snapshot(HISTORY)
    }
}

pub fn compute_stats(samples: &VecDeque<Sample>, resolved: &str) -> Stats {
    let recent: Vec<&Sample> = samples.iter().rev().take(60).collect();
    let sent = recent.len();
    let ok: Vec<f32> = recent.iter().filter_map(|s| s.rtt).collect();
    let lost = sent - ok.len();
    let mut st = Stats {
        sent,
        lost,
        loss_pct: if sent > 0 { lost as f32 * 100.0 / sent as f32 } else { 0.0 },
        resolved_ip: resolved.to_string(),
        reachable: recent.iter().take(5).any(|s| s.rtt.is_some()),
        ..Default::default()
    };
    st.last = samples.back().and_then(|s| s.rtt);
    if !ok.is_empty() {
        st.avg = Some(ok.iter().sum::<f32>() / ok.len() as f32);
        st.min = ok.iter().cloned().reduce(f32::min);
        st.max = ok.iter().cloned().reduce(f32::max);
        if ok.len() > 1 {
            let diffs: Vec<f32> = ok.windows(2).map(|w| (w[0] - w[1]).abs()).collect();
            st.jitter = Some(diffs.iter().sum::<f32>() / diffs.len() as f32);
        }
    }
    st
}

fn resolve(host: &str) -> Option<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(ip);
    }
    format!("{host}:0").to_socket_addrs().ok()?.map(|a| a.ip()).find(|ip| ip.is_ipv4())
}

fn ping_loop(host: String, state: Arc<Mutex<TargetState>>, stop: Arc<AtomicBool>) {
    let mut pinger = match winping::Pinger::new() {
        Ok(p) => p,
        Err(_) => return,
    };
    pinger.set_timeout(1000);
    let mut buf = winping::Buffer::new();
    let mut ip: Option<IpAddr> = None;
    let mut last_resolve = Instant::now() - Duration::from_secs(120);
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        if ip.is_none() || last_resolve.elapsed() > Duration::from_secs(60) {
            ip = resolve(&host);
            last_resolve = Instant::now();
            if let Ok(mut st) = state.lock() {
                st.resolved = ip.map(|i| i.to_string()).unwrap_or_default();
            }
        }
        let rtt = match ip {
            Some(addr) => match pinger.send(addr, &mut buf) {
                Ok(ms) => Some(ms as f32),
                Err(_) => None,
            },
            None => None,
        };
        let now = chrono::Utc::now().timestamp_millis();
        if let Ok(mut st) = state.lock() {
            if st.samples.len() >= HISTORY {
                st.samples.pop_front();
            }
            st.samples.push_back(Sample { t: now, rtt });
        }
        let elapsed = started.elapsed();
        if elapsed < Duration::from_millis(1000) {
            thread::sleep(Duration::from_millis(1000) - elapsed);
        }
    }
}
