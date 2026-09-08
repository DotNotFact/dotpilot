import { invoke } from "@tauri-apps/api/core";

export type Policy = "direct" | "vpn" | "radmin";

export interface AppEntry {
  id: string;
  name: string;
  exe_paths: string[];
  kind: "game" | "browser" | "dev" | "tool" | string;
  policy: Policy;
  dscp: number | null;
  throttle_mbps: number | null;
  background: boolean;
  color: string;
  note: string;
}

export interface PingTarget {
  id: string;
  name: string;
  host: string;
  color: string;
}

export interface Profile {
  id: string;
  name: string;
  description: string;
  power_plan: string;
  game_mode: boolean;
  zapret_running: boolean | null;
  happ_running: boolean | null;
  icon: string;
}

export interface Config {
  apps: AppEntry[];
  ping_targets: PingTarget[];
  profiles: Profile[];
  active_profile: string;
  auto_game_mode: boolean;
  sync_happ_config: boolean;
  happ_config_path: string;
  happ_exe: string;
  happ_service: string;
  zapret_dir: string;
  zapret_service: string;
  radmin_exe: string;
  radmin_service: string;
  radmin_alias: string;
  anthropic_api_key: string;
  ai_model: string;
  ai_proxy: string;
  ai_effort: string;
  poll_ms: number;
  /** Токен бота Telegram для уведомлений. */
  telegram_bot_token: string;
  /** Идентификатор чата для уведомлений. */
  telegram_chat_id: string;
  /** Отправлять ли уведомления о проблемах. */
  notify_enabled: boolean;
  /** Приостанавливать запись в железо, пока работает античит. */
  anticheat_safe_mode: boolean;
  /** Цена киловатт-часа для подсчёта стоимости энергии. */
  power_tariff: number;
  tg_exe: string;
  tgws_exe: string;
  tgws_autostart: boolean;
  tg_proxy_link: string;
  headphone_products: string[];
  proxy_guard: boolean;
  proxy_port: number;
}

export interface Adapter {
  name: string;
  description: string;
  status: string;
  link_speed: string;
  if_index: number;
  mac: string;
  metric: number | null;
  automatic_metric: boolean | null;
  connected: boolean;
  ipv4: string[];
  dns: string[];
  role: "wifi" | "ethernet" | "happ" | "radmin" | "virtual" | "other";
  rx_bps: number;
  tx_bps: number;
}

export interface Route {
  prefix: string;
  next_hop: string;
  interface: string;
  if_index: number;
  route_metric: number;
  interface_metric: number;
}

export interface ServiceStatus {
  id: "happ" | "zapret" | "radmin";
  name: string;
  service: string;
  service_state: string;
  gui_running: boolean;
  gui_pids: number[];
  detail: string;
  mode: string;
  proxied_apps: string[];
}

export interface HappConfigInfo {
  exists: boolean;
  tun_enabled: boolean;
  strict_route: boolean;
  final_outbound: string;
  proxied_paths: string[];
  direct_processes: string[];
}

export interface ProcInfo {
  pid: number;
  name: string;
  exe: string;
  cpu: number;
  mem_mb: number;
}

export interface AppStatus {
  id: string;
  running: boolean;
  procs: ProcInfo[];
  in_happ_list: boolean;
  qos_rules: number;
}

