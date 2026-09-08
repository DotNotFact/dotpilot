import { useState } from "react";
import { useStore } from "../store";
import { api } from "../lib/api";
import { Section, Markdown } from "../components/ui";
import { Sparkles, Loader2 } from "lucide-react";

const presets = [
  "Проанализируй состояние сети и скажи, что исправить, чтобы в играх не было пинга и потерь, когда параллельно работают Claude Code и Docker.",
  "Правильно ли расставлены метрики интерфейсов и маршруты для Wi-Fi, Happ и Radmin? Что мешает друг другу?",
  "Проверь политики QoS и игровой режим: достаточно ли лимитов для фоновых приложений и верно ли выбран DSCP?",
  "Что в Wi-Fi-линке (сигнал, канал, диапазон) стоит поменять для стабильного пинга в Warface?",
];

export default function Ai() {
  const cfg = useStore((s) => s.cfg);
  const setPage = useStore((s) => s.setPage);
  const [q, setQ] = useState(presets[0]);
  const [answer, setAnswer] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const ask = async () => {
    setBusy(true);
    setError(null);
    setAnswer(null);
    try {
      setAnswer(await api.askClaude(q));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Диагностика</div>
        <h1 className="text-[24px] mt-1 flex items-center gap-2">
          <Sparkles size={22} className="text-teal" /> Спросить Claude о твоей сети
        </h1>
        <p className="text-ink-2 text-[12.5px] mt-1 max-w-[760px] leading-snug">В запрос уходит полный снимок: адаптеры, маршруты, Wi-Fi, службы, политики, последние 60 секунд пингов, CPU/GPU и журнал. API-ключ и ответ никуда, кроме api.anthropic.com, не отправляются.</p>
      </header>

      {!cfg?.anthropic_api_key && (
        <div className="panel p-3 text-[12.5px] border-amber/50 text-amber flex items-center justify-between">
          <span>Не задан API-ключ Anthropic.</span>
          <button className="btn btn-sm" onClick={() => setPage("settings")}>
            Открыть настройки
          </button>
        </div>
      )}

      <Section title="Вопрос">
        <div className="flex flex-wrap gap-1.5 mb-2">
          {presets.map((p, i) => (
            <button key={i} className={`btn btn-sm ${q === p ? "btn-primary" : ""}`} onClick={() => setQ(p)}>
              {["Пинг и потери при работе", "Метрики и маршруты", "QoS и игровой режим", "Wi-Fi линк"][i]}
            </button>
          ))}
        </div>
        <textarea className="input min-h-[80px] resize-y selectable" value={q} onChange={(e) => setQ(e.target.value)} />
        <div className="flex items-center gap-3 mt-2">
          <button className="btn btn-primary" disabled={busy || !cfg?.anthropic_api_key} onClick={ask}>
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />} {busy ? "Claude думает…" : "Отправить снимок и спросить"}
          </button>
          <span className="text-[11.5px] text-ink-3">
            модель {cfg?.ai_model} · усилие {cfg?.ai_effort} · прокси {cfg?.ai_proxy || "нет"}
          </span>
        </div>
      </Section>

      {error && <div className="panel p-3 text-[12.5px] border-coral/50 text-coral selectable">{error}</div>}
      {answer && (
        <Section title="Ответ">
          <Markdown text={answer} />
        </Section>
      )}
    </div>
  );
}
