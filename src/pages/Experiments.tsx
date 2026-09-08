import { useEffect, useRef, useState } from "react";
import { Section, Tag, StatusPill, Label } from "../components/ui";
import { api, type VoltageState, type TuneState } from "../lib/api";

/** Длительность одного шага подбора и объём проверяемой памяти. */
const STEP_SECONDS = 60;
const STEP_MEMORY_MB = 512;

export default function Experiments() {
  const [st, setSt] = useState<VoltageState | null>(null);
  const [draft, setDraft] = useState<Record<number, number>>({});
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [armed, setArmed] = useState(false);
  const [tune, setTune] = useState<TuneState | null>(null);
  const [tuning, setTuning] = useState(false);
  const tuningRef = useRef(false);

  /**
   * Подбор идёт шагами, а не одной длинной командой: каждый шаг применяет
   * следующее смещение и сам себя проверяет, а между шагами интерфейс успевает
   * показать результат и дать возможность остановиться.
   */
  const runTuning = async () => {
    if (tuningRef.current) {
      tuningRef.current = false;
      await api.cpuTuneStop().then(setTune).catch((e) => setErr(String(e)));
      setTuning(false);
      return;
    }
    setErr(null);
    setMsg(null);
    tuningRef.current = true;
    setTuning(true);
    try {
      setTune(await api.cpuTuneStart());
      while (tuningRef.current) {
        const r = await api.cpuTuneStep(STEP_SECONDS, STEP_MEMORY_MB);
        setTune(r.state);
        setMsg(r.detail);
        if (!r.passed || r.state.finished) break;
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      tuningRef.current = false;
      setTuning(false);
      await refresh();
    }
  };

  const refresh = () =>
    api
      .voltageState()
      .then((v) => {
        setSt(v);
        setDraft(Object.fromEntries(v.items.map((i) => [i.id, i.current_mv])));
      })
      .catch((e) => setErr(String(e)));

  useEffect(() => {
    refresh();
    api.cpuTuneState().then(setTune).catch(() => setTune(null));
  }, []);

  const run = async (fn: () => Promise<unknown>, ok?: string) => {
    setBusy(true);
    setErr(null);
    setMsg(null);
    try {
      await fn();
      if (ok) setMsg(ok);
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Эксперименты</div>
        <h1 className="text-[24px] mt-1">Напряжение процессора</h1>
        <p className="text-ink-2 text-[12.5px] mt-1">
          Смещение напряжения через штатный ACPI-интерфейс платы — тот же, через который работают вентиляторы. Кернел-драйвер
          не нужен. Раздел помечен бетой: интерфейс проверен чтением, но опробован меньше остального в приложении.
        </p>
      </header>

      <Section title="Как это устроено и почему так" sub="Прочитайте до того, как что-то менять.">
        <div className="grid grid-cols-3 gap-3 text-[12.5px] text-ink-2 leading-relaxed">
          <div className="panel-2 p-3">
            <div className="text-ink font-semibold mb-1">Изменения не переживают перезагрузку</div>
            Плата не записывает эти значения в прошивку — метода фиксации в интерфейсе нет вовсе. Любая перезагрузка,
            включая аварийную, снимает смещение. Это и есть встроенный откат: если подобрали слишком глубоко и система
            зависла, достаточно нажать Reset.
          </div>
          <div className="panel-2 p-3">
            <div className="text-ink font-semibold mb-1">Только понижение, никогда повышение</div>
            Прошивка разрешает ±300 мВ, но приложение пускает только вниз. Слишком глубокий андервольт вызывает зависание,
            которое лечится перезагрузкой. Повышенное напряжение не ломает ничего сразу, но необратимо ускоряет деградацию
            кристалла — вред, который нельзя ни отменить, ни заметить.
          </div>
          <div className="panel-2 p-3">
            <div className="text-ink font-semibold mb-1">Прямая установка напряжения отключена</div>
            Задать абсолютное значение вместо смещения означает отменить автоматическое управление: процессор перестанет
            снижать напряжение в простое и будет держать заданное всегда. Такие регуляторы показаны, но заблокированы.
          </div>
        </div>
      </Section>

      {!st ? (
        <div className="text-ink-3">Загрузка…</div>
      ) : !st.available ? (
        <Section title="Интерфейс недоступен">
          <div className="text-[12.5px] text-ink-2">{st.note}</div>
        </Section>
      ) : (
        <Section
          title="Регуляторы"
          sub={st.note}
          right={
            <div className="flex items-center gap-2">
              {st.interface_version && <Tag>интерфейс {st.interface_version}</Tag>}
              <button className="btn" disabled={busy} onClick={() => run(() => api.voltageReset(), "Все смещения сняты.")}>
                Снять смещения
              </button>
            </div>
          }
        >
          <div className="panel-2 px-3.5 py-3 mb-3 border-l-2" style={{ borderLeftColor: "var(--color-amber)" }}>
            <div className="flex items-center justify-between gap-3">
              <div className="text-[12.5px] text-ink-2">
                Изменение напряжения процессора может привести к зависанию. Несохранённая работа будет потеряна.
                Перед первым подбором закройте всё важное.
              </div>
              <label className="flex items-center gap-2 shrink-0 cursor-pointer text-[12.5px]">
                <input type="checkbox" checked={armed} onChange={(e) => setArmed(e.target.checked)} />
                Понимаю, разрешить изменения
              </label>
            </div>
          </div>

          <div className="flex flex-col gap-2">
            {st.items.map((it) => {
              const value = draft[it.id] ?? it.current_mv;
              const changed = value !== it.current_mv;
              return (
                <div key={it.id} className="panel-2 px-3.5 py-3">
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2.5 min-w-0">
                      <StatusPill ok={it.adjustable} warn={!it.adjustable} text={it.adjustable ? "доступно" : "заблокировано"} />
                      <div className="min-w-0">
                        <div className="text-[13px]">{it.name}</div>
                        <div className="text-[11.5px] text-ink-2">
                          сейчас {it.current_mv > 0 ? "+" : ""}
                          {it.current_mv} мВ · прошивка допускает {it.firmware_min_mv}…{it.firmware_max_mv} мВ шагом {it.step_mv}
                        </div>
                      </div>
                    </div>
                    {it.is_offset ? <Tag>смещение</Tag> : <Tag color="var(--color-coral)">абсолютное</Tag>}
                  </div>

                  {it.adjustable && (
                    <div className="flex items-center gap-3 mt-2.5">
                      <input
                        type="range"
                        className="flex-1"
                        min={it.allowed_min_mv}
                        max={it.allowed_max_mv}
                        step={it.step_mv}
                        value={value}
                        disabled={busy || !armed}
                        onChange={(e) => setDraft({ ...draft, [it.id]: Number(e.target.value) })}
                      />
                      <span className="num text-[15px] w-20 text-right">
                        {value > 0 ? "+" : ""}
                        {value} мВ
                      </span>
                      <button
                        className="btn"
                        disabled={busy || !armed || !changed}
                        onClick={() =>
                          run(async () => {
                            const applied = await api.voltageSetOffset(it.id, value);
                            if (applied !== value) setMsg(`Значение приведено к допустимому: ${applied} мВ.`);
                          }, `«${it.name}» — применено ${value} мВ.`)
                        }
                      >
                        Применить
                      </button>
                    </div>
                  )}

                  <div className="text-[11.5px] text-ink-3 mt-1.5">{it.note}</div>
                </div>
              );
            })}
          </div>

          {msg && <div className="text-[12.5px] text-ink-2 mt-2">{msg}</div>}
          {err && <div className="text-[12.5px] text-coral mt-2">{err}</div>}

          <div className="panel-2 px-3.5 py-3 mt-3">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="text-[13px]">Автоподбор смещения</div>
                <div className="text-[11.5px] text-ink-2 mt-0.5">
                  Шаг 25 мВ вниз, после каждого — {STEP_SECONDS} секунд нагрузки на процессор и проверка памяти. При
                  первом сбое возврат на два шага вверх.
                </div>
              </div>
              <button className="btn shrink-0" disabled={busy || !armed} onClick={runTuning}>
                {tuning ? "Остановить" : "Начать подбор"}
              </button>
            </div>

            {tune && (tune.history.length > 0 || tuning) && (
              <>
                <div className="grid grid-cols-3 gap-3 mt-3">
                  <div>
                    <div className="eyebrow">Устойчиво до</div>
                    <div className="num text-[20px] text-mint">{tune.deepest_stable_mv} мВ</div>
                  </div>
                  <div>
                    <div className="eyebrow">Сбой на</div>
                    <div className="num text-[20px]" style={{ color: tune.first_unstable_mv != null ? "var(--color-coral)" : undefined }}>
                      {tune.first_unstable_mv != null ? `${tune.first_unstable_mv} мВ` : "—"}
                    </div>
                  </div>
                  <div>
                    <div className="eyebrow">Оставлено</div>
                    <div className="num text-[20px] text-teal">
                      {tune.recommended_mv != null ? `${tune.recommended_mv} мВ` : "—"}
                    </div>
                  </div>
                </div>

                {tuning && (
                  <div className="text-[12px] text-amber mt-2">
                    Идёт шаг: процессор под полной нагрузкой. Это займёт около минуты на шаг.
                  </div>
                )}

                {tune.history.length > 0 && (
                  <div className="mt-2.5 flex flex-col gap-1">
                    {tune.history.slice(-8).reverse().map((h, i) => (
                      <div key={i} className="text-[11.5px] text-ink-2">
                        <span className="num text-ink-3">{h.offset_mv} мВ</span> · {h.detail}
                      </div>
                    ))}
                  </div>
                )}
              </>
            )}
          </div>

          <div className="mt-3">
            <Label
              title="Как подбирать смещение"
              text="Идите шагами по 25–30 мВ вниз и после каждого прогоняйте тест процессора и памяти на странице «Процессор». Признак того, что зашли слишком глубоко, — не обязательно вылет: чаще это расхождения контрольных сумм в тесте или зависания в простое, а не под нагрузкой."
              rec="Найдя первое неустойчивое значение, вернитесь на два шага вверх — запас нужен на жару и просадки питания."
              warn="Смещение снимается перезагрузкой. Если система зависла, просто перезагрузите её: ничего восстанавливать не придётся."
            >
              <span className="text-[12px] text-ink-2">Порядок подбора</span>
            </Label>
          </div>
        </Section>
      )}
    </div>
  );
}