export interface CpuInfo {
  name: string;
  cores: number;
  usage: number;
  per_core: number[];
  freq_mhz: number;
}
export interface MemInfo {
  total_mb: number;
  used_mb: number;
  percent: number;
}
export interface GpuInfo {
  available: boolean;
  name: string;
  temp_c: number;
  util_pct: number;
  mem_used_mb: number;
  mem_total_mb: number;
  power_w: number;
  clock_mhz: number;
  mem_clock_mhz: number;
  fan_pct: number;
  /** Штатный лимит мощности в ваттах — база для процентов из NVAPI. */
  power_limit_w: number;
}
export interface NetRate {
  name: string;
  rx_bps: number;
  tx_bps: number;
  rx_total: number;
  tx_total: number;
}
export interface GameModeState {
  active: boolean;
  auto: boolean;
  manual: boolean | null;
  trigger: string;
}
export interface QosPolicy {
  Name: string;
  AppPathNameMatchCondition: string | null;
  DSCPAction: number | null;
  ThrottleRateActionBitsPerSecond: number | null;
}
export interface PowerPlan {
  guid: string;
  name: string;
  active: boolean;
}
export interface Sample {
  t: number;
  rtt: number | null;
}
export interface Stats {
  last: number | null;
  avg: number | null;
  min: number | null;
  max: number | null;
  jitter: number | null;
  loss_pct: number;
  sent: number;
  lost: number;
  resolved_ip: string;
  reachable: boolean;
}
export interface TargetSeries {
  id: string;
  name: string;
  host: string;
  color: string;
  stats: Stats;
  samples: Sample[];
}
export interface LogEntry {
  ts: number;
  level: string;
  msg: string;
}

export interface SelfStats {
  pid: number;
  cpu_pct: number;
  cpu_total_pct: number;
  mem_mb: number;
  webview_mem_mb: number;
  webview_cpu_pct: number;
  webview_procs: number;
  ps_calls: number;
  threads: number;
  cpu_ms: number;
  wall_ms: number;
}

export interface ProcRow {
  pid: number;
  parent: number;
  name: string;
  exe: string;
  cpu: number;
  mem_mb: number;
  gpu_mb: number;
  services: string[];
  flags: string[];
  children: number;
}

export interface TgwsStatus {
  running: boolean;
  pid: number;
  ports: string[];
  exe_exists: boolean;
  telegram_running: boolean;
}

export interface ProxyEnv {
  system_enabled: boolean;
  system_server: string;
  system_override: string;
  env_http: string;
  env_https: string;
  env_no_proxy: string;
}
export interface ProxyState {
  port: number;
  alive: boolean;
  env: ProxyEnv;
  env_points_local: boolean;
  exceptions_ok: boolean;
  guard_enabled: boolean;
  guard_holding: boolean;
  problems: string[];
}

export interface Snapshot {
  ts: number;
  admin: boolean;
  self_stats: SelfStats;
  hidden: boolean;
  proxy: ProxyState;
  adapters: Adapter[];
  routes: Route[];
  wifi: [string, string][];
  services: ServiceStatus[];
  happ: HappConfigInfo;
  system_proxy_enabled: boolean;
  system_proxy: string;
  apps: AppStatus[];
  cpu: CpuInfo;
  mem: MemInfo;
  gpu: GpuInfo;
  net_rates: NetRate[];
  game_mode: GameModeState;
  qos: QosPolicy[];
  power_plans: PowerPlan[];
  ping: TargetSeries[];
  events: LogEntry[];
  default_via: string;
}

export interface Connection {
  pid: number;
  proto: string;
  local: string;
  remote: string;
  state: string;
  via: string;
}

export interface Tweak {
  id: string;
  name: string;
  description: string;
  state: boolean | null;
  reboot: boolean;
}

export interface ApplyReport {
  ok: boolean;
  lines: string[];
}

export interface AudioDevice {
  id: string;
  instance_id: string;
  name: string;
  flow: "playback" | "capture";
  state: "active" | "disabled" | "unplugged" | "notpresent" | "unknown";
  is_default: boolean;
  is_default_comm: boolean;
  hands_free: boolean;
  bluetooth: boolean;
  product: string;
  apo: boolean;
  apo_backup: boolean;
  enhancements_disabled: boolean;
}
export interface EqStatus {
  installed: boolean;
  include_present: boolean;
  preset: string;
  device_filter: string;
  config_path: string;
}
export interface AudioInfo {
  module_installed: boolean;
  devices: AudioDevice[];
  eq: EqStatus;
}
export interface PermItem {
  id: string;
  name: string;
  description: string;
  effect: string;
  state: boolean | null;
  can_grant: boolean;
  needs_admin: boolean;
  detail: string;
}

