import { useEffect, useRef, useState } from "react";
import { AreaChart, Area, ResponsiveContainer, YAxis, XAxis, Tooltip, CartesianGrid } from "recharts";
import { useStore } from "../store";
import { Section, Tile, Tag, StatusPill, Bar, Label } from "../components/ui";
import {
  timeHM,
  api,
  type GpuNvapi,
  type GpuCapabilities,
  type OcJournal,
  type GpuCandidate,
  type OcStage,
  type OcSuggestion,
  type StageVerdict,
} from "../lib/api";
import { runGpuTest, type GpuTestProgress } from "../lib/gputest";

/** Длительности ступеней должны совпадать с Stage::seconds в ocsafe.rs. */
const STAGE_SECONDS: Record<OcStage, number> = { smoke: 30, medium: 300, long: 1800 };

const history: { t: number; util: number; temp: number; power: number }[] = [];

/** Подписи датчиков температуры, как их называет NVAPI. */
const TEMP_LABEL: Record<string, string> = {
  gpu: "Ядро",
  memory: "Память",
  power_supply: "Питание",
  board: "Плата",
  vcd_board: "Плата VCD",
  vcd_inlet: "Вход VCD",
  vcd_outlet: "Выход VCD",
  unknown: "Неизвестный датчик",
};

/** Строка матрицы возможностей: что именно приложение сможет менять. */
function Capability({ on, name, note }: { on: boolean; name: string; note: string }) {
  return (
    <div className="flex items-start gap-2.5 py-1.5">
      <StatusPill ok={on} text={on ? "есть" : "нет"} />
      <div className="min-w-0">
        <div className="text-[13px]">{name}</div>
        <div className="text-[11.5px] text-ink-2">{note}</div>
      </div>
    </div>
  );
}

