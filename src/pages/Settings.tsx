import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useStore } from "../store";
import { api, type Config, type PingTarget } from "../lib/api";
import { Section, Switch, Label, Tip } from "../components/ui";
import { Plus, Trash2, FolderOpen } from "lucide-react";

export default function Settings() {
  const cfg = useStore((s) => s.cfg);
  const saveConfig = useStore((s) => s.saveConfig);
  const toast = useStore((s) => s.toast);
  const [d, setD] = useState<Config | null>(null);
  const [path, setPath] = useState("");

  useEffect(() => {
    if (cfg) setD(JSON.parse(JSON.stringify(cfg)));
    api.configPath().then(setPath);
  }, [cfg]);

  if (!d) return <div className="text-ink-3">Загрузка…</div>;
  const set = (patch: Partial<Config>) => setD({ ...d, ...patch });
  const save = async () => {
    try {
      await saveConfig(d);
      toast("success", "Настройки сохранены");
    } catch (e) {
      toast("error", String(e));
    }
  };
  const pick = async (key: keyof Config, dir = false) => {
    const f = await open({ directory: dir, multiple: false });
    if (f) set({ [key]: String(f) } as Partial<Config>);
  };
  const setTarget = (i: number, patch: Partial<PingTarget>) => set({ ping_targets: d.ping_targets.map((t, j) => (j === i ? { ...t, ...patch } : t)) });

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between">
        <div>
          <div className="eyebrow">Настройки</div>
          <h1 className="text-[24px] mt-1">Пути, ключи, цели пинга</h1>
          <p className="text-ink-2 text-[12px] mt-1 num">
            Конфиг лежит рядом с exe:{" "}
            <button className="text-teal hover:underline cursor-pointer" onClick={() => revealItemInDir(path).catch(() => {})}>
              {path}
            </button>
          </p>
        </div>
        <button className="btn btn-primary" onClick={save}>
          Сохранить
        </button>
      </header>

      <Section title="Claude API" sub="Ключ хранится только в локальном конфиге. Через прокси Happ (127.0.0.1:10809) запрос уходит в туннель, если api.anthropic.com недоступен напрямую.">
        <div className="grid grid-cols-[2fr_1fr_1fr_1fr] gap-3">
          <Field label="API-ключ (sk-ant-…)">
            <input className="input num" type="password" value={d.anthropic_api_key} onChange={(e) => set({ anthropic_api_key: e.target.value })} placeholder="sk-ant-api03-…" />
          </Field>
          <Field label="Модель">
            <select className="input" value={d.ai_model} onChange={(e) => set({ ai_model: e.target.value })}>
              <option value="claude-opus-5">claude-opus-5</option>
              <option value="claude-sonnet-5">claude-sonnet-5</option>
              <option value="claude-fable-5-1">claude-fable-5-1</option>
            </select>
          </Field>
          <Field label="Усилие">
            <select className="input" value={d.ai_effort} onChange={(e) => set({ ai_effort: e.target.value })}>
              {["low", "medium", "high", "xhigh"].map((x) => (
                <option key={x}>{x}</option>
              ))}
            </select>
          </Field>
          <Field label="HTTP-прокси для API">
            <input className="input num" value={d.ai_proxy} onChange={(e) => set({ ai_proxy: e.target.value })} placeholder="пусто = напрямую" />
          </Field>
        </div>
      </Section>

      <Section title="Цели пинга" sub="Первые четыре показываются на главной. Роутер и DNS-серверы — хорошая база; добавь IP игрового сервера, если знаешь.">
        <div className="flex flex-col gap-2">
          {d.ping_targets.map((t, i) => (
            <div key={t.id} className="grid grid-cols-[1fr_1fr_60px_32px] gap-2 items-center">
              <input className="input" value={t.name} onChange={(e) => setTarget(i, { name: e.target.value })} />
              <input className="input num" value={t.host} onChange={(e) => setTarget(i, { host: e.target.value })} />
              <input className="input !p-0.5 h-8" type="color" value={t.color} onChange={(e) => setTarget(i, { color: e.target.value })} />
              <button className="btn btn-sm btn-danger justify-center" onClick={() => set({ ping_targets: d.ping_targets.filter((_, j) => j !== i) })}>
                <Trash2 size={12} />
              </button>
            </div>
          ))}
          <button className="btn btn-sm w-fit" onClick={() => set({ ping_targets: [...d.ping_targets, { id: "t" + Date.now().toString(36), name: "Новая цель", host: "1.1.1.1", color: "#e87ba4" }] })}>
            <Plus size={12} /> Добавить цель
          </button>
        </div>
      </Section>

      <Section title="Happ VPN">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Happ.exe">
            <PathInput value={d.happ_exe} onChange={(v) => set({ happ_exe: v })} onPick={() => pick("happ_exe")} />
          </Field>
          <Field label="Служба">
            <input className="input" value={d.happ_service} onChange={(e) => set({ happ_service: e.target.value })} />
          </Field>
          <Field label="config.json (sing-box, генерирует Happ)">
            <PathInput value={d.happ_config_path} onChange={(v) => set({ happ_config_path: v })} onPick={() => pick("happ_config_path")} />
          </Field>
          <div className="flex items-center gap-3 pt-5">
            <Tip title="Синхронизация списка Happ" text="При «Применить политики» DotPilot впишет exe с каналом «Через Happ» в правило process_path → proxy и уберёт оттуда exe с каналом «Напрямую». Перед записью делает копию config.json.dotpilot.bak." warn="Happ пересоздаёт config.json при переподключении и может стереть правки. Тогда надёжнее добавить приложения в самом Happ.">
              <Switch on={d.sync_happ_config} onChange={(v) => set({ sync_happ_config: v })} />
            </Tip>
            <div className="text-[12px]">
              <div>Синхронизировать список приложений Happ при «Применить политики»</div>
              <div className="text-ink-3 text-[11px]">Экспериментально: DotPilot правит process_path в config.json (делает .bak). Happ может перезаписать файл при переподключении, тогда добавь приложения в самом Happ.</div>
            </div>
          </div>
        </div>
      </Section>

      <Section title="zapret и Radmin">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Папка zapret">
            <PathInput value={d.zapret_dir} onChange={(v) => set({ zapret_dir: v })} onPick={() => pick("zapret_dir", true)} />
          </Field>
          <Field label="Служба zapret">
            <input className="input" value={d.zapret_service} onChange={(e) => set({ zapret_service: e.target.value })} />
          </Field>
          <Field label="Radmin.exe">
            <PathInput value={d.radmin_exe} onChange={(v) => set({ radmin_exe: v })} onPick={() => pick("radmin_exe")} />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Служба Radmin">
              <input className="input" value={d.radmin_service} onChange={(e) => set({ radmin_service: e.target.value })} />
            </Field>
            <Field label="Имя адаптера Radmin">
              <input className="input" value={d.radmin_alias} onChange={(e) => set({ radmin_alias: e.target.value })} />
            </Field>
          </div>
        </div>
      </Section>

      <Section title="Telegram">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Telegram.exe">
            <PathInput value={d.tg_exe} onChange={(v) => set({ tg_exe: v })} onPick={() => pick("tg_exe")} />
          </Field>
          <Field label="TgWsProxy.exe">
            <PathInput value={d.tgws_exe} onChange={(v) => set({ tgws_exe: v })} onPick={() => pick("tgws_exe")} />
          </Field>
        </div>
      </Section>

      <Section title="Опрос и нагрузка DotPilot" sub="Приложение должно оставаться незаметным: цель до 1% CPU и до 150 МБ памяти.">
        <div className="grid grid-cols-[220px_1fr] gap-4 items-start">
          <Field label="Интервал обновления, мс">
            <input className="input num" type="number" min={1000} step={500} value={d.poll_ms} onChange={(e) => set({ poll_ms: Number(e.target.value) })} />
          </Field>
          <SelfTest />
        </div>
      </Section>

      <Section title="Сброс" sub="Вернуть DotPilot к заводскому состоянию">
        <div className="flex gap-2 flex-wrap">
          <Tip title="Сбросить все настройки" text="Возвращает список приложений, цели пинга, профили и пути к значениям по умолчанию, удаляет QoS-правила DotPilot. API-ключ сохраняется. Звук и твики Windows сбрасываются отдельными кнопками на своих страницах.">
            <button className="btn btn-danger" onClick={() => api.resetConfig().then(() => useStore.getState().loadConfig()).then(() => toast("success", "Настройки сброшены")).catch((e) => toast("error", String(e)))}>
              Сбросить все настройки
            </button>
          </Tip>
        </div>
      </Section>
    </div>
  );
}