/** Данные NVAPI — то, чего не отдаёт nvidia-smi. */
export interface GpuFanState {
  cooler_id: number;
  rpm: number;
  level_percent: number;
  min_level: number;
  max_level: number;
  automatic: boolean;
}

export interface GpuClockOffset {
  current_mhz: number;
  min_mhz: number;
  max_mhz: number;
  editable: boolean;
}

export interface GpuTempSensor {
  target: string;
  current_c: number;
  max_c: number;
}

export interface GpuNvapi {
  name: string;
  temperatures: GpuTempSensor[];
  clock_current_mhz: number | null;
  clock_base_mhz: number | null;
  clock_boost_mhz: number | null;
  mem_clock_current_mhz: number | null;
  util_gpu_percent: number | null;
  util_fb_percent: number | null;
  power_current_percent: number | null;
  power_min_percent: number | null;
  power_default_percent: number | null;
  power_max_percent: number | null;
  fans: GpuFanState[];
  core_offset: GpuClockOffset;
  mem_offset: GpuClockOffset;
}

/** Какие рычаги управления реально существуют на этой машине. */
export interface GpuCapabilities {
  nvapi: boolean;
  gpu_count: number;
  read_thermals: boolean;
  read_clocks: boolean;
  read_pstates: boolean;
  set_clock_offsets: boolean;
  set_power_limit: boolean;
  set_fan_curve: boolean;
  set_voltage_direct: boolean;
}

/** Настройка разгона. `fan_level: null` — вентиляторами управляет драйвер. */
export interface GpuCandidate {
  core_offset_mhz: number;
  mem_offset_mhz: number;
  power_percent: number;
  fan_level: number | null;
}

export type OcStage = "smoke" | "medium" | "long";

export interface OcBounds {
  core_min_mhz: number;
  core_max_mhz: number;
  mem_min_mhz: number;
  mem_max_mhz: number;
  power_min_percent: number;
  power_max_percent: number;
  fan_min_percent: number;
  fan_max_percent: number;
}

export interface OcPending {
  candidate: GpuCandidate;
  stage: OcStage;
  applied_at: number;
  boot_id: number;
}

export interface OcRejected {
  candidate: GpuCandidate;
  reason: string;
  at: number;
}

export interface OcEvent {
  at: number;
  kind: string;
  text: string;
}

export interface OcJournal {
  pending: OcPending | null;
  last_known_good: GpuCandidate | null;
  rejected: OcRejected[];
  history: OcEvent[];
  bounds: OcBounds;
}

export interface OcApplyReport {
  applied: GpuCandidate;
  clamped: boolean;
  stage: OcStage;
  stage_seconds: number;
  message: string;
}

/** Что собрал тест за время ступени. */
export interface StageEvidence {
  gpu_mismatches: number;
  started_at: number;
  peak_temp_c: number | null;
  completed: boolean;
}

export interface StageVerdict {
  passed: boolean;
  reason: string;
  faults: string[];
  journal: OcJournal;
}

export interface StressResult {
  kind: string;
  seconds: number;
  threads: number;
  passes: number;
  mismatches: number;
  features: string[];
  note: string;
}

/** Предложение модели в сыром виде, до обрезки коридором. */
export interface OcProposal {
  core_offset_mhz: number;
  mem_offset_mhz: number;
  power_percent: number;
  fan_level: number | null;
  reasoning: string;
  expectation: string;
  stop: boolean;
  confidence?: string | null;
}

export interface OcSuggestion {
  proposal: OcProposal;
  /** То, во что предложение превратилось после обрезки. */
  candidate: GpuCandidate;
  clamped: boolean;
  model: string;
  input_tokens: number;
  output_tokens: number;
}

/** Замер платформы: то, что меняется в BIOS, плюс результаты встроенных тестов. */
export interface MemoryModule {
  bank: string;
  capacity_gb: number;
  configured_mts: number;
  configured_mv: number;
  part_number: string;
}

export interface MemoryConfig {
  modules: MemoryModule[];
  total_gb: number;
  profile_enabled: boolean;
  verdict: string;
}