export default function Gpu() {
  const snap = useStore((s) => s.snap);
  const lastT = useRef(0);
  const [nv, setNv] = useState<GpuNvapi | null>(null);
  const [caps, setCaps] = useState<GpuCapabilities | null>(null);
  const [nvError, setNvError] = useState<string | null>(null);
  const [oc, setOc] = useState<OcJournal | null>(null);
  const [draft, setDraft] = useState<GpuCandidate>({ core_offset_mhz: 0, mem_offset_mhz: 0, power_percent: 100, fan_level: null });
  const [busy, setBusy] = useState(false);
  const [ocMsg, setOcMsg] = useState<string | null>(null);
  const [ocErr, setOcErr] = useState<string | null>(null);

  const [testing, setTesting] = useState(false);
  const [progress, setProgress] = useState<GpuTestProgress | null>(null);
  const [auto, setAuto] = useState(false);
  const [suggestion, setSuggestion] = useState<OcSuggestion | null>(null);
  // Пик температуры набирается из общего опроса NVAPI, чтобы не дёргать драйвер отдельно.
  const testingRef = useRef(false);
  const peakRef = useRef<number | null>(null);
  const autoRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);

  const refreshOc = () => api.ocState().then(setOc).catch(() => setOc(null));

  /** Прогоняет нагрузку на время ступени и передаёт улики бэкенду за вердиктом. */
  const runStageFor = async (stage: OcStage): Promise<StageVerdict | null> => {
    const seconds = STAGE_SECONDS[stage] ?? 30;
    const startedAt = Math.floor(Date.now() / 1000);
    peakRef.current = nv?.temperatures.find((t) => t.target === "gpu")?.current_c ?? null;
    testingRef.current = true;
    setTesting(true);
    setOcErr(null);
    setOcMsg(null);
    setProgress(null);
    try {
      const r = await runGpuTest(seconds, setProgress, abortRef.current?.signal);
      if (!r.available) {
        setOcErr(r.reason ?? "WebGPU недоступен");
        return null;
      }
      const verdict = await api.ocValidate({
        gpu_mismatches: r.mismatches,
        started_at: startedAt,
        peak_temp_c: peakRef.current,
        completed: r.completed,
      });
      setOc(verdict.journal);
      setOcMsg(
        `${verdict.reason} Просчитано ${r.dispatches} проходов${r.reason ? `. ${r.reason}` : ""}` +
          (verdict.faults.length ? ` Сбои драйвера: ${verdict.faults.join(", ")}.` : ""),
      );
      return verdict;
    } catch (e) {
      setOcErr(String(e));
      return null;
    } finally {
      testingRef.current = false;
      setTesting(false);
      setProgress(null);
    }
  };

  const runStage = async () => {
    if (oc?.pending) await runStageFor(oc.pending.stage);
  };

  /**
   * Автоподбор: спросить шаг → применить → прогнать ступень → вернуть исход
   * модели следующим сообщением. Останавливается по кнопке, по решению модели
   * или по ошибке — молча крутиться в пустоту петля не должна.
   */
  const autoLoop = async () => {
    if (autoRef.current) {
      autoRef.current = false;
      abortRef.current?.abort();
      return;
    }
    autoRef.current = true;
    setAuto(true);
    abortRef.current = new AbortController();
    let note = "";
    try {
      while (autoRef.current) {
        let journal = await api.ocState();

        if (!journal.pending) {
          const s = await api.ocPropose(note);
          setSuggestion(s);
          if (s.proposal.stop) {
            setOcMsg(`Подбор закончен по решению модели: ${s.proposal.reasoning}`);
            break;
          }
          await api.ocApply(s.candidate, "smoke");
          journal = await api.ocState();
          setOc(journal);
        }

        if (!journal.pending) break;
        const verdict = await runStageFor(journal.pending.stage);
        if (!verdict) break; // ошибка теста уже показана
        note = verdict.passed
          ? `Предыдущий шаг прошёл проверку: ${verdict.reason}`
          : `Предыдущий шаг откачен: ${verdict.reason}`;
        if (!autoRef.current) break;
      }
    } catch (e) {
      setOcErr(String(e));
    } finally {
      autoRef.current = false;
      setAuto(false);
      abortRef.current = null;
      await refreshOc();
    }
  };

  useEffect(() => {
    api
      .ocState()
      .then((j) => {
        setOc(j);
        // За исходную точку берём то, что уже признано проверенным.
        if (j.last_known_good) setDraft(j.last_known_good);
      })
      .catch(() => setOc(null));
  }, []);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setOcErr(null);
    try {
      await fn();
      await refreshOc();
    } catch (e) {
      setOcErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    api.gpuCapabilities().then(setCaps).catch(() => setCaps(null));
  }, []);

  // NVAPI опрашивается отдельно от общего снимка: вызовы дешёвые, но идут в драйвер.
  useEffect(() => {
    let alive = true;
    const tick = () =>
      api
        .gpuNvapi()
        .then((v) => {
          if (!alive) return;
          setNv(v);
          setNvError(null);
          if (testingRef.current) {
            const t = v.temperatures.find((s) => s.target === "gpu")?.current_c;
            if (t != null) peakRef.current = Math.max(peakRef.current ?? t, t);
          }
        })
        .catch((e) => alive && setNvError(String(e)));
    tick();
    const id = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (!snap || !snap.gpu.available) return;
    if (snap.ts !== lastT.current) {
      lastT.current = snap.ts;
      history.push({ t: snap.ts, util: snap.gpu.util_pct, temp: snap.gpu.temp_c, power: snap.gpu.power_w });
      if (history.length > 180) history.shift();
    }
  }, [snap]);

  if (!snap) return <div className="text-ink-3">Загрузка…</div>;
  const g = snap.gpu;

  // Лимит мощности приходит в процентах от штатного — переводим в ватты по паспортному значению.
  const wattsAt = (pct: number | null) =>
    pct != null && g.power_limit_w ? Math.round((pct / 100) * g.power_limit_w) : null;

  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Видеокарта</div>
        <h1 className="text-[24px] mt-1">{nv?.name || (g.available ? g.name : "NVIDIA GPU не найдена")}</h1>
        <p className="text-ink-2 text-[12.5px] mt-1">
          Телеметрия и управление через NVAPI напрямую из драйвера. Смещения частот, лимит мощности и режим
          вентиляторов держатся только до перезагрузки, поэтому любой сбой откатывает их сам.
        </p>
      </header>

      {g.available && (
        <>
          <div className="grid grid-cols-6 gap-3">
            <Tile label="Загрузка" value={g.util_pct.toFixed(0)} unit="%" color="var(--color-mint)" />
            <Tile
              label="Температура"
              value={g.temp_c.toFixed(0)}
              unit="°C"
              color={g.temp_c > 80 ? "var(--color-coral)" : g.temp_c > 70 ? "var(--color-amber)" : "var(--color-mint)"}
            />
            <Tile label="Мощность" value={g.power_w.toFixed(0)} unit="Вт" />
            <Tile label="Частота ядра" value={g.clock_mhz.toFixed(0)} unit="МГц" />
            <Tile label="Частота памяти" value={g.mem_clock_mhz.toFixed(0)} unit="МГц" />
            <Tile label="Вентилятор" value={g.fan_pct.toFixed(0)} unit="%" />
          </div>

          <Section title="История, последние 6 минут">
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={[...history]} margin={{ top: 8, right: 12, left: -10, bottom: 0 }}>
                <CartesianGrid stroke="#1b2230" vertical={false} />
                <XAxis dataKey="t" tickFormatter={(v) => timeHM(Number(v))} stroke="#2d3a4f" tick={{ fill: "#5f6b80", fontSize: 10 }} minTickGap={60} />
                <YAxis domain={[0, 100]} stroke="#2d3a4f" tick={{ fill: "#5f6b80", fontSize: 10 }} width={40} />
                <Tooltip contentStyle={{ background: "#121721", border: "1px solid #2d3a4f", borderRadius: 8, fontSize: 12 }} labelFormatter={(v) => timeHM(Number(v))} isAnimationActive={false} />
                <Area type="monotone" dataKey="util" name="Загрузка %" stroke="#5fd18c" fill="#5fd18c" fillOpacity={0.15} strokeWidth={1.8} isAnimationActive={false} dot={false} />
                <Area type="monotone" dataKey="temp" name="Температура °C" stroke="#f2b33d" fill="#f2b33d" fillOpacity={0.06} strokeWidth={1.4} isAnimationActive={false} dot={false} />
              </AreaChart>
            </ResponsiveContainer>
          </Section>
        </>
      )}

      {nvError && (
        <Section title="NVAPI недоступна" sub="Раздел ниже работает только с картой NVIDIA и установленным драйвером.">
          <div className="text-[12.5px] text-coral">{nvError}</div>
        </Section>
      )}

      {nv && (
        <>
          <Section
            title="Лимит мощности"
            sub="Границы задаёт сам драйвер — выйти за них нельзя даже намеренно."
            right={<Tag>{nv.power_current_percent?.toFixed(0)} % от штатного</Tag>}
          >
            <div className="grid grid-cols-4 gap-3">
              <Tile
                label="Сейчас"
                value={wattsAt(nv.power_current_percent) ?? "—"}
                unit="Вт"
                hint={`${nv.power_current_percent?.toFixed(1)} %`}
                color="var(--color-teal)"
              />
              <Tile label="Минимум" value={wattsAt(nv.power_min_percent) ?? "—"} unit="Вт" hint={`${nv.power_min_percent?.toFixed(1)} %`} />
              <Tile label="Штатный" value={wattsAt(nv.power_default_percent) ?? "—"} unit="Вт" hint={`${nv.power_default_percent?.toFixed(1)} %`} />
              <Tile
                label="Максимум"
                value={wattsAt(nv.power_max_percent) ?? "—"}
                unit="Вт"
                hint={`${nv.power_max_percent?.toFixed(1)} %`}
                color="var(--color-amber)"
              />
            </div>
            <div className="mt-3">
              <Bar
                value={(nv.power_current_percent ?? 0) - (nv.power_min_percent ?? 0)}
                max={(nv.power_max_percent ?? 100) - (nv.power_min_percent ?? 0)}
                color="var(--color-teal)"
              />
            </div>
          </Section>

          <Section title="Смещения частот" sub="Ноль означает, что разгон сейчас не применён.">
            <div className="grid grid-cols-2 gap-3">
              {(
                [
                  ["Ядро", nv.core_offset],
                  ["Память", nv.mem_offset],
                ] as const
              ).map(([label, o]) => (
                <div key={label} className="panel-2 px-3.5 py-3">
                  <div className="flex items-center justify-between">
                    <div className="eyebrow">{label}</div>
                    {o.editable ? <Tag color="var(--color-mint)">редактируемо</Tag> : <Tag color="var(--color-coral)">заблокировано</Tag>}
                  </div>
                  <div className="num text-[22px] leading-none font-medium mt-1">
                    {o.current_mhz > 0 ? "+" : ""}
                    {o.current_mhz} <span className="text-[11.5px] text-ink-3">МГц</span>
                  </div>
                  <div className="text-[11.5px] text-ink-2 mt-1">
                    Драйвер разрешает от {o.min_mhz} до +{o.max_mhz} МГц
                  </div>
                </div>
              ))}
            </div>
          </Section>

          <Section title="Вентиляторы" sub={`Драйвер видит ${nv.fans.length} шт. Пока все под автоматикой драйвера.`}>
            <div className="grid grid-cols-3 gap-3">
              {nv.fans.map((f) => (
                <div key={f.cooler_id} className="panel-2 px-3.5 py-3">
                  <div className="flex items-center justify-between">
                    <div className="eyebrow">Вентилятор {f.cooler_id}</div>
                    <Tag color={f.automatic ? "var(--color-ink-3)" : "var(--color-amber)"}>{f.automatic ? "авто" : "вручную"}</Tag>
                  </div>
                  <div className="num text-[22px] leading-none font-medium mt-1">
                    {f.rpm} <span className="text-[11.5px] text-ink-3">об/мин</span>
                  </div>
                  <div className="mt-2">
                    <Bar value={f.level_percent} max={100} color="var(--color-teal)" />
                  </div>
                  <div className="text-[11.5px] text-ink-2 mt-1">
                    {f.level_percent} % · допустимо {f.min_level}–{f.max_level} %
                  </div>
                </div>
              ))}
            </div>
          </Section>

          {nv.temperatures.length > 0 && (
            <Section title="Датчики температуры">
              <div className="grid grid-cols-4 gap-3">
                {nv.temperatures.map((t) => (
                  <Tile
                    key={t.target}
                    label={TEMP_LABEL[t.target] ?? t.target}
                    value={t.current_c}
                    unit="°C"
                    hint={`предел ${t.max_c} °C`}
                    color={t.current_c > 80 ? "var(--color-coral)" : t.current_c > 70 ? "var(--color-amber)" : "var(--color-mint)"}
                  />
                ))}
              </div>
            </Section>
          )}
        </>
      )}

      {oc && (
        <Section
          title="Подбор разгона"
          sub="Значения обрезаются коридором до того, как попадут в драйвер — выйти за него нельзя ни вручную, ни через нейросеть."
          right={
            oc.last_known_good ? (
              <Tag color="var(--color-mint)">есть проверенная настройка</Tag>
            ) : (
              <Tag>проверенных настроек пока нет</Tag>
            )
          }
        >
          <div className="grid grid-cols-4 gap-3">
            {(
              [
                ["Ядро, МГц", "core_offset_mhz", oc.bounds.core_min_mhz, oc.bounds.core_max_mhz, 10],
                ["Память, МГц", "mem_offset_mhz", oc.bounds.mem_min_mhz, oc.bounds.mem_max_mhz, 50],
                ["Мощность, %", "power_percent", oc.bounds.power_min_percent, oc.bounds.power_max_percent, 1],
              ] as const
            ).map(([label, key, min, max, step]) => (
              <div key={key} className="panel-2 px-3.5 py-3">
                <div className="eyebrow">{label}</div>
                <input
                  className="input num w-full mt-1.5"
                  type="number"
                  min={min}
                  max={max}
                  step={step}
                  value={draft[key]}
                  disabled={busy || !!oc.pending}
                  onChange={(e) => setDraft({ ...draft, [key]: Number(e.target.value) })}
                />
                <div className="text-[11.5px] text-ink-2 mt-1">
                  коридор {min}…{max}
                </div>
              </div>
            ))}
            <div className="panel-2 px-3.5 py-3">
              <div className="eyebrow">Вентиляторы, %</div>
              <input
                className="input num w-full mt-1.5"
                type="number"
                min={oc.bounds.fan_min_percent}
                max={oc.bounds.fan_max_percent}
                step={5}
                placeholder="авто"
                value={draft.fan_level ?? ""}
                disabled={busy || !!oc.pending}
                onChange={(e) => setDraft({ ...draft, fan_level: e.target.value === "" ? null : Number(e.target.value) })}
              />
              <div className="text-[11.5px] text-ink-2 mt-1">пусто — управляет драйвер</div>
            </div>
          </div>

          {oc.pending ? (
            <div className="panel-2 px-3.5 py-3 mt-3">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13px]">
                    Идёт проверка: ядро {oc.pending.candidate.core_offset_mhz > 0 ? "+" : ""}
                    {oc.pending.candidate.core_offset_mhz} МГц, память {oc.pending.candidate.mem_offset_mhz > 0 ? "+" : ""}
                    {oc.pending.candidate.mem_offset_mhz} МГц, мощность {oc.pending.candidate.power_percent.toFixed(0)} %
                  </div>
                  <div className="text-[11.5px] text-ink-2 mt-0.5">
                    Ступень «{oc.pending.stage}». Пока настройка не подтверждена, вылет или перезагрузка отправят её в чёрный список.
                  </div>
                </div>
                <div className="flex gap-2 shrink-0">
                  <button className="btn" disabled={busy || testing} onClick={runStage}>
                    {testing
                      ? `Идёт проверка… ${progress ? `${Math.round(progress.seconds)} с` : ""}`
                      : `Запустить проверку · ${STAGE_SECONDS[oc.pending.stage] < 60 ? `${STAGE_SECONDS[oc.pending.stage]} с` : `${STAGE_SECONDS[oc.pending.stage] / 60} мин`}`}
                  </button>
                  <button className="btn" disabled={busy || testing} onClick={() => run(() => api.ocReject("отклонено вручную"))}>
                    Откатить
                  </button>
                </div>
              </div>
              {testing && (
                <div className="mt-2.5">
                  <Bar
                    value={progress?.seconds ?? 0}
                    max={STAGE_SECONDS[oc.pending.stage] ?? 30}
                    color={progress && progress.mismatches > 0 ? "var(--color-coral)" : "var(--color-teal)"}
                  />
                  <div className="text-[11.5px] text-ink-2 mt-1">
                    Просчитано {progress?.dispatches ?? 0} проходов, расхождений {progress?.mismatches ?? 0}
                    {peakRef.current != null && `, пик температуры ${peakRef.current} °C`}
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="flex items-center gap-2 mt-3">
              {(["smoke", "medium", "long"] as OcStage[]).map((s) => (
                <button
                  key={s}
                  className="btn"
                  disabled={busy || !caps?.set_clock_offsets}
                  onClick={() => run(async () => setOcMsg((await api.ocApply(draft, s)).message))}
                >
                  Применить · {s === "smoke" ? "30 с" : s === "medium" ? "5 мин" : "30 мин"}
                </button>
              ))}
              <button className="btn ml-auto" disabled={busy} onClick={() => run(() => api.ocReset())}>
                Снять разгон
              </button>
            </div>
          )}

          <div className="flex items-center gap-2 mt-2">
            <button className="btn" disabled={busy || (testing && !auto)} onClick={autoLoop}>
              {auto ? "Остановить автоподбор" : "Автоподбор с Claude"}
            </button>
            <button
              className="btn"
              disabled={busy || auto || testing || !!oc.pending}
              onClick={() => run(async () => setSuggestion(await api.ocPropose("")))}
            >
              Спросить один шаг
            </button>
            <span className="text-[11.5px] text-ink-2">
              Модель предлагает, коридор решает: предложение обрезается границами до записи в драйвер.
            </span>
          </div>

          {suggestion && (
            <div className="panel-2 px-3.5 py-3 mt-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13px]">
                    Предложение: ядро {suggestion.candidate.core_offset_mhz > 0 ? "+" : ""}
                    {suggestion.candidate.core_offset_mhz} МГц, память {suggestion.candidate.mem_offset_mhz > 0 ? "+" : ""}
                    {suggestion.candidate.mem_offset_mhz} МГц, мощность {suggestion.candidate.power_percent.toFixed(0)} %
                    {suggestion.candidate.fan_level != null && `, вентиляторы ${suggestion.candidate.fan_level} %`}
                  </div>
                  <div className="text-[12px] text-ink-2 mt-1">{suggestion.proposal.reasoning}</div>
                  <div className="text-[12px] text-ink-2 mt-1">
                    <span className="text-ink-3">Ожидание: </span>
                    {suggestion.proposal.expectation}
                  </div>
                </div>
                <div className="flex flex-col items-end gap-1 shrink-0">
                  {suggestion.proposal.stop && <Tag color="var(--color-amber)">модель предлагает остановиться</Tag>}
                  {suggestion.clamped && <Tag color="var(--color-amber)">урезано коридором</Tag>}
                  {suggestion.proposal.confidence && <Tag>уверенность: {suggestion.proposal.confidence}</Tag>}
                </div>
              </div>
              {suggestion.clamped && (
                <div className="text-[11.5px] text-ink-2 mt-1.5">
                  Модель просила ядро {suggestion.proposal.core_offset_mhz > 0 ? "+" : ""}
                  {suggestion.proposal.core_offset_mhz} МГц, память {suggestion.proposal.mem_offset_mhz > 0 ? "+" : ""}
                  {suggestion.proposal.mem_offset_mhz} МГц, мощность {suggestion.proposal.power_percent.toFixed(0)} % — значения приведены в коридор.
                </div>
              )}
              <div className="flex items-center gap-2 mt-2.5">
                <button
                  className="btn"
                  disabled={busy || auto || testing || !!oc.pending || suggestion.proposal.stop}
                  onClick={() => run(async () => setOcMsg((await api.ocApply(suggestion.candidate, "smoke")).message))}
                >
                  Применить предложение
                </button>
                <button className="btn" disabled={busy || auto} onClick={() => setSuggestion(null)}>
                  Отклонить
                </button>
                <span className="text-[11.5px] text-ink-3 ml-auto">
                  {suggestion.model} · {suggestion.input_tokens} вход / {suggestion.output_tokens} выход
                </span>
              </div>
            </div>
          )}

          {ocMsg && <div className="text-[12.5px] text-ink-2 mt-2">{ocMsg}</div>}
          {ocErr && <div className="text-[12.5px] text-coral mt-2">{ocErr}</div>}

          {oc.rejected.length > 0 && (
            <div className="mt-3">
              <div className="eyebrow mb-1.5">Отклонённые комбинации — повторно не предлагаются</div>
              <div className="flex flex-col gap-1">
                {oc.rejected.slice(-5).reverse().map((r, i) => (
                  <div key={i} className="text-[12px] text-ink-2">
                    ядро {r.candidate.core_offset_mhz > 0 ? "+" : ""}
                    {r.candidate.core_offset_mhz}, память {r.candidate.mem_offset_mhz > 0 ? "+" : ""}
                    {r.candidate.mem_offset_mhz}, мощность {r.candidate.power_percent.toFixed(0)} % — {r.reason}
                  </div>
                ))}
              </div>
            </div>
          )}

          {oc.history.length > 0 && (
            <div className="mt-3">
              <div className="eyebrow mb-1.5">Журнал</div>
              <div className="flex flex-col gap-1">
                {oc.history.slice(-6).reverse().map((h, i) => (
                  <div key={i} className="text-[12px] text-ink-2">
                    <span className="num text-ink-3">{timeHM(h.at * 1000)}</span> · {h.text}
                  </div>
                ))}
              </div>
            </div>
          )}
        </Section>
      )}

      {caps && (
        <Section
          title="Что доступно на этой машине"
          sub="Проверено обращением к драйверу, а не предположением по модели карты."
        >
          <div className="grid grid-cols-2 gap-x-6">
            <Capability on={caps.nvapi} name="NVAPI отвечает" note={`Найдено карт NVIDIA: ${caps.gpu_count}`} />
            <Capability on={caps.read_pstates} name="Чтение кривой V/F" note="Текущие смещения и границы, разрешённые драйвером" />
            <Capability on={caps.set_power_limit} name="Изменение лимита мощности" note="Главный рычаг: держит температуру и шум под контролем" />
            <Capability on={caps.set_clock_offsets} name="Смещения частот и андервольт" note="Слетают при перезагрузке — поэтому откат всегда гарантирован" />
            <Capability on={caps.set_fan_curve} name="Кривая вентиляторов" note="Через NVAPI, без kernel-драйвера и риска для анти-читов" />
            <Capability
              on={caps.set_voltage_direct}
              name="Прямое управление напряжением"
              note="На Blackwell не экспортируется — андервольт делается смещением точек V/F"
            />
          </div>
          <div className="mt-3">
            <Label
              title="Почему разгон видеокарты безопаснее, чем звучит"
              text="Смещения частот, лимит мощности и режим вентиляторов драйвер держит только до перезагрузки и не записывает в саму карту. Любая перезагрузка — в том числе аварийная — возвращает штатные значения."
              rec="Именно поэтому подбор параметров видеокарты можно доверить автоматике, а изменения в BIOS — нет."
            >
              <span className="text-[12px] text-ink-2">Как устроен откат</span>
            </Label>
          </div>
        </Section>
      )}
    </div>
  );
}
