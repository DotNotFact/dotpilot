import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { api, type SelfStats } from "../lib/api";
import { Section, Tile, Tip } from "../components/ui";
import { Gauge, Play, Copy, Square } from "lucide-react";

type PhaseId = "idle" | "active" | "tray";
interface Phase {
  id: PhaseId;
  name: string;
  seconds: number;
  what: string;
}
const PHASES: Phase[] = [
  { id: "idle", name: "Окно открыто, ничего не делаем", seconds: 20, what: "Обычный режим: сбор данных раз в 3 с, интерфейс обновляется по событию." },
  { id: "active", name: "Активная работа", seconds: 20, what: "Каждую секунду запрашиваем снимок и список процессов, как при быстром переключении страниц." },
  { id: "tray", name: "Свёрнуто в трей", seconds: 20, what: "Режим трея: опрос раз в 8 с, PowerShell раз в 20–60 с, интерфейс не обновляется." },
];

interface Sample {
  t: number;
  phase: PhaseId;
  cpu: number; // % of whole machine over the last interval, from exact CPU-time counters
  mem: number;
  ps: number;
  sysCpu: number;
}

interface PhaseResult {
  phase: Phase;
  n: number;
  cpuAvg: number; // exact: Δcpu_ms / (Δwall_ms × cores)
  cpuMax: number;
  memAvg: number;
  memMax: number;
  psPerMin: number;
  sysCpuAvg: number;
}