const fieldHints: Record<string, { text: string; rec?: string }> = {
  "API-ключ (sk-ant-…)": { text: "Ключ Anthropic API с console.anthropic.com. Хранится в DotPilot.config.json рядом с exe в открытом виде, не копируй конфиг посторонним.", rec: "Заведи отдельный ключ только для DotPilot, чтобы его можно было отозвать." },
  Модель: { text: "claude-opus-5 — лучший разбор сетевых проблем. claude-sonnet-5 — быстрее и в 2,5 раза дешевле. claude-fable-5-1 — максимум интеллекта, дорого и медленно.", rec: "opus-5 для диагностики, sonnet-5 для быстрых вопросов." },
  Усилие: { text: "Сколько модель «думает» перед ответом: low — короткий ответ за секунды, xhigh — глубокий разбор за минуту-две и дороже.", rec: "medium для обычной диагностики." },
  "HTTP-прокси для API": { text: "api.anthropic.com не отвечает из РФ напрямую. 127.0.0.1:10809 — локальный HTTP-прокси Happ (xray), через него запрос уйдёт в туннель. Пусто — напрямую." },
  "Happ.exe": { text: "Путь к окну Happ. Кнопка «Открыть» на странице «Сеть» запускает именно его." },
  Служба: { text: "Имя службы Windows демона Happ (happd). Проверить: Get-Service HappService." },
  "config.json (sing-box, генерирует Happ)": { text: "Файл, который Happ передаёт ядру sing-box. Из него DotPilot читает, какие exe идут в туннель (process_path) и включён ли TUN." },
  "Папка zapret": { text: "Папка с general.bat и bin\\winws.exe. Используется, если служба zapret не установлена." },
  "Служба zapret": { text: "Имя службы, созданной service.bat из комплекта zapret." },
  "Radmin.exe": { text: "Окно Radmin VPN для управления сетями друзей." },
  "Служба Radmin": { text: "Служба RvControlSvc: пока она работает, адаптер Radmin VPN активен." },
  "Имя адаптера Radmin": { text: "Как адаптер называется в Windows (ncpa.cpl). DotPilot по нему держит метрику 1 для сети 26.x." },
  "Интервал обновления, мс": { text: "Как часто собираются CPU/RAM/трафик/процессы. 2000 мс — баланс; ниже 1000 нельзя. Адаптеры и маршруты обновляются реже (12 с)." },
};

