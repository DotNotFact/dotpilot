import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Section, Tile, Tag, Markdown, StatusPill, Switch } from "../components/ui";
import { useStore } from "../store";
import {
  api,
  timeHM,
  type HealthReport,
  type Finding,
  type Severity,
  type TrendReport,
  type MaintenanceTask,
  type UpdateReport,
} from "../lib/api";

const SEVERITY: Record<Severity, { label: string; color: string; order: number }> = {
  problem: { label: "требует внимания", color: "var(--color-coral)", order: 0 },
  warning: { label: "можно улучшить", color: "var(--color-amber)", order: 1 },
  notice: { label: "к сведению", color: "var(--color-blue)", order: 2 },
  ok: { label: "в норме", color: "var(--color-mint)", order: 3 },
};

function Card({ f }: { f: Finding }) {
  const s = SEVERITY[f.severity];
  return (
    <div className="panel-2 px-3.5 py-3 border-l-2" style={{ borderLeftColor: s.color }}>
      <div className="flex items-baseline justify-between gap-3">
        <div className="text-[13px]">
          <span className="text-ink-3">{f.area} · </span>
          {f.title}
        </div>
        <Tag color={s.color}>{s.label}</Tag>
      </div>
      <div className="text-[12.5px] mt-1.5">{f.measured}</div>
      <div className="text-[12px] text-ink-2 mt-1">
        <span className="text-ink-3">Норма: </span>
        {f.reference}
      </div>
      {f.advice && (
        <div className="text-[12px] text-ink-2 mt-1">
          <span className="text-ink-3">Что делать: </span>
          {f.advice}
        </div>
      )}
    </div>
  );
}