export interface FirmwareInfo {
  board: string;
  bios_vendor: string;
  bios_version: string;
  bios_date: string;
  cpu: string;
  cpu_base_mhz: number;
  cores: number;
  threads: number;
}

export interface Baseline {
  at: number;
  label: string;
  firmware: FirmwareInfo;
  memory: MemoryConfig;
  loaded_clock_mhz: number | null;
  cpu_passes_per_sec: number;
  memory_mb_per_sec: number;
  cpu_mismatches: number;
  memory_mismatches: number;
}

export interface BaselineStore {
  items: Baseline[];
}

export interface Comparison {
  cpu_delta_percent: number;
  memory_delta_percent: number;
  clock_delta_mhz: number | null;
  memory_profile_changed: boolean;
  bios_version_changed: boolean;
  stable: boolean;
  summary: string;
}

export interface BiosSetting {
  path: string;
  value: string;
  why: string;
  risk: string;
}

export interface BiosAdvice {
  settings: BiosSetting[];
  order: string;
  verify: string;
  expected_gain: string;
  warnings: string[];
}

export interface BiosSuggestion {
  advice: BiosAdvice;
  model: string;
  input_tokens: number;
  output_tokens: number;
}

/** Вентиляторы платы через ACPI-интерфейс Gigabyte. */
export interface BoardSensor {
  id: number;
  celsius: number;
  connected: boolean;
}

export interface BoardFan {
  id: number;
  stop_enabled: boolean;
  target_sensor: number;
  off_limit_c: number;
  on_limit_c: number;
}

export interface FanControllerState {
  available: boolean;
  hw_id: number | null;
  sensors: BoardSensor[];
  fans: BoardFan[];
  backend: string;
  note: string;
}

/** Оценка состояния ПК: измерение, норма и расхождение. */
export type Severity = "ok" | "notice" | "warning" | "problem";

export interface Finding {
  area: string;
  title: string;
  severity: Severity;
  measured: string;
  reference: string;
  advice: string | null;
}

export interface HealthReport {
  at: number;
  findings: Finding[];
  problems: number;
  warnings: number;
  summary: string;
}

/** Регуляторы напряжения из ACPI-интерфейса платы. Экспериментально. */
export interface VoltageItem {
  id: number;
  name: string;
  current_mv: number;
  default_mv: number;
  firmware_min_mv: number;
  firmware_max_mv: number;
  step_mv: number;
  is_offset: boolean;
  adjustable: boolean;
  allowed_min_mv: number;
  allowed_max_mv: number;
  note: string;
}

export interface VoltageState {
  available: boolean;
  interface_version: string | null;
  items: VoltageItem[];
  backend: string;
  note: string;
}

/** Долгие наблюдения: деградация охлаждения и расход электричества. */
export interface CoolingTrend {
  enough_data: boolean;
  gpu_delta_c: number | null;
  cpu_delta_c: number | null;
  compared: number;
  verdict: string;
}

export interface EnergyReport {
  gpu_kwh: number;
  gpu_cost: number;
  tariff: number;
  span_days: number;
  avg_watts: number;
  note: string;
}

export interface TrendReport {
  samples: number;
  span_days: number;
  cooling: CoolingTrend;
  energy: EnergyReport;
}

/** Сосуществование с античитами: что приложение делает и блокируется ли запись. */
export interface DetectedAntiCheat {
  process: string;
  product: string;
  pid: number;
}

export interface Activity {
  what: string;
  how: string;
  /** ordinary — как обычные приложения, notable — заметно, paused — приостановлено. */
  kind: "ordinary" | "notable" | "paused" | string;
  detail: string;
}

export interface AntiCheatStatus {
  safe_mode: boolean;
  detected: DetectedAntiCheat[];
  writes_blocked: boolean;
  activities: Activity[];
  note: string;
}