function SelfTest() {
  const snap = useStore((s) => s.snap);
  const [samples, setSamples] = useState<{ t: number; cpu: number; mem: number }[]>([]);
  const [running, setRunning] = useState(false);
  const [left, setLeft] = useState(0);
  useEffect(() => {
    if (!running || !snap) return;
    setSamples((s) => [...s, { t: snap.ts, cpu: snap.self_stats.cpu_total_pct, mem: snap.self_stats.mem_mb + snap.self_stats.webview_mem_mb }].slice(-40));
  }, [snap?.ts, running]);
  useEffect(() => {
    if (!running) return;
    const iv = setInterval(() => setLeft((l) => (l <= 1 ? (setRunning(false), 0) : l - 1)), 1000);
    return () => clearInterval(iv);
  }, [running]);
  const start = () => {
    setSamples([]);
    setLeft(60);
    setRunning(true);
  };
  const avg = samples.length ? samples.reduce((s, x) => s + x.cpu, 0) / samples.length : 0;
  const max = samples.reduce((m, x) => Math.max(m, x.cpu), 0);
  const memMax = samples.reduce((m, x) => Math.max(m, x.mem), 0);
  const verdict = !samples.length ? null : max < 1.5 && memMax < 200 ? "Отлично: приложение почти не заметно." : max < 4 ? "Нормально: короткие всплески при опросе PowerShell." : "Выше ожидаемого: пришли мне журнал, разберёмся.";
  const s = snap?.self_stats;
  return (
    <div className="panel-2 p-3">
      <div className="flex items-center justify-between gap-3">
        <div className="text-[12.5px]">
          Сейчас: <span className="num">{s?.cpu_total_pct.toFixed(2) ?? "—"}% CPU</span> · <span className="num">{(s?.mem_mb ?? 0) + (s?.webview_mem_mb ?? 0)} МБ</span> ({s?.mem_mb ?? 0} МБ ядро + {s?.webview_mem_mb ?? 0} МБ WebView, {s?.webview_procs ?? 0} проц.) · вызовов PowerShell {s?.ps_calls ?? 0}
        </div>
        <Tip title="Тест на 60 секунд" text="Записывает нагрузку самого DotPilot каждые несколько секунд, пока ты работаешь в приложении, и выносит вердикт по максимуму и среднему." side="right">
          <button className="btn btn-sm" disabled={running} onClick={start}>
            {running ? `Идёт тест… ${left} с` : "Тест 60 с"}
          </button>
        </Tip>
      </div>
      {samples.length > 0 && (
        <div className="mt-2">
          <div className="flex items-end gap-[3px] h-12">
            {samples.map((x, i) => (
              <div key={i} className="flex-1 rounded-sm" style={{ height: `${Math.min(100, (x.cpu / 5) * 100)}%`, background: x.cpu > 3 ? "var(--color-amber)" : "var(--color-teal)" }} title={`${x.cpu.toFixed(2)}% · ${x.mem} МБ`} />
            ))}
          </div>
          <div className="text-[11.5px] text-ink-2 mt-1 num">
            среднее {avg.toFixed(2)}% · максимум {max.toFixed(2)}% · память до {memMax} МБ — <span className="text-ink">{verdict}</span>
          </div>
        </div>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  const h = fieldHints[label];
  return (
    <label className="flex flex-col gap-1">
      {h ? (
        <Label title={label} text={h.text} rec={h.rec}>
          {label}
        </Label>
      ) : (
        <span className="eyebrow">{label}</span>
      )}
      {children}
    </label>
  );
}

function PathInput({ value, onChange, onPick }: { value: string; onChange: (v: string) => void; onPick: () => void }) {
  return (
    <div className="flex gap-1">
      <input className="input num" value={value} onChange={(e) => onChange(e.target.value)} />
      <button type="button" className="btn btn-sm" onClick={onPick}>
        <FolderOpen size={12} />
      </button>
    </div>
  );
}
