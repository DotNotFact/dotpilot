import { useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { AreaChart, Area, ResponsiveContainer, YAxis, Tooltip } from "recharts";
import { useStore } from "../store";
import { api, fmtBps, fmtMs, timeHM } from "../lib/api";
import { Section, Tile, StatusPill, Tag, Bar, Tip } from "../components/ui";
import Topology from "../components/Topology";
import PingChart from "../components/PingChart";
import { Download, FileSpreadsheet, Gamepad2, Sparkles, Bot } from "lucide-react";

export default function Dashboard() {
  const snap = useStore((s) => s.snap);
  const cfg = useStore((s) => s.cfg);
  const toast = useStore((s) => s.toast);
  const setPage = useStore((s) => s.setPage);
  const [busy, setBusy] = useState(false);

  const verdict = useMemo(() => {
    if (!snap) return null;
    const gw = snap.ping.find((p) => p.id === "gw") ?? snap.ping[0];
    const inet = snap.ping.find((p) => p.id === "yadns" || p.id === "gdns") ?? snap.ping[1];
    const loss = Math.max(...snap.ping.filter((p) => p.id !== "radmin").map((p) => p.stats.loss_pct), 0);
    const jitter = Math.max(gw?.stats.jitter ?? 0, inet?.stats.jitter ?? 0);
    const rtt = inet?.stats.avg ?? 0;
    let level: "good" | "warn" | "bad" = "good";
    let text = "Сеть в порядке: задержка низкая, потерь нет.";
    if (loss > 3 || rtt > 120 || jitter > 30) {
      level = "bad";
      text = loss > 3 ? `Потери пакетов ${loss.toFixed(0)}% за последнюю минуту. Игра будет лагать.` : rtt > 120 ? `Высокая задержка до интернета (${rtt.toFixed(0)} мс).` : `Сильный джиттер (${jitter.toFixed(0)} мс): нестабильный Wi-Fi или загруженный канал.`;
    } else if (loss > 0.5 || rtt > 70 || jitter > 12 || (gw?.stats.avg ?? 0) > 10) {
      level = "warn";
      text = (gw?.stats.avg ?? 0) > 10 ? `Пинг до роутера ${gw!.stats.avg!.toFixed(0)} мс: Wi-Fi под нагрузкой или слабый сигнал.` : loss > 0.5 ? `Единичные потери (${loss.toFixed(1)}%).` : `Задержка выше обычной (${rtt.toFixed(0)} мс, джиттер ${jitter.toFixed(0)} мс).`;
    }
    return { level, text, loss, jitter, rtt };
  }, [snap]);

  if (!snap || !cfg) return <Loading />;

  const wifi = snap.adapters.find((a) => a.role === "wifi" && a.status === "Up");
  const wifiLocationBlocked = snap.wifi.some(([k, v]) => k === "__error__" && v === "location");
  const wifiKV = (keys: string[]) => snap.wifi.find(([k]) => keys.some((x) => k.toLowerCase().includes(x)))?.[1] ?? "—";
  const signal = wifiKV(["сигнал", "signal"]);
  const ssid = snap.wifi.find(([k]) => /^ssid$/i.test(k.trim()))?.[1] ?? wifiKV(["ssid"]);
  const channel = wifiKV(["канал", "channel"]);
  const radio = wifiKV(["радио", "radio"]);
  const band = wifiKV(["диапазон", "band"]);
  const rx = wifiKV(["прием", "приём", "receive"]);
  const tx = wifiKV(["передач", "transmit"]);
  const signalNum = parseInt(signal) || 0;

  const exportAs = async (format: "json" | "csv") => {
    try {
      setBusy(true);
      const stamp = new Date().toISOString().replace(/[:T]/g, "-").slice(0, 19);
      const path = await save({ defaultPath: `DotPilot-${stamp}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
      if (!path) return;
      await api.exportSnapshot(path, format);
      toast("success", `Сохранено: ${path}`);
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(false);
    }
  };

  const exportAi = async () => {
    try {
      setBusy(true);
      const stamp = new Date().toISOString().replace(/[:T]/g, "-").slice(0, 16);
      const path = await save({ defaultPath: `DotPilot-log-${stamp}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
      const md = await api.aiReport(path ?? undefined);
      await navigator.clipboard.writeText(md);
      toast("success", path ? `Лог для ИИ сохранён и скопирован в буфер: ${path}` : "Лог для ИИ скопирован в буфер обмена");
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(false);
    }
  };

  const gm = snap.game_mode;
  const levelColor = verdict?.level === "good" ? "var(--color-mint)" : verdict?.level === "warn" ? "var(--color-amber)" : "var(--color-coral)";
  const totalRx = snap.adapters.filter((a) => a.status === "Up" && a.role !== "virtual").reduce((s, a) => s + a.rx_bps, 0);
  const totalTx = snap.adapters.filter((a) => a.status === "Up" && a.role !== "virtual").reduce((s, a) => s + a.tx_bps, 0);

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between gap-4">
        <div>
          <div className="eyebrow">Главная · {timeHM(snap.ts)}</div>
          <h1 className="text-[24px] mt-1 flex items-center gap-3">
            <span className="dot dot-live" style={{ background: levelColor, width: 10, height: 10 }} />
            {verdict?.text}
          </h1>
        </div>
        <div className="flex gap-2">
          <Tip title="Лог для ИИ (Markdown)" text="Собирает за всё время работы: статистику пинга по целям, инциденты потерь и всплесков с контекстом (что было запущено, трафик, CPU, Happ/zapret), состояние сети, политики, журнал. В начале файла — инструкция для нейросети. Файл сохраняется и сразу копируется в буфер." rec="Кинь текст в Claude или любой чат и спроси, из-за чего были потери и что поменять." side="right">
            <button className="btn btn-primary" disabled={busy} onClick={exportAi}>
              <Bot size={14} /> Лог для ИИ
            </button>
          </Tip>
          <Tip title="Экспорт полного снимка (JSON)" text="Адаптеры, маршруты, Wi-Fi, службы, политики, вся история пингов за 2 часа, CPU/GPU и журнал — в один файл. Удобно прислать мне в чат или сохранить «до/после» изменений." side="right">
            <button className="btn" disabled={busy} onClick={() => exportAs("json")}>
              <Download size={14} /> JSON
            </button>
          </Tip>
          <Tip title="Экспорт пингов (CSV)" text="Таблица время; цель; хост; задержка (или loss) по секундам за последние 2 часа. Открывается в Excel для графиков." side="right">
            <button className="btn" disabled={busy} onClick={() => exportAs("csv")}>
              <FileSpreadsheet size={14} /> CSV пингов
            </button>
          </Tip>
          <Tip title="Диагностика с Claude" text="Отправляет тот же снимок в Claude API и возвращает разбор: что видно в данных и какие команды выполнить." side="right">
            <button className="btn" onClick={() => setPage("ai")}>
              <Sparkles size={14} /> Спросить Claude
            </button>
          </Tip>
        </div>
      </header>

      <div className="grid grid-cols-6 gap-3">
        {snap.ping.slice(0, 4).map((p, i) => (
          <Tip key={p.id} title={`Пинг до ${p.name} (${p.host})`} text={i === 0 ? "Задержка до твоего роутера — чистый показатель качества Wi-Fi. Норма 1–5 мс. Если растёт до 20+ мс или появляются потери, проблема в радиоканале, а не у провайдера." : "Задержка до узла в интернете за минуту: среднее, джиттер (разброс между соседними замерами) и потери. Джиттер выше 10 мс и любые потери заметны в шутере сильнее, чем высокий ровный пинг."} rec={i === 0 ? "Растёт вместе с загрузкой Claude/Docker — включай игровой режим." : "Сравни с пингом до роутера: если оба растут одновременно — виноват Wi-Fi; если только этот — провайдер или VPN."} className="w-full">
            <div className="w-full">
              <Tile label={p.name} value={p.stats.reachable ? (p.stats.last?.toFixed(0) ?? "…") : "×"} unit="мс" color={p.stats.reachable ? (p.stats.loss_pct > 3 ? "var(--color-coral)" : p.stats.loss_pct > 0 ? "var(--color-amber)" : p.color) : "var(--color-ink-3)"} hint={p.stats.reachable ? `ср ${p.stats.avg?.toFixed(0) ?? "—"} · джит ${p.stats.jitter?.toFixed(0) ?? "—"} · потери ${p.stats.loss_pct.toFixed(0)}%` : `${p.host} не отвечает`} />
            </div>
          </Tip>
        ))}
        <Tip title="Суммарный трафик" text="Скорость приёма и отдачи по всем физическим адаптерам и VPN сейчас. Если игра лагает, а здесь десятки Мбит/с — кто-то качает в фоне: смотри «Сеть» → лимиты." className="w-full">
          <div className="w-full">
            <Tile label="Трафик" value={fmtBps(totalRx).split(" ")[0]} unit={fmtBps(totalRx).split(" ")[1] + " ↓"} hint={`${fmtBps(totalTx)} ↑`} color="var(--color-teal)" />
          </div>
        </Tip>
        <Tile label="Игровой режим" value={gm.active ? "ВКЛ" : "выкл"} color={gm.active ? "var(--color-amber)" : "var(--color-ink-2)"} hint={gm.trigger}>
          <div className="mt-2 flex gap-1">
            <Tip title={gm.active ? "Выключить игровой режим" : "Включить игровой режим"} text="Игровой режим ограничивает скорость фоновых приложений (Claude Code, Docker) и помечает игровой трафик приоритетом DSCP 46. Обычно включается сам при запуске игры." side="right" up>
              <button className="btn btn-sm" onClick={() => api.setGameMode(gm.active ? false : true).then(() => useStore.getState().refresh())}>
                <Gamepad2 size={12} /> {gm.active ? "Выключить" : "Включить"}
              </button>
            </Tip>
            {gm.manual !== null && (
              <button className="btn btn-sm" onClick={() => api.setGameMode(null).then(() => useStore.getState().refresh())}>
                Авто
              </button>
            )}
          </div>
        </Tile>
      </div>

      <div className="grid grid-cols-[1.6fr_1fr] gap-4">
        <Section title="Карта маршрутов" sub="Кто через что ходит прямо сейчас. Анимированные линии = приложение запущено.">
          <Topology snap={snap} cfg={cfg} />
        </Section>
        <Section title="Wi-Fi" sub={wifi ? wifi.description : "Адаптер не активен"}>
          {wifiLocationBlocked && (
            <div className="panel-2 p-2.5 mb-3 text-[11.5px] text-amber leading-snug">
              Windows 11 скрывает SSID и уровень сигнала, пока приложениям запрещено «Расположение».{" "}
              <button className="underline cursor-pointer" onClick={() => api.openUri("ms-settings:privacy-location").catch((e) => toast("error", String(e)))}>
                Открыть настройки расположения
              </button>{" "}
              или{" "}
              <button className="underline cursor-pointer" onClick={() => setPage("access")}>
                выдать доступ автоматически на странице «Доступ»
              </button>
              .
            </div>
          )}
          {wifi ? (
            <div className="flex flex-col gap-3">
              <div>
                <div className="flex items-baseline justify-between">
                  <span className="text-[15px] font-semibold">{ssid}</span>
                  <span className="num text-[13px]" style={{ color: signalNum >= 70 ? "var(--color-mint)" : signalNum >= 45 ? "var(--color-amber)" : "var(--color-coral)" }}>
                    {signal}
                  </span>
                </div>
                <div className="mt-1.5">
                  <Bar value={signalNum} color={signalNum >= 70 ? "var(--color-mint)" : signalNum >= 45 ? "var(--color-amber)" : "var(--color-coral)"} />
                </div>
              </div>
              <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-[12px]">
                <KV k="Стандарт" v={radio} />
                <KV k="Диапазон" v={band} />
                <KV k="Канал" v={channel} />
                <KV k="Линк" v={wifi.link_speed} />
                <KV k="Приём" v={rx.includes("—") ? rx : `${rx} Мбит/с`} />
                <KV k="Передача" v={tx.includes("—") ? tx : `${tx} Мбит/с`} />
                <KV k="IPv4" v={wifi.ipv4.join(", ") || "—"} />
                <KV k="DNS" v={wifi.dns.join(", ") || "—"} />
                <KV k="Метрика" v={String(wifi.metric ?? "—")} />
                <KV k="Шлюз по умолч." v={snap.default_via || "—"} />
              </div>
              <div className="text-[11.5px] text-ink-3 leading-snug">
                {signal === "—" ? "Уровень сигнала недоступен. Линк " + wifi.link_speed + ": если ниже 300 Mbps, сигнал слабый или занят диапазон 2.4 ГГц." : signalNum < 60 ? "Сигнал слабый: пинг и потери будут расти под нагрузкой. Ближе к роутеру или 5/6 ГГц." : band.includes("2.4") ? "Работа в 2.4 ГГц: перегруженный диапазон, лучше 5 ГГц / 6 ГГц." : "Хороший линк. Если потери появляются только при работе Claude/Docker, включай игровой режим."}
              </div>
            </div>
          ) : (
            <div className="text-ink-3 text-[12.5px]">Wi-Fi не подключён.</div>
          )}
        </Section>
      </div>

      <div className="grid grid-cols-[1.6fr_1fr] gap-4">
        <Section
          title="Задержка, последние 3 минуты"
          sub="Пунктир: 50 мс — комфорт, 100 мс — предел для шутера. Разрывы линии = потери."
          right={
            <div className="flex gap-1.5">
              {snap.ping.map((p) => (
                <Tag key={p.id} color={p.color}>
                  {p.name} {p.stats.loss_pct > 0 ? `· −${p.stats.loss_pct.toFixed(0)}%` : ""}
                </Tag>
              ))}
            </div>
          }
        >
          <PingChart series={snap.ping} height={230} />
        </Section>
        <Section title="Система">
          <div className="flex flex-col gap-3">
            <Meter label={snap.cpu.name || "CPU"} value={snap.cpu.usage} suffix={`${snap.cpu.usage.toFixed(0)}% · ${(snap.cpu.freq_mhz / 1000).toFixed(2)} ГГц`} />
            <Meter label="Память" value={snap.mem.percent} suffix={`${(snap.mem.used_mb / 1024).toFixed(1)} / ${(snap.mem.total_mb / 1024).toFixed(0)} ГБ`} color="var(--color-blue)" />
            {snap.gpu.available && (
              <>
                <Meter label={snap.gpu.name} value={snap.gpu.util_pct} suffix={`${snap.gpu.util_pct.toFixed(0)}% · ${snap.gpu.temp_c.toFixed(0)}°C · ${snap.gpu.power_w.toFixed(0)} Вт`} color="var(--color-mint)" />
                <Meter label="Видеопамять" value={(snap.gpu.mem_used_mb / Math.max(snap.gpu.mem_total_mb, 1)) * 100} suffix={`${(snap.gpu.mem_used_mb / 1024).toFixed(1)} / ${(snap.gpu.mem_total_mb / 1024).toFixed(0)} ГБ`} color="var(--color-violet)" />
              </>
            )}
            <Tip title="Нагрузка самого DotPilot" text="Процесс Rust плюс окна WebView2. При свёрнутом в трей окне сбор данных замедляется до 8–60 секунд, а интерфейс не обновляется вовсе." rec="Цель: до 1% CPU и до 150 МБ. Полный тест — в «Настройках»." className="w-full">
              <div className="flex justify-between text-[12px] w-full">
                <span className="text-ink-2">DotPilot сам</span>
                <span className="num">
                  {snap.self_stats.cpu_total_pct.toFixed(1)}% · {snap.self_stats.mem_mb + snap.self_stats.webview_mem_mb} МБ
                </span>
              </div>
            </Tip>
            <div className="border-t border-line pt-3">
              <div className="eyebrow mb-2">Адаптеры</div>
              <div className="flex flex-col gap-1.5">
                {snap.adapters
                  .filter((a) => a.status === "Up")
                  .slice(0, 5)
                  .map((a) => (
                    <div key={a.name} className="flex items-center gap-2 text-[12px]">
                      <StatusPill ok text="" />
                      <span className="flex-1 truncate">{a.name}</span>
                      <span className="num text-ink-2 text-[11px]">{fmtBps(a.rx_bps)} ↓</span>
                      <span className="num text-ink-3 text-[11px]">{fmtBps(a.tx_bps)} ↑</span>
                    </div>
                  ))}
              </div>
            </div>
          </div>
        </Section>
      </div>

      <div className="grid grid-cols-[1fr_1fr] gap-4">
        <Section title="Трафик по адаптерам" sub="Скорость приёма за последние секунды">
          <Throughput snapRates={snap.adapters.filter((a) => a.status === "Up" && a.role !== "virtual").map((a) => ({ name: a.name, rx: a.rx_bps, tx: a.tx_bps }))} />
        </Section>
        <Section
          title="Журнал событий"
          sub="Что DotPilot делал в этой сессии"
          right={
            <button className="btn btn-sm" onClick={() => navigator.clipboard.writeText(snap.events.map((e) => `${timeHM(e.ts)} [${e.level}] ${e.msg}`).join("\n")).then(() => toast("success", "Журнал скопирован"))}>
              Копировать
            </button>
          }
        >
          <div className="flex flex-col gap-1 max-h-[220px] overflow-y-auto pr-1">
            {snap.events.length === 0 && <div className="text-ink-3 text-[12px]">Пока пусто.</div>}
            {snap.events.map((e, i) => (
              <div key={i} className="flex gap-2 text-[12px] leading-snug">
                <span className="num text-ink-3 flex-none">{timeHM(e.ts)}</span>
                <span className={`flex-none dot mt-1.5 ${e.level === "error" ? "bg-coral" : "bg-teal"}`} />
                <span className="text-ink-2 selectable">{e.msg}</span>
              </div>
            ))}
          </div>
        </Section>
      </div>
    </div>
  );
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between gap-2 border-b border-line/60 pb-1">
      <span className="text-ink-3">{k}</span>
      <span className="num text-right truncate">{v}</span>
    </div>
  );
}

function Meter({ label, value, suffix, color = "var(--color-teal)" }: { label: string; value: number; suffix: string; color?: string }) {
  return (
    <div>
      <div className="flex justify-between text-[12px] mb-1">
        <span className="truncate text-ink-2">{label}</span>
        <span className="num text-ink">{suffix}</span>
      </div>
      <Bar value={value} color={color} />
    </div>
  );
}

const rateHistory: Record<string, { t: number; rx: number; tx: number }[]> = {};

function Throughput({ snapRates }: { snapRates: { name: string; rx: number; tx: number }[] }) {
  const now = Date.now();
  for (const r of snapRates) {
    const h = (rateHistory[r.name] ??= []);
    if (!h.length || now - h[h.length - 1].t > 1500) {
      h.push({ t: now, rx: r.rx, tx: r.tx });
      if (h.length > 90) h.shift();
    }
  }
  return (
    <div className="grid grid-cols-2 gap-3">
      {snapRates.slice(0, 4).map((r) => (
        <div key={r.name} className="panel-2 p-2.5">
          <div className="flex justify-between text-[11.5px] mb-1">
            <span className="truncate text-ink-2">{r.name}</span>
            <span className="num">{fmtBps(r.rx)} ↓</span>
          </div>
          <ResponsiveContainer width="100%" height={56}>
            <AreaChart data={[...(rateHistory[r.name] ?? [])]} margin={{ top: 2, right: 0, left: 0, bottom: 0 }}>
              <YAxis hide domain={[0, "auto"]} />
              <Tooltip contentStyle={{ background: "#121721", border: "1px solid #2d3a4f", borderRadius: 8, fontSize: 11 }} formatter={(v) => fmtBps(Number(v))} labelFormatter={(v) => timeHM(Number(v))} isAnimationActive={false} />
              <Area type="monotone" dataKey="rx" stroke="#35d0c6" fill="#35d0c6" fillOpacity={0.15} strokeWidth={1.5} isAnimationActive={false} dot={false} />
              <Area type="monotone" dataKey="tx" stroke="#9085e9" fill="#9085e9" fillOpacity={0.1} strokeWidth={1.2} isAnimationActive={false} dot={false} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      ))}
    </div>
  );
}

function Loading() {
  return (
    <div className="h-[70vh] flex items-center justify-center text-ink-3 text-[13px]">
      <div className="flex items-center gap-3">
        <span className="dot dot-live bg-teal" /> Собираю данные о сети и системе…
      </div>
    </div>
  );
}
