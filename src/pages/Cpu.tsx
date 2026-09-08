import { useEffect, useRef } from "react";
import { AreaChart, Area, ResponsiveContainer, YAxis, XAxis, Tooltip, CartesianGrid } from "recharts";
import { useStore } from "../store";
import { Section, Tile, Bar } from "../components/ui";
import { timeHM } from "../lib/api";

const history: { t: number; cpu: number; mem: number }[] = [];

export default function Cpu() {
  const snap = useStore((s) => s.snap);
  const lastT = useRef(0);
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
        <p className="text-ink-2 text-[12.5px] mt-1">Мониторинг в реальном времени. Управление лимитами (PBO, Curve Optimizer, схемы питания по ядрам) — в разработке; пока схема питания переключается на странице «Профили».</p>
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
