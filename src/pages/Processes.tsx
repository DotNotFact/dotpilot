import { useEffect, useMemo, useRef, useState } from "react";
import { useStore } from "../store";
import { api, type ProcRow } from "../lib/api";
import { Section, Tag, Tip, Tile } from "../components/ui";
import { Activity, RefreshCw, Skull, Search, ShieldAlert, Layers } from "lucide-react";

type Sort = "cpu" | "mem" | "gpu" | "name";

export default function Processes() {
  const toast = useStore((s) => s.toast);
  const snap = useStore((s) => s.snap);
  const [rows, setRows] = useState<ProcRow[] | null>(null);
  const [sort, setSort] = useState<Sort>("cpu");
  const [q, setQ] = useState("");
  const [onlyFlagged, setOnlyFlagged] = useState(false);
  const [groupSvc, setGroupSvc] = useState(true);
  const [busy, setBusy] = useState<number | null>(null);
  const timer = useRef<number | null>(null);

  const load = async () => {
    try {
      setRows(await api.processes(120));
    } catch (e) {
      toast("error", String(e));
    }
  };
  useEffect(() => {
    load();
    timer.current = window.setInterval(load, 4000);
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, []);

  const view = useMemo(() => {
    if (!rows) return [];
    let r = rows;
    if (groupSvc) {
      // fold svchost instances into one row per process (they are already per pid); just keep them
      r = r.map((x) => x);
    }
    if (q.trim()) {
      const s = q.trim().toLowerCase();
      r = r.filter((x) => x.name.toLowerCase().includes(s) || x.exe.toLowerCase().includes(s) || x.services.some((v) => v.toLowerCase().includes(s)));
    }
    if (onlyFlagged) r = r.filter((x) => x.flags.some((f) => f !== "svchost" && f !== "critical"));
    const cmp: Record<Sort, (a: ProcRow, b: ProcRow) => number> = {
      cpu: (a, b) => b.cpu - a.cpu,
      mem: (a, b) => b.mem_mb - a.mem_mb,
      gpu: (a, b) => b.gpu_mb - a.gpu_mb,
      name: (a, b) => a.name.localeCompare(b.name),
    };
    return [...r].sort(cmp[sort]).slice(0, 80);
  }, [rows, sort, q, onlyFlagged, groupSvc]);

  const totals = useMemo(() => {
    if (!rows) return { svchost: 0, svchostMb: 0, flagged: 0, top: rows };
    const sv = rows.filter((r) => r.name.toLowerCase() === "svchost.exe");
    return { svchost: sv.length, svchostMb: sv.reduce((s, r) => s + r.mem_mb, 0), flagged: rows.filter((r) => r.flags.some((f) => f === "miner-name" || f === "temp-exe")).length };
  }, [rows]);

  const kill = async (r: ProcRow) => {
    if (r.flags.includes("critical")) return;
    setBusy(r.pid);
    try {
      toast("success", await api.killProcess(r.pid));
      await load();
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between gap-4">
        <div>
          <div className="eyebrow">Процессы</div>
          <h1 className="text-[24px] mt-1 flex items-center gap-2">
            <Activity size={22} className="text-teal" /> Кто ест процессор, память и видеокарту
          </h1>
          <p className="text-ink-2 text-[12.5px] mt-1 max-w-[820px] leading-snug">Список обновляется каждые 4 секунды из уже собранных данных, без дополнительной нагрузки. Подозрительные процессы помечаются: имена майнеров, exe из папки Temp, аномальная загрузка. Системные процессы завершить нельзя.</p>
        </div>
        <div className="flex gap-2">
          <button className="btn" onClick={load}>
            <RefreshCw size={14} /> Обновить
          </button>
        </div>
      </header>

      <div className="grid grid-cols-5 gap-3">
        <Tile label="CPU всего" value={(snap?.cpu.usage ?? 0).toFixed(0)} unit="%" color="var(--color-teal)" />
        <Tile label="Память" value={((snap?.mem.used_mb ?? 0) / 1024).toFixed(1)} unit={`/ ${((snap?.mem.total_mb ?? 0) / 1024).toFixed(0)} ГБ`} color="var(--color-blue)" />
        <Tip title="svchost.exe" text="Хост системных служб: Windows запускает по одному svchost на каждую службу (или группу). Их десятки — это нормально. Завершать нельзя: служба перезапустится или система станет нестабильной. Если хочется меньше — отключай сами службы в services.msc." className="w-full">
          <div className="w-full">
            <Tile label="svchost" value={totals.svchost} unit="процессов" hint={`${totals.svchostMb} МБ суммарно`} />
          </div>
        </Tip>
        <Tip title="DotPilot" text="Собственная нагрузка приложения: процесс Rust + окна WebView2. Цель — до 1% CPU и до 150 МБ памяти. Подробный тест — в «Настройках»." className="w-full">
          <div className="w-full">
            <Tile label="DotPilot сам" value={(snap?.self_stats.cpu_total_pct ?? 0).toFixed(1)} unit="% CPU" hint={`${(snap?.self_stats.mem_mb ?? 0) + (snap?.self_stats.webview_mem_mb ?? 0)} МБ · ${snap?.self_stats.ps_calls ?? 0} вызовов PowerShell`} color="var(--color-mint)" />
          </div>
        </Tip>
        <Tile label="Подозрительных" value={totals.flagged} color={totals.flagged ? "var(--color-coral)" : "var(--color-ink-2)"} hint={totals.flagged ? "смотри метки в списке" : "майнеров и exe из Temp не видно"} />
      </div>

      <Section
        title="Процессы"
        right={
          <div className="flex items-center gap-2">
            <div className="relative">
              <Search size={13} className="absolute left-2 top-2 text-ink-3" />
              <input className="input !pl-7 !w-52" placeholder="имя, путь, служба" value={q} onChange={(e) => setQ(e.target.value)} />
            </div>
            <select className="input !w-auto" value={sort} onChange={(e) => setSort(e.target.value as Sort)}>
              <option value="cpu">по CPU</option>
              <option value="mem">по памяти</option>
              <option value="gpu">по видеопамяти</option>
              <option value="name">по имени</option>
            </select>
            <Tip title="Только подозрительные" text="Показывает процессы с метками: имя майнера, запуск из Temp, высокая нагрузка CPU или памяти.">
              <button className={`btn btn-sm ${onlyFlagged ? "btn-primary" : ""}`} onClick={() => setOnlyFlagged(!onlyFlagged)}>
                <ShieldAlert size={12} /> подозрительные
              </button>
            </Tip>
            <Tip title="Службы внутри svchost" text="Показывать имена служб, которые живут в каждом svchost.exe. Так понятно, что именно это за хост.">
              <button className={`btn btn-sm ${groupSvc ? "btn-primary" : ""}`} onClick={() => setGroupSvc(!groupSvc)}>
                <Layers size={12} /> службы
              </button>
            </Tip>
          </div>
        }
      >
        <table className="w-full text-[12.5px]">
          <thead>
            <tr className="text-left text-ink-3 text-[10.5px] uppercase tracking-wider">
              <th className="py-1.5 font-semibold">Процесс</th>
              <th className="font-semibold text-right">CPU</th>
              <th className="font-semibold text-right">Память</th>
              <th className="font-semibold text-right pr-4">VRAM</th>
              <th className="font-semibold pl-2">Метки</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {view.map((r) => (
              <tr key={r.pid} className="border-t border-line/60 hover:bg-panel-2/40">
                <td className="py-1.5">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{r.name}</span>
                    <span className="num text-[10.5px] text-ink-3">#{r.pid}</span>
                    {r.children > 0 && <span className="num text-[10.5px] text-ink-3">+{r.children} доч.</span>}
                  </div>
                  <div className="text-[11px] text-ink-3 truncate max-w-[520px] num">{groupSvc && r.services.length ? r.services.join(", ") : r.exe}</div>
                </td>
                <td className="num text-right" style={{ color: r.cpu > 25 ? "var(--color-coral)" : r.cpu > 8 ? "var(--color-amber)" : undefined }}>
                  {r.cpu.toFixed(1)}%
                </td>
                <td className="num text-right">{r.mem_mb} МБ</td>
                <td className="num text-right text-ink-2 pr-4">{r.gpu_mb > 0 ? `${r.gpu_mb.toFixed(0)} МБ` : "—"}</td>
                <td className="pl-2">
                  <div className="flex gap-1 flex-wrap">
                    {r.flags.map((f) => (
                      <FlagTag key={f} f={f} />
                    ))}
                  </div>
                </td>
                <td className="text-right">
                  {!r.flags.includes("critical") && (
                    <Tip title={`Завершить ${r.name}`} text="Принудительно закрывает процесс (как «Снять задачу» в диспетчере). Несохранённые данные программы потеряются." side="right">
                      <button className="btn btn-sm btn-danger" disabled={busy === r.pid} onClick={() => kill(r)}>
                        <Skull size={12} />
                      </button>
                    </Tip>
                  )}
                </td>
              </tr>
            ))}
            {rows === null && (
              <tr>
                <td colSpan={6} className="py-4 text-ink-3">
                  Читаю процессы…
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </Section>
    </div>
  );
}

function FlagTag({ f }: { f: string }) {
  const m: Record<string, [string, string, string]> = {
    "miner-name": ["майнер?", "var(--color-coral)", "Имя процесса совпадает с известным майнером. Проверь путь: если это не твоя программа — заверши и удали exe."],
    "temp-exe": ["из Temp", "var(--color-coral)", "Исполняемый файл лежит во временной папке и грузит CPU. Так ведут себя вредоносные программы и «дропперы»."],
    "high-cpu": ["CPU", "var(--color-amber)", "Больше 25% всего процессора прямо сейчас. Для компиляции или игры это нормально, для фонового процесса — нет."],
    "high-mem": ["память", "var(--color-amber)", "Больше 2 ГБ оперативной памяти."],
    svchost: ["svchost", "var(--color-ink-3)", "Системный хост служб. Завершать нельзя."],
    critical: ["система", "var(--color-ink-3)", "Критичный процесс Windows или сам DotPilot."],
  };
  const [label, color, hint] = m[f] ?? [f, "var(--color-ink-3)", ""];
  return (
    <Tip title={label} text={hint}>
      <Tag color={color}>{label}</Tag>
    </Tip>
  );
}