export default function Impact() {
  const snap = useStore((s) => s.snap);
  const toast = useStore((s) => s.toast);
  const [running, setRunning] = useState(false);
  const [phaseIdx, setPhaseIdx] = useState(-1);
  const [left, setLeft] = useState(0);
  const [samples, setSamples] = useState<Sample[]>([]);
  const [results, setResults] = useState<PhaseResult[] | null>(null);
  const stopRef = useRef(false);
  const cores = Math.max(snap?.cpu.cores ?? 24, 1);

  useEffect(() => () => { api.benchMode(false); }, []);

  const run = async () => {
    stopRef.current = false;
    setRunning(true);
    setResults(null);
    setSamples([]);
    const all: Sample[] = [];
    const res: PhaseResult[] = [];
    let prev: SelfStats = await api.selfSample();
    for (let pi = 0; pi < PHASES.length && !stopRef.current; pi++) {
      const ph = PHASES[pi];
      setPhaseIdx(pi);
      await api.benchMode(ph.id === "tray");
      await new Promise((r) => setTimeout(r, 1500)); // let the cadence switch settle
      prev = await api.selfSample();
      const first = prev;
      const phaseSamples: Sample[] = [];
      for (let s = ph.seconds; s > 0 && !stopRef.current; s--) {
        setLeft(s);
        const t0 = Date.now();
        if (ph.id === "active") {
          await Promise.allSettled([api.snapshot(), api.processes(60)]);
        }
        // Sampling at 2 s keeps the test itself out of the measurement as much as possible.
        const dt0 = Date.now() - t0;
        if (dt0 < 2000) await new Promise((r) => setTimeout(r, 2000 - dt0));
        s -= 1;
        const st: SelfStats = await api.selfSample();
        const wall = Math.max(st.wall_ms - prev.wall_ms, 1);
        const cpu = ((st.cpu_ms - prev.cpu_ms) / wall / cores) * 100;
        const sysCpu = useStore.getState().snap?.cpu.usage ?? 0;
        const smp: Sample = { t: st.wall_ms, phase: ph.id, cpu: Math.max(0, cpu), mem: st.mem_mb + st.webview_mem_mb, ps: st.ps_calls - prev.ps_calls, sysCpu };
        all.push(smp);
        phaseSamples.push(smp);
        prev = st;
        setSamples([...all]);
      }
      const last = prev;
      const wall = Math.max(last.wall_ms - first.wall_ms, 1);
      const n = Math.max(phaseSamples.length, 1);
      res.push({
        phase: ph,
        n: phaseSamples.length,
        cpuAvg: Math.max(0, ((last.cpu_ms - first.cpu_ms) / wall / cores) * 100),
        cpuMax: phaseSamples.reduce((a, x) => Math.max(a, x.cpu), 0),
        memAvg: phaseSamples.reduce((a, x) => a + x.mem, 0) / n,
        memMax: phaseSamples.reduce((a, x) => Math.max(a, x.mem), 0),
        psPerMin: ((last.ps_calls - first.ps_calls) / wall) * 60000,
        sysCpuAvg: phaseSamples.reduce((a, x) => a + x.sysCpu, 0) / n,
      });
    }
    await api.benchMode(false);
    setResults(res);
    setRunning(false);
    setPhaseIdx(-1);
    api.log("info", `Тест влияния: ${res.map((r) => `${r.phase.id} ${r.cpuAvg.toFixed(2)}%/${r.memMax.toFixed(0)}МБ`).join(", ")}`);
  };

  const stop = () => {
    stopRef.current = true;
  };

  const grade = (r: PhaseResult[]) => {
    const worst = Math.max(...r.map((x) => x.cpuAvg));
    const mem = Math.max(...r.map((x) => x.memMax));
    if (worst < 1 && mem < 600) return { text: "Незаметно", color: "var(--color-mint)", why: "Меньше 1% процессора в любом режиме (на 24 потоках это меньше четверти одного ядра). На FPS и пинг не влияет." };
    if (worst < 2.5 && mem < 800) return { text: "Легко", color: "var(--color-teal)", why: "Пики при опросе PowerShell, но в среднем ниже 2,5%. Для 12-ядерного 7900X это доли одного ядра." };
    if (worst < 5) return { text: "Ощутимо", color: "var(--color-amber)", why: "Заметно при активной работе с интерфейсом. Сверни в трей во время игры." };
    return { text: "Тяжело", color: "var(--color-coral)", why: "Выше ожидаемого. Пришли мне журнал с Главной, посмотрим, что грузит." };
  };

  const copyReport = () => {
    if (!results) return;
    const lines = [
      `DotPilot: тест влияния на систему (${new Date().toLocaleString("ru-RU")})`,
      `CPU: ${snap?.cpu.name ?? ""} · ${cores} потоков · метод: счётчики процессорного времени процесса и его WebView2 (точные, не выборочные)`,
      ...results.map((r) => `${r.phase.name}: CPU ср ${r.cpuAvg.toFixed(2)}% макс ${r.cpuMax.toFixed(2)}% · RAM ср ${r.memAvg.toFixed(0)} макс ${r.memMax.toFixed(0)} МБ · PowerShell ${r.psPerMin.toFixed(1)}/мин · система в это время ${r.sysCpuAvg.toFixed(0)}% CPU`),
      `Вердикт: ${grade(results).text} — ${grade(results).why}`,
    ];
    navigator.clipboard.writeText(lines.join("\n")).then(() => toast("success", "Отчёт теста скопирован"));
  };

  const s = snap?.self_stats;
  const cur = phaseIdx >= 0 ? PHASES[phaseIdx] : null;

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between gap-4">
        <div>
          <div className="eyebrow">Влияние</div>
          <h1 className="text-[24px] mt-1 flex items-center gap-2">
            <Gauge size={22} className="text-teal" /> Сколько DotPilot отнимает у ПК
          </h1>
          <p className="text-ink-2 text-[12.5px] mt-1 max-w-[820px] leading-snug">Тест измеряет только само приложение: процесс DotPilot плюс его окна WebView2, по счётчикам процессорного времени Windows. Три режима по 20 секунд: простой, активная работа и трей. Цифры — доля от всего процессора ({cores} потока), то есть 1% здесь = четверть одного ядра.</p>
        </div>
        <div className="flex gap-2">
          {running ? (
            <button className="btn btn-danger" onClick={stop}>
              <Square size={14} /> Остановить
            </button>
          ) : (
            <Tip title="Запустить тест (60 с)" text="Во время фазы «трей» интерфейс намеренно не обновляется, это нормально. Не сворачивай окно, пока идёт тест." side="right">
              <button className="btn btn-primary" onClick={run}>
                <Play size={14} /> Запустить тест
              </button>
            </Tip>
          )}
          {results && (
            <button className="btn" onClick={copyReport}>
              <Copy size={14} /> Копировать отчёт
            </button>
          )}
        </div>
      </header>

      <div className="grid grid-cols-5 gap-3">
        <Tile label="Сейчас CPU" value={s ? s.cpu_total_pct.toFixed(2) : "—"} unit="%" color="var(--color-teal)" hint={s ? `ядро ${s.cpu_pct.toFixed(2)}% + WebView ${s.webview_cpu_pct.toFixed(2)}%` : ""} />
        <Tile label="Сейчас память" value={s ? s.mem_mb + s.webview_mem_mb : "—"} unit="МБ" hint={s ? `${s.mem_mb} МБ ядро + ${s.webview_mem_mb} МБ WebView (${s.webview_procs} проц.)` : ""} />
        <Tile label="PowerShell" value={s?.ps_calls ?? "—"} unit="вызовов с запуска" hint="каждый ≈ 0,3 с одного ядра" />
        <Tile label="Система CPU" value={snap ? snap.cpu.usage.toFixed(0) : "—"} unit="%" hint="вся машина, для сравнения" color="var(--color-ink-2)" />
        <Tile label="Цель" value="< 1" unit="% CPU" color="var(--color-mint)" hint="как у лёгкой утилиты в трее" />
      </div>

      {running && cur && (
        <Section title={`Фаза ${phaseIdx + 1} из ${PHASES.length}: ${cur.name}`} sub={cur.what}>
          <div className="flex items-center gap-3">
            <div className="flex-1 rounded-full bg-line h-2 overflow-hidden">
              <div className="h-full bg-teal" style={{ width: `${((cur.seconds - left) / cur.seconds) * 100}%` }} />
            </div>
            <span className="num text-[12px] w-10 text-right">{left} с</span>
          </div>
        </Section>
      )}

      {samples.length > 0 && (
        <Section title="Замеры" sub="Каждый столбик — 2 секунды. Высота: доля CPU всего ПК, шкала до 3%.">
          <div className="flex items-end gap-[3px] h-24">
            {samples.map((x, i) => (
              <div key={i} className="flex-1 rounded-sm" title={`${x.phase} · ${x.cpu.toFixed(2)}% · ${x.mem} МБ`} style={{ height: `${Math.max(2, Math.min(100, (x.cpu / 3) * 100))}%`, background: x.phase === "tray" ? "var(--color-violet)" : x.phase === "active" ? "var(--color-amber)" : "var(--color-teal)" }} />
            ))}
          </div>
          <div className="flex gap-4 mt-2 text-[11.5px] text-ink-3">
            <span><span className="dot bg-teal inline-block mr-1" />простой</span>
            <span><span className="dot bg-amber inline-block mr-1" />активная работа</span>
            <span><span className="dot bg-violet inline-block mr-1" />трей</span>
          </div>
        </Section>
      )}

      {results && (
        <Section title="Результат" right={<span className="tag" style={{ color: grade(results).color, background: `color-mix(in srgb, ${grade(results).color} 14%, transparent)` }}>{grade(results).text}</span>}>
          <div className="text-[13px] mb-3">{grade(results).why}</div>
          <table className="w-full text-[12.5px]">
            <thead>
              <tr className="text-left text-ink-3 text-[10.5px] uppercase tracking-wider">
                <th className="py-1.5 font-semibold">Режим</th>
                <th className="font-semibold text-right">CPU среднее</th>
                <th className="font-semibold text-right">CPU пик (2 с)</th>
                <th className="font-semibold text-right">Память ср / макс</th>
                <th className="font-semibold text-right">PowerShell / мин</th>
                <th className="font-semibold text-right">Система в это время</th>
              </tr>
            </thead>
            <tbody>
              {results.map((r) => (
                <tr key={r.phase.id} className="border-t border-line/60">
                  <td className="py-1.5">{r.phase.name}</td>
                  <td className="num text-right" style={{ color: r.cpuAvg < 1 ? "var(--color-mint)" : r.cpuAvg < 2.5 ? "var(--color-teal)" : "var(--color-amber)" }}>{r.cpuAvg.toFixed(2)}%</td>
                  <td className="num text-right">{r.cpuMax.toFixed(2)}%</td>
                  <td className="num text-right">{r.memAvg.toFixed(0)} / {r.memMax.toFixed(0)} МБ</td>
                  <td className="num text-right">{r.psPerMin.toFixed(1)}</td>
                  <td className="num text-right text-ink-2">{r.sysCpuAvg.toFixed(0)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="text-[11.5px] text-ink-3 mt-3 leading-snug">Как читать: WebView2 (интерфейс) занимает больше памяти, чем ядро на Rust, но в трее не рисует ничего. PowerShell — это опросы адаптеров, служб и QoS; в трее они в 4–5 раз реже. Если хочется ещё легче, увеличь интервал опроса в «Настройках».</div>
        </Section>
      )}
    </div>
  );
}
