import { useEffect, useRef, useState } from "react";
import { AreaChart, Area, ResponsiveContainer, YAxis, XAxis, Tooltip, CartesianGrid } from "recharts";
import { useStore } from "../store";
import { Section, Tile, Bar, Tag, StatusPill } from "../components/ui";
import {
  timeHM,
  api,
  type FirmwareInfo,
  type MemoryConfig,
  type Baseline,
  type Comparison,
  type BiosSuggestion,
} from "../lib/api";

const history: { t: number; cpu: number; mem: number }[] = [];

export default function Cpu() {
  const snap = useStore((s) => s.snap);
  const lastT = useRef(0);
  const [fw, setFw] = useState<FirmwareInfo | null>(null);
  const [mem, setMem] = useState<MemoryConfig | null>(null);
  const [baselines, setBaselines] = useState<Baseline[]>([]);
  const [cmp, setCmp] = useState<Comparison | null>(null);
  const [bios, setBios] = useState<BiosSuggestion | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [plErr, setPlErr] = useState<string | null>(null);

  const loadPlatform = () => {
    api
      .platformState()
      .then(([f, m]) => {
        setFw(f);
        setMem(m);
      })
      .catch((e) => setPlErr(String(e)));
    api
      .platformBaselines()
      .then(([store, c]) => {
        setBaselines(store.items);
        setCmp(c);
      })
      .catch(() => {});
  };

  useEffect(loadPlatform, []);

  const measure = async (label: string) => {
    setBusy("Идёт замер: нагрузка на процессор и проверка памяти…");
    setPlErr(null);
    try {
      const store = await api.platformMeasure(label);
      setBaselines(store.items);
      const [, c] = await api.platformBaselines();
      setCmp(c);
      loadPlatform();
    } catch (e) {
      setPlErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const askBios = async () => {
    setBusy("Claude разбирает замеры…");
    setPlErr(null);
    try {
      setBios(await api.biosAdvice(""));
    } catch (e) {
      setPlErr(String(e));
    } finally {
      setBusy(null);
    }
  };
  useEffect(() => {
    if (!snap) return;
    if (snap.ts !== lastT.current) {
      lastT.current = snap.ts;
      history.push({ t: snap.ts, cpu: snap.cpu.usage, mem: snap.mem.percent });
      if (history.length > 180) history.shift();
    }
  }, [snap]);
  if (!snap) return <div className="text-ink-3">Загрузка…</div>;
  const c = snap.cpu;
  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Процессор</div>
        <h1 className="text-[24px] mt-1">{c.name || "CPU"}</h1>
        <p className="text-ink-2 text-[12.5px] mt-1">Мониторинг в реальном времени. PBO, Curve Optimizer и EXPO переключаются только в BIOS — ниже приложение меряет их эффект до и после и подсказывает конкретные значения. Схема питания переключается на странице «Профили».</p>
      </header>
      <div className="grid grid-cols-4 gap-3">
        <Tile label="Загрузка" value={c.usage.toFixed(0)} unit="%" color="var(--color-teal)" />
        <Tile label="Частота" value={(c.freq_mhz / 1000).toFixed(2)} unit="ГГц" />
        <Tile label="Потоки" value={c.cores} unit="лог. ядер" />
        <Tile label="Память" value={(snap.mem.used_mb / 1024).toFixed(1)} unit={`/ ${(snap.mem.total_mb / 1024).toFixed(0)} ГБ`} color="var(--color-blue)" hint={`${snap.mem.percent.toFixed(0)}% занято`} />
      </div>
      <Section title="Загрузка по потокам">
        <div className="grid grid-cols-6 gap-x-4 gap-y-2">
          {c.per_core.map((v, i) => (
            <div key={i} className="flex items-center gap-2 text-[11.5px]">
              <span className="num text-ink-3 w-8">#{i}</span>
              <div className="flex-1">
                <Bar value={v} color={v > 85 ? "var(--color-coral)" : v > 50 ? "var(--color-amber)" : "var(--color-teal)"} height={8} />
              </div>
              <span className="num w-9 text-right">{v.toFixed(0)}%</span>
            </div>
          ))}
        </div>
      </Section>
      <Section title="Тюнинг без BIOS: план реализации" sub="Что будет на этой странице дальше и почему именно так">
        <div className="grid grid-cols-3 gap-3 text-[12.5px] text-ink-2 leading-relaxed">
          <div className="panel-2 p-3">
            <div className="text-ink font-semibold mb-1">Процессор 7900X</div>
            <ul className="list-disc ml-4 flex flex-col gap-1">
              <li>Драйвер AMD Ryzen Master + утилита PBO2 Tuner: Curve Optimizer по ядрам и лимиты PPT/TDC/EDC прямо из Windows, <b className="text-ink">без BIOS</b>. Всё сбрасывается при перезагрузке — встроенный откат.</li>
              <li>Автоподбор: DotPilot ставит −10 на все ядра, гоняет CoreCycler по одному ядру, при ошибке возвращает +5 для этого ядра и запоминает. Итог — стабильный профиль, который применяется при входе.</li>
              <li>Лимит температуры 85°C вместо 95°C (PPT 142 → 120 Вт): почти те же FPS, минус троттлинг и шум.</li>
            </ul>
          </div>
          <div className="panel-2 p-3">
            <div className="text-ink font-semibold mb-1">Видеокарта RTX 5070 Ti</div>
            <ul className="list-disc ml-4 flex flex-col gap-1">
              <li>Лимит мощности через nvidia-smi -pl (уже доступно, требует администратора).</li>
              <li>Андервольт кривой напряжения: профили MSI Afterburner (уже установлен), запуск по профилю DotPilot: «Игра» = undervolt-профиль, «Работа» = сток.</li>
              <li>Кривая вентилятора и цель температуры через Afterburner / FanControl.</li>
            </ul>
          </div>
          <div className="panel-2 p-3">
            <div className="text-ink font-semibold mb-1">Вентиляторы, память, SSD</div>
            <ul className="list-disc ml-4 flex flex-col gap-1">
              <li>LibreHardwareMonitor как источник датчиков (Tctl, VRM, NVMe, обороты) через его HTTP-json; FanControl для кривых по профилям.</li>
              <li>SSD: SMART, температура и износ; предупреждение при 70°C.</li>
              <li>BIOS напрямую не трогаем: нет безопасного способа откатить, если ПК не загрузится. Всё делаем из Windows, где откат = перезагрузка.</li>
            </ul>
          </div>
        </div>
      </Section>
      <Section
        title="Настройка через BIOS"
        sub="Реальные рычаги Zen 4 — EXPO, PBO и Curve Optimizer — живут в BIOS и из Windows не переключаются. Приложение делает то, что умеет честно: меряет до и после и подтверждает результат числами."
        right={fw ? <Tag>{`BIOS ${fw.bios_version} от ${fw.bios_date}`}</Tag> : undefined}
      >
        {mem && (
          <div className="panel-2 px-3.5 py-3">
            <div className="flex items-start gap-2.5">
              <StatusPill ok={mem.profile_enabled} warn={!mem.profile_enabled} text={mem.profile_enabled ? "включён" : "выключен"} />
              <div className="min-w-0">
                <div className="text-[13px]">Профиль памяти EXPO</div>
                <div className="text-[12px] text-ink-2 mt-0.5">{mem.verdict}</div>
                <div className="text-[11.5px] text-ink-3 mt-1">
                  {mem.modules.map((m) => `${m.bank}: ${m.capacity_gb.toFixed(0)} ГБ, ${m.configured_mts} МТ/с, ${(m.configured_mv / 1000).toFixed(3)} В`).join(" · ")}
                </div>
              </div>
            </div>
          </div>
        )}

        <div className="flex items-center gap-2 mt-3">
          <button className="btn" disabled={!!busy} onClick={() => measure(mem?.profile_enabled ? "после правки" : "до правки")}>
            Снять замер
          </button>
          <button className="btn" disabled={!!busy || baselines.length === 0} onClick={askBios}>
            Спросить Claude, что менять
          </button>
          {busy && <span className="text-[12px] text-ink-2">{busy}</span>}
        </div>
        {plErr && <div className="text-[12.5px] text-coral mt-2">{plErr}</div>}

        {baselines.length > 0 && (
          <div className="mt-3">
            <div className="eyebrow mb-1.5">Замеры</div>
            <div className="flex flex-col gap-1">
              {baselines.slice(-5).reverse().map((b, i) => (
                <div key={i} className="text-[12px] text-ink-2">
                  <span className="num text-ink-3">{timeHM(b.at * 1000)}</span> · {b.label} — процессор{" "}
                  <span className="num">{b.cpu_passes_per_sec.toFixed(0)}</span> проходов/с, память{" "}
                  <span className="num">{b.memory_mb_per_sec.toFixed(0)}</span> МБ/с
                  {b.loaded_clock_mhz != null && <>, частота под нагрузкой <span className="num">{b.loaded_clock_mhz.toFixed(0)}</span> МГц</>}
                  {(b.cpu_mismatches > 0 || b.memory_mismatches > 0) && <span className="text-coral"> · есть ошибки</span>}
                </div>
              ))}
            </div>
          </div>
        )}

        {cmp && (
          <div className="panel-2 px-3.5 py-3 mt-3">
            <div className="flex items-center justify-between gap-3">
              <div className="text-[13px]">Разница между двумя последними замерами</div>
              {cmp.stable ? <Tag color="var(--color-mint)">ошибок нет</Tag> : <Tag color="var(--color-coral)">найдены ошибки</Tag>}
            </div>
            <div className="grid grid-cols-3 gap-3 mt-2">
              <Tile
                label="Процессор"
                value={`${cmp.cpu_delta_percent > 0 ? "+" : ""}${cmp.cpu_delta_percent.toFixed(1)}`}
                unit="%"
                color={cmp.cpu_delta_percent >= 0 ? "var(--color-mint)" : "var(--color-coral)"}
              />
              <Tile
                label="Память"
                value={`${cmp.memory_delta_percent > 0 ? "+" : ""}${cmp.memory_delta_percent.toFixed(1)}`}
                unit="%"
                color={cmp.memory_delta_percent >= 0 ? "var(--color-mint)" : "var(--color-coral)"}
              />
              <Tile
                label="Частота под нагрузкой"
                value={cmp.clock_delta_mhz != null ? `${cmp.clock_delta_mhz > 0 ? "+" : ""}${cmp.clock_delta_mhz.toFixed(0)}` : "—"}
                unit="МГц"
              />
            </div>
            <div className="text-[12px] text-ink-2 mt-2">{cmp.summary}</div>
          </div>
        )}

        {bios && (
          <div className="mt-3">
            <div className="eyebrow mb-1.5">Что менять — от Claude</div>
            <div className="flex flex-col gap-2">
              {bios.advice.settings.map((s, i) => (
                <div key={i} className="panel-2 px-3.5 py-3">
                  <div className="flex items-baseline justify-between gap-3">
                    <div className="text-[13px]">{s.path}</div>
                    <Tag color="var(--color-teal)">{s.value}</Tag>
                  </div>
                  <div className="text-[12px] text-ink-2 mt-1">{s.why}</div>
                  <div className="text-[11.5px] text-ink-3 mt-1">Риск и откат: {s.risk}</div>
                </div>
              ))}
            </div>
            <div className="text-[12px] text-ink-2 mt-2">
              <div><span className="text-ink-3">Порядок: </span>{bios.advice.order}</div>
              <div className="mt-1"><span className="text-ink-3">Проверить после: </span>{bios.advice.verify}</div>
              <div className="mt-1"><span className="text-ink-3">Чего ждать: </span>{bios.advice.expected_gain}</div>
            </div>
            {bios.advice.warnings.length > 0 && (
              <ul className="text-[12px] text-amber mt-2 list-disc pl-5">
                {bios.advice.warnings.map((w, i) => <li key={i}>{w}</li>)}
              </ul>
            )}
            <div className="text-[11.5px] text-ink-3 mt-2">
              {bios.model} · {bios.input_tokens} вход / {bios.output_tokens} выход
            </div>
          </div>
        )}
      </Section>

      <Section title="История, последние 6 минут">
        <ResponsiveContainer width="100%" height={220}>
          <AreaChart data={[...history]} margin={{ top: 8, right: 12, left: -10, bottom: 0 }}>
            <CartesianGrid stroke="#1b2230" vertical={false} />
            <XAxis dataKey="t" tickFormatter={(v) => timeHM(Number(v))} stroke="#2d3a4f" tick={{ fill: "#5f6b80", fontSize: 10 }} minTickGap={60} />
            <YAxis domain={[0, 100]} stroke="#2d3a4f" tick={{ fill: "#5f6b80", fontSize: 10 }} width={40} />
            <Tooltip contentStyle={{ background: "#121721", border: "1px solid #2d3a4f", borderRadius: 8, fontSize: 12 }} labelFormatter={(v) => timeHM(Number(v))} formatter={(v, n) => [`${Number(v).toFixed(0)}%`, n === "cpu" ? "CPU" : "RAM"]} isAnimationActive={false} />
            <Area type="monotone" dataKey="cpu" stroke="#35d0c6" fill="#35d0c6" fillOpacity={0.15} strokeWidth={1.8} isAnimationActive={false} dot={false} />
            <Area type="monotone" dataKey="mem" stroke="#3987e5" fill="#3987e5" fillOpacity={0.08} strokeWidth={1.4} isAnimationActive={false} dot={false} />
          </AreaChart>
        </ResponsiveContainer>
      </Section>
    </div>
  );
}