/** Обслуживание: что и когда пора делать. */
export interface MaintenanceTask {
  id: string;
  name: string;
  every_days: number;
  last_run: number | null;
  days_since: number | null;
  due: boolean;
  /** Может ли приложение выполнить задачу само. */
  automatic: boolean;
  why: string;
  where_to: string;
}

export interface NotifyResult {
  sent: boolean;
  detail: string;
}

export const api = {
  maintenanceState: () => invoke<MaintenanceTask[]>("maintenance_state"),
  maintenanceDone: (id: string) => invoke<MaintenanceTask[]>("maintenance_done", { id }),
  notifyTest: () => invoke<NotifyResult>("notify_test"),
  hardwareReport: (path?: string) => invoke<string>("hardware_report", { path: path ?? null }),
  anticheatStatus: () => invoke<AntiCheatStatus>("anticheat_status"),
  anticheatSetMode: (on: boolean) => invoke<void>("anticheat_set_mode", { on }),
  trendsReport: () => invoke<TrendReport>("trends_report"),
  voltageState: () => invoke<VoltageState>("voltage_state"),
  voltageSetOffset: (id: number, millivolts: number) => invoke<number>("voltage_set_offset", { id, millivolts }),
  voltageReset: () => invoke<void>("voltage_reset"),
  healthCheck: () => invoke<HealthReport>("health_check"),
  healthAdvice: (report: HealthReport) => invoke<string>("health_advice", { report }),
  fansState: () => invoke<FanControllerState>("fans_state"),
  fansSetLimits: (id: number, offC: number, onC: number) => invoke<[number, number]>("fans_set_limits", { id, offC, onC }),
  fansSetZero: (id: number, enabled: boolean) => invoke<void>("fans_set_zero", { id, enabled }),
  fansSetSensor: (id: number, sensor: number) => invoke<void>("fans_set_sensor", { id, sensor }),
  fansForceOn: (id: number) => invoke<void>("fans_force_on", { id }),
  fansRestore: () => invoke<void>("fans_restore"),
  platformState: () => invoke<[FirmwareInfo, MemoryConfig]>("platform_state"),
  platformMeasure: (label: string, seconds = 8, memoryMb = 512) =>
    invoke<BaselineStore>("platform_measure", { label, seconds, memoryMb }),
  platformBaselines: () => invoke<[BaselineStore, Comparison | null]>("platform_baselines"),
  biosAdvice: (question = "") => invoke<BiosSuggestion>("bios_advice", { question }),
  ocPropose: (note = "") => invoke<OcSuggestion>("oc_propose", { note }),
  ocValidate: (evidence: StageEvidence) => invoke<StageVerdict>("oc_validate", { evidence }),
  benchCpu: (seconds: number, threads = 0) => invoke<StressResult>("bench_cpu", { seconds, threads }),
  benchMemory: (megabytes: number, seconds: number) => invoke<StressResult>("bench_memory", { megabytes, seconds }),
  gpuPeakTemp: (samples: number, intervalMs: number) => invoke<number | null>("gpu_peak_temp", { samples, intervalMs }),
  gpuNvapi: () => invoke<GpuNvapi>("gpu_nvapi"),
  gpuCapabilities: () => invoke<GpuCapabilities>("gpu_capabilities"),
  ocState: () => invoke<OcJournal>("oc_state"),
  ocApply: (candidate: GpuCandidate, stage: OcStage) => invoke<OcApplyReport>("oc_apply", { candidate, stage }),
  ocConfirm: () => invoke<OcJournal>("oc_confirm"),
  ocReject: (reason: string) => invoke<OcJournal>("oc_reject", { reason }),
  ocReset: () => invoke<OcJournal>("oc_reset"),
  proxyFix: (action: "clear-env" | "set-env" | "no-proxy" | "disable-system" | "enable-system") => invoke<string>("proxy_fix", { action }),
  aiReport: (path?: string) => invoke<string>("ai_report", { path: path ?? null }),
  selfSample: () => invoke<SelfStats>("self_sample"),
  benchMode: (hidden: boolean) => invoke<void>("bench_mode", { hidden }),
  openUri: (uri: string) => invoke<void>("open_uri", { uri }),
  processes: (limit = 60) => invoke<ProcRow[]>("get_processes", { limit }),
  killProcess: (pid: number) => invoke<string>("kill_process", { pid }),
  soundFixFootsteps: (products: string[], preset = "footsteps") => invoke<string[]>("sound_fix_footsteps", { products, preset }),
  soundReset: () => invoke<string[]>("sound_reset"),
  apoToggle: (id: string, attach: boolean) => invoke<string>("apo_toggle", { id, attach }),
  tgwsStatus: () => invoke<TgwsStatus>("tgws_status"),
  tgwsControl: (action: "start" | "stop" | "launch-telegram") => invoke<string>("tgws_control", { action }),
  resetConfig: () => invoke<void>("reset_config"),
  tweaksReset: () => invoke<string[]>("tweaks_reset"),
  removeQos: () => invoke<void>("remove_qos_rules"),
  audioInfo: () => invoke<AudioInfo>("audio_info"),
  audioSetDefault: (id: string, role: "playback" | "comm" | "both") => invoke<string>("audio_set_default", { id, role }),
  audioSetEnabled: (instanceId: string, enabled: boolean) => invoke<string>("audio_set_enabled", { instanceId, enabled }),
  eqApply: (preset: string, device: string) => invoke<string>("eq_apply", { preset, device }),
  openSound: (target: string) => invoke<number>("open_sound", { target }),
  permsStatus: () => invoke<PermItem[]>("perms_status"),
  permsGrant: (id: string, on: boolean) => invoke<string>("perms_grant", { id, on }),
  relaunchElevated: () => invoke<void>("relaunch_elevated"),
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  config: () => invoke<Config>("get_config"),
  saveConfig: (cfg: Config) => invoke<void>("save_config", { cfg }),
  applyPolicies: () => invoke<ApplyReport>("apply_policies"),
  setGameMode: (mode: boolean | null) => invoke<GameModeState>("set_game_mode", { mode }),
  setInterfaceMetric: (ifIndex: number, metric: number | null) =>
    invoke<string>("set_interface_metric", { ifIndex, metric }),
  setAdapterEnabled: (name: string, enabled: boolean) =>
    invoke<string>("set_adapter_enabled", { name, enabled }),
  serviceControl: (id: string, action: "start" | "stop" | "launch" | "kill") =>
    invoke<string>("service_control", { id, action }),
  launchApp: (appId: string) => invoke<string>("launch_app", { appId }),
  connections: (appId: string) => invoke<Connection[]>("get_connections", { appId }),
  tweaks: () => invoke<Tweak[]>("tweaks_state"),
  setTweak: (id: string, on: boolean) => invoke<string>("set_tweak", { id, on }),
  quickAction: (id: string) => invoke<string>("quick_action", { id }),
  setPowerPlan: (guid: string) => invoke<void>("set_power_plan", { guid }),
  applyProfile: (id: string) => invoke<string[]>("apply_profile", { id }),
  exportSnapshot: (path: string, format: "json" | "csv") =>
    invoke<string>("export_snapshot", { path, format }),
  askClaude: (question: string) => invoke<string>("ask_claude", { question }),
  configPath: () => invoke<string>("config_file_path"),
  log: (level: string, msg: string) => invoke<void>("app_log", { level, msg }),
};

export function fmtBps(bps: number): string {
  if (!isFinite(bps) || bps < 0) return "0 б/с";
  if (bps < 1000) return `${bps.toFixed(0)} б/с`;
  if (bps < 1e6) return `${(bps / 1e3).toFixed(1)} Кб/с`;
  if (bps < 1e9) return `${(bps / 1e6).toFixed(2)} Мб/с`;
  return `${(bps / 1e9).toFixed(2)} Гб/с`;
}

export function fmtMs(v: number | null | undefined, digits = 0): string {
  if (v === null || v === undefined || !isFinite(v)) return "—";
  return `${v.toFixed(digits)} мс`;
}

export function timeHM(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}
