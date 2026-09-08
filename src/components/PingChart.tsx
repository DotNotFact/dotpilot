import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid, ReferenceLine, Legend } from "recharts";
import type { TargetSeries } from "../lib/api";
import { timeHM } from "../lib/api";

export default function PingChart({ series, height = 220 }: { series: TargetSeries[]; height?: number }) {
  if (!series.length) return <div className="text-ink-3 text-[12px]">Нет целей для пинга.</div>;
  // merge by timestamp bucket (1 s)
  const rows = new Map<number, Record<string, number | null>>();
  for (const s of series) {
    for (const smp of s.samples) {
      const t = Math.round(smp.t / 1000) * 1000;
      const row = rows.get(t) ?? { t };
      row[s.id] = smp.rtt;
      rows.set(t, row);
    }
  }
  const data = [...rows.values()].sort((a, b) => (a.t as number) - (b.t as number));
  const maxAvg = Math.max(...series.map((s) => s.stats.max ?? 0), 20);
  const yMax = Math.min(Math.ceil(maxAvg / 25) * 25, 500);

  return (
    <ResponsiveContainer width="100%" height={height}>
      <LineChart data={data} margin={{ top: 8, right: 12, left: -10, bottom: 0 }}>
        <CartesianGrid stroke="#1b2230" vertical={false} />
        <XAxis dataKey="t" tickFormatter={(v) => timeHM(v as number)} stroke="#2d3a4f" tick={{ fill: "#5f6b80", fontSize: 10, fontFamily: "Cascadia Mono, Consolas" }} minTickGap={60} />
        <YAxis domain={[0, yMax]} stroke="#2d3a4f" tick={{ fill: "#5f6b80", fontSize: 10, fontFamily: "Cascadia Mono, Consolas" }} width={44} tickFormatter={(v) => `${v}`} />
        <ReferenceLine y={50} stroke="#f2b33d" strokeOpacity={0.35} strokeDasharray="4 4" />
        <ReferenceLine y={100} stroke="#f0605d" strokeOpacity={0.35} strokeDasharray="4 4" />
        <Tooltip
          contentStyle={{ background: "#121721", border: "1px solid #2d3a4f", borderRadius: 8, fontSize: 12, fontFamily: "Cascadia Mono, Consolas" }}
          labelFormatter={(v) => timeHM(v as number)}
          formatter={(v, name) => [v === null || v === undefined ? "потеря" : `${Number(v).toFixed(0)} мс`, series.find((s) => s.id === name)?.name ?? String(name)]}
          isAnimationActive={false}
        />
        <Legend wrapperStyle={{ fontSize: 11.5, color: "#9aa6ba" }} formatter={(v) => series.find((s) => s.id === v)?.name ?? v} />
        {series.map((s) => (
          <Line key={s.id} type="monotone" dataKey={s.id} stroke={s.color} strokeWidth={1.8} dot={false} isAnimationActive={false} connectNulls={false} />
        ))}
      </LineChart>
    </ResponsiveContainer>
  );
}