export default function Health() {
  const [report, setReport] = useState<HealthReport | null>(null);
  const [advice, setAdvice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [trends, setTrends] = useState<TrendReport | null>(null);
  const [tasks, setTasks] = useState<MaintenanceTask[]>([]);
  const [updates, setUpdates] = useState<UpdateReport | null>(null);

  const saveHistory = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const path = await save({ defaultPath: `История-ПК-${stamp}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
    if (!path) return;
    setBusy("Собираю историю…");
    try {
      await api.serviceHistory(path);
      setMsgOk(`История сохранена: ${path}. Файл можно вставить в заявку или показать мастеру.`);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };
  const cfg = useStore((s) => s.cfg);
  const saveConfig = useStore((s) => s.saveConfig);

  const saveReport = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const path = await save({ defaultPath: `Состояние-ПК-${stamp}.html`, filters: [{ name: "HTML", extensions: ["html"] }] });
    if (!path) return;
    setBusy("Собираю отчёт…");
    try {
      await api.hardwareReport(path);
      setMsgOk(`Отчёт сохранён: ${path}. Откройте его в браузере и напечатайте в PDF, если нужен файл для передачи.`);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const check = async () => {
    setBusy("Идёт проверка: датчики, диски, журнал аппаратных ошибок…");
    setErr(null);
    setAdvice(null);
    try {
      setReport(await api.healthCheck());
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const ask = async () => {
    if (!report) return;
    setBusy("Claude разбирает отчёт…");
    setErr(null);
    try {
      setAdvice(await api.healthAdvice(report));
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const [msgOk, setMsgOk] = useState<string | null>(null);

  useEffect(() => {
    check();
    api.trendsReport().then(setTrends).catch(() => setTrends(null));
    api.maintenanceState().then(setTasks).catch(() => setTasks([]));
    api.updatesReport().then(setUpdates).catch(() => setUpdates(null));
  }, []);

  const sorted = report ? [...report.findings].sort((a, b) => SEVERITY[a.severity].order - SEVERITY[b.severity].order) : [];
  const counts = {
    problem: sorted.filter((f) => f.severity === "problem").length,
    warning: sorted.filter((f) => f.severity === "warning").length,
    notice: sorted.filter((f) => f.severity === "notice").length,
    ok: sorted.filter((f) => f.severity === "ok").length,
  };

  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Здоровье ПК</div>
        <h1 className="text-[24px] mt-1">{report ? report.summary : "Проверка…"}</h1>
        <p className="text-ink-2 text-[12.5px] mt-1">
          Каждый вывод показывает, что измерено и с какой нормой сравнено. Нормы привязаны к конкретному поколению
          железа: 95 °C для Ryzen 7000 — рабочий предел, а не авария, и оценка это учитывает.
        </p>
      </header>

      <div className="flex items-center gap-2">
        <button className="btn" disabled={!!busy} onClick={check}>
          Проверить заново
        </button>
        <button className="btn" disabled={!!busy || !report} onClick={ask}>
          Второе мнение от Claude
        </button>
        {busy && <span className="text-[12px] text-ink-2">{busy}</span>}
        {report && !busy && <span className="text-[11.5px] text-ink-3 ml-auto">снято в {timeHM(report.at * 1000)}</span>}
      </div>

      {err && <div className="text-[12.5px] text-coral">{err}</div>}

      {report && (
        <>
          <div className="grid grid-cols-4 gap-3">
            <Tile label="Требует внимания" value={counts.problem} color={counts.problem ? "var(--color-coral)" : undefined} />
            <Tile label="Можно улучшить" value={counts.warning} color={counts.warning ? "var(--color-amber)" : undefined} />
            <Tile label="К сведению" value={counts.notice} />
            <Tile label="В норме" value={counts.ok} color="var(--color-mint)" />
          </div>

          <Section title="Что нашлось" sub="Отсортировано по важности: сначала то, что стоит посмотреть.">
            <div className="flex flex-col gap-2">
              {sorted.map((f, i) => (
                <Card key={i} f={f} />
              ))}
            </div>
          </Section>
        </>
      )}

      {trends && (
        <Section
          title="Наблюдения во времени"
          sub="Отвечает не на «что сейчас», а на «что меняется». Данные копятся, пока приложение работает."
          right={<Tag>{trends.samples} образцов за {trends.span_days.toFixed(1)} сут</Tag>}
        >
          <div className="grid grid-cols-2 gap-3">
            <div className="panel-2 px-3.5 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="text-[13px]">Охлаждение</div>
                {trends.cooling.enough_data ? (
                  <Tag color={(trends.cooling.gpu_delta_c ?? 0) > 5 ? "var(--color-amber)" : "var(--color-mint)"}>
                    {trends.cooling.compared} замеров
                  </Tag>
                ) : (
                  <Tag>копим данные</Tag>
                )}
              </div>
              {trends.cooling.enough_data && (
                <div className="flex gap-4 mt-2">
                  {trends.cooling.gpu_delta_c != null && (
                    <div>
                      <div className="eyebrow">Видеокарта</div>
                      <div className="num text-[20px]" style={{ color: trends.cooling.gpu_delta_c > 1.5 ? "var(--color-amber)" : "var(--color-mint)" }}>
                        {trends.cooling.gpu_delta_c > 0 ? "+" : ""}
                        {trends.cooling.gpu_delta_c.toFixed(1)} °C
                      </div>
                    </div>
                  )}
                  {trends.cooling.cpu_delta_c != null && (
                    <div>
                      <div className="eyebrow">Процессор</div>
                      <div className="num text-[20px]" style={{ color: trends.cooling.cpu_delta_c > 1.5 ? "var(--color-amber)" : "var(--color-mint)" }}>
                        {trends.cooling.cpu_delta_c > 0 ? "+" : ""}
                        {trends.cooling.cpu_delta_c.toFixed(1)} °C
                      </div>
                    </div>
                  )}
                </div>
              )}
              <div className="text-[12px] text-ink-2 mt-1.5">{trends.cooling.verdict}</div>
              <div className="text-[11.5px] text-ink-3 mt-1">
                Сравнивается температура при одинаковой потребляемой мощности. Рост оборотов сам по себе ни о чём не
                говорит: при фиксированной кривой они и так следуют за температурой.
              </div>
            </div>

            <div className="panel-2 px-3.5 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="text-[13px]">Электричество</div>
                <div className="flex items-center gap-1.5">
                  <input
                    className="input num w-20"
                    type="number"
                    min={0}
                    step={0.5}
                    value={cfg?.power_tariff ?? trends.energy.tariff}
                    onChange={(e) => cfg && saveConfig({ ...cfg, power_tariff: Number(e.target.value) })}
                  />
                  <span className="text-[11.5px] text-ink-3">₽/кВт·ч</span>
                </div>
              </div>
              <div className="flex gap-4 mt-2">
                <div>
                  <div className="eyebrow">Израсходовано</div>
                  <div className="num text-[20px]">{trends.energy.gpu_kwh.toFixed(2)} <span className="text-[11.5px] text-ink-3">кВт·ч</span></div>
                </div>
                <div>
                  <div className="eyebrow">Это стоило</div>
                  <div className="num text-[20px] text-amber">{trends.energy.gpu_cost.toFixed(0)} <span className="text-[11.5px] text-ink-3">₽</span></div>
                </div>
                <div>
                  <div className="eyebrow">В среднем</div>
                  <div className="num text-[20px]">{trends.energy.avg_watts.toFixed(0)} <span className="text-[11.5px] text-ink-3">Вт</span></div>
                </div>
              </div>
              <div className="text-[11.5px] text-ink-3 mt-1.5">{trends.energy.note}</div>
            </div>
          </div>
        </Section>
      )}

      {tasks.length > 0 && (
        <Section
          title="Обслуживание"
          sub="Тяжёлые проверки не запускаются сами: решать, когда это уместно, должен человек. Планировщик только напоминает."
          right={
            <div className="flex gap-2">
              <button className="btn" disabled={!!busy} onClick={saveReport}>
                Отчёт о состоянии
              </button>
              <button className="btn" disabled={!!busy} onClick={saveHistory}>
                История для мастерской
              </button>
            </div>
          }
        >
          <div className="flex flex-col gap-1.5">
            {tasks.map((t) => (
              <div key={t.id} className="flex items-start gap-2.5 py-1.5">
                <StatusPill ok={!t.due} warn={t.due} text={t.due ? "пора" : "сделано"} />
                <div className="min-w-0 flex-1">
                  <div className="text-[13px]">
                    {t.name}
                    <span className="text-ink-3"> · раз в {t.every_days} дн.</span>
                    {!t.automatic && <span className="text-ink-3"> · вручную</span>}
                  </div>
                  <div className="text-[11.5px] text-ink-2 mt-0.5">{t.why}</div>
                  <div className="text-[11.5px] text-ink-3 mt-0.5">
                    {t.days_since != null ? `Последний раз ${t.days_since.toFixed(0)} дн. назад` : "Ни разу не отмечалось"} · {t.where_to}
                  </div>
                </div>
                <button
                  className="btn shrink-0"
                  disabled={!!busy}
                  onClick={() => api.maintenanceDone(t.id).then(setTasks).catch((e) => setErr(String(e)))}
                >
                  Отметить
                </button>
              </div>
            ))}
          </div>

          <div className="panel-2 px-3.5 py-3 mt-3">
            <div className="flex items-center justify-between gap-3">
              <div className="text-[13px]">Уведомления в Telegram</div>
              <Switch
                on={cfg?.notify_enabled ?? false}
                disabled={!cfg?.telegram_bot_token || !cfg?.telegram_chat_id}
                onChange={(v) => cfg && saveConfig({ ...cfg, notify_enabled: v })}
              />
            </div>
            <div className="text-[11.5px] text-ink-2 mt-1">
              Отправляются только сообщения о том, что требует внимания: найденные проблемы в проверке состояния, откат
              разгона и восстановление после сбоя. Замечания и обычные показания не отправляются, иначе уведомления
              быстро научатся игнорировать.
            </div>
            <div className="text-[11.5px] text-ink-3 mt-1">
              Приложение не собирает телеметрию и никуда её не отправляет помимо этого. Токен хранится в файле
              настроек рядом с программой в открытом виде.
            </div>
            <div className="flex items-end gap-2 mt-2.5">
              <div className="flex-1">
                <div className="eyebrow">Токен бота</div>
                <input
                  className="input num w-full mt-1"
                  type="password"
                  placeholder="123456:AA…"
                  value={cfg?.telegram_bot_token ?? ""}
                  onChange={(e) => cfg && saveConfig({ ...cfg, telegram_bot_token: e.target.value })}
                />
              </div>
              <div className="w-44">
                <div className="eyebrow">Чат</div>
                <input
                  className="input num w-full mt-1"
                  placeholder="123456789"
                  value={cfg?.telegram_chat_id ?? ""}
                  onChange={(e) => cfg && saveConfig({ ...cfg, telegram_chat_id: e.target.value })}
                />
              </div>
              <button
                className="btn"
                disabled={!!busy || !cfg?.telegram_bot_token || !cfg?.telegram_chat_id}
                onClick={() =>
                  api
                    .notifyTest()
                    .then((r) => setMsgOk(r.detail))
                    .catch((e) => setErr(String(e)))
                }
              >
                Проверить связь
              </button>
            </div>
          </div>

          {msgOk && <div className="text-[12.5px] text-mint mt-2">{msgOk}</div>}
        </Section>
      )}

      {updates && (
        <Section
          title="Версии драйверов и прошивки"
          sub="Приложение показывает, что установлено и насколько это старое. Какая версия последняя — оно не знает и не притворяется."
        >
          <div className="flex flex-col gap-1.5">
            {updates.components.map((c, i) => (
              <div key={i} className="flex items-start gap-2.5 py-1.5">
                <StatusPill ok={!c.stale} warn={c.stale} text={c.stale ? "стоит проверить" : "свежее"} />
                <div className="min-w-0 flex-1">
                  <div className="text-[13px]">{c.name}</div>
                  <div className="text-[11.5px] text-ink-2 mt-0.5">
                    <span className="num">{c.installed}</span>
                    {c.date && ` · от ${c.date}`}
                    {c.age_days != null && ` · это ${c.age_days} дн. назад`}
                  </div>
                  <div className="text-[11.5px] text-ink-3 mt-0.5">{c.note}</div>
                </div>
                <button className="btn shrink-0" onClick={() => api.openUri(c.check_at)}>
                  Проверить
                </button>
              </div>
            ))}
          </div>
          <div className="text-[11.5px] text-ink-3 mt-3">{updates.disclaimer}</div>
        </Section>
      )}

      {advice && (
        <Section title="Второе мнение" sub="Claude смотрит на тот же отчёт и добавляет контекст, которого нет в зашитых порогах.">
          <Markdown text={advice} />
        </Section>
      )}
    </div>
  );
}
