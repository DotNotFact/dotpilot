import { useEffect, useState } from "react";
import { Section, Tile, Tag, StatusPill, Switch, Label } from "../components/ui";
import { api, type FanControllerState } from "../lib/api";

/** Подписи датчиков платы. Номер 2 — тот, по которому плата ведёт вентиляторы. */
const SENSOR_LABEL: Record<number, string> = {
  0: "Датчик 0",
  1: "Датчик 1",
  2: "Процессор",
  3: "Датчик 3",
  4: "Датчик 4",
  5: "Датчик 5",
};

export default function Fans() {
  const [st, setSt] = useState<FanControllerState | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [draft, setDraft] = useState<Record<number, { off: number; on: number }>>({});

  const refresh = () => api.fansState().then(setSt).catch((e) => setErr(String(e)));

  useEffect(() => {
    refresh();
    // Датчики платы меняются медленно — опрос раз в 5 с, чтобы не дёргать PowerShell.
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
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

  if (!st) return <div className="text-ink-3">Загрузка…</div>;

  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Вентиляторы</div>
        <h1 className="text-[24px] mt-1">Вентиляторы платы</h1>
        <p className="text-ink-2 text-[12.5px] mt-1">
          Управление идёт через штатный ACPI-интерфейс платы, а не через кернел-драйвер. Скважность ШИМ считает кривая
          Smart Fan из BIOS — приложение управляет тем, что она получает на вход.
        </p>
      </header>

      {!st.available ? (
        <Section title="Интерфейс недоступен">
          <div className="text-[12.5px] text-ink-2">{st.note}</div>
        </Section>
      ) : (
        <>
          <Section
            title="Датчики платы"
            sub="Те самые, по которым плата решает, как крутить вентиляторы."
            right={<Tag>{st.backend}</Tag>}
          >
            <div className="grid grid-cols-6 gap-3">
              {st.sensors.map((s) => (
                <Tile
                  key={s.id}
                  label={SENSOR_LABEL[s.id] ?? `Датчик ${s.id}`}
                  value={s.connected ? s.celsius : "—"}
                  unit={s.connected ? "°C" : ""}
                  hint={s.connected ? undefined : "разъём пуст"}
                  color={!s.connected ? undefined : s.celsius > 80 ? "var(--color-coral)" : s.celsius > 65 ? "var(--color-amber)" : "var(--color-mint)"}
                />
              ))}
            </div>
          </Section>

          <Section
            title="Вентиляторы"
            sub="Остановка на низких температурах — самый заметный способ сделать простой тише. Пороги обязательны: без них вентилятор может остаться стоять под нагрузкой."
            right={
              <button className="btn" disabled={busy} onClick={() => run(() => api.fansRestore(), "Вентиляторы возвращены под управление BIOS.")}>
                Вернуть под BIOS
              </button>
            }
          >
            <div className="flex flex-col gap-2">
              {st.fans.map((f) => {
                const d = draft[f.id] ?? { off: f.off_limit_c || 40, on: f.on_limit_c || 50 };
                const sensor = st.sensors.find((s) => s.id === f.target_sensor);
                return (
                  <div key={f.id} className="panel-2 px-3.5 py-3">
                    <div className="flex items-center justify-between gap-3">
                      <div className="flex items-center gap-2.5 min-w-0">
                        <StatusPill ok={!f.stop_enabled} warn={f.stop_enabled} text={f.stop_enabled ? "может стоять" : "всегда крутится"} />
                        <div className="min-w-0">
                          <div className="text-[13px]">Вентилятор {f.id}</div>
                          <div className="text-[11.5px] text-ink-2">
                            ведётся по: {SENSOR_LABEL[f.target_sensor] ?? `датчик ${f.target_sensor}`}
                            {sensor?.connected && ` · сейчас ${sensor.celsius} °C`}
                          </div>
                        </div>
                      </div>
                      <div className="flex items-center gap-2 shrink-0">
                        <span className="text-[11.5px] text-ink-2">останов</span>
                        <Switch
                          on={f.stop_enabled}
                          disabled={busy}
                          onChange={(v) => run(() => api.fansSetZero(f.id, v))}
                        />
                      </div>
                    </div>

                    <div className="flex items-end gap-2 mt-2.5">
                      <div>
                        <div className="eyebrow">Стоит ниже</div>
                        <input
                          className="input num w-24 mt-1"
                          type="number"
                          min={0}
                          max={55}
                          value={d.off}
                          disabled={busy}
                          onChange={(e) => setDraft({ ...draft, [f.id]: { ...d, off: Number(e.target.value) } })}
                        />
                      </div>
                      <div>
                        <div className="eyebrow">Крутится с</div>
                        <input
                          className="input num w-24 mt-1"
                          type="number"
                          min={0}
                          max={70}
                          value={d.on}
                          disabled={busy}
                          onChange={(e) => setDraft({ ...draft, [f.id]: { ...d, on: Number(e.target.value) } })}
                        />
                      </div>
                      <button
                        className="btn"
                        disabled={busy}
                        onClick={() =>
                          run(async () => {
                            const [off, on] = await api.fansSetLimits(f.id, d.off, d.on);
                            setDraft({ ...draft, [f.id]: { off, on } });
                            if (off !== d.off || on !== d.on) {
                              setMsg(`Пороги приведены к безопасным: ${off} / ${on} °C.`);
                            }
                          }, `Вентилятор ${f.id}: пороги записаны.`)
                        }
                      >
                        Записать пороги
                      </button>
                      <select
                        className="input w-40"
                        value={f.target_sensor}
                        disabled={busy}
                        onChange={(e) => run(() => api.fansSetSensor(f.id, Number(e.target.value)))}
                      >
                        {st.sensors.map((s) => (
                          <option key={s.id} value={s.id}>
                            {SENSOR_LABEL[s.id] ?? `Датчик ${s.id}`}
                          </option>
                        ))}
                      </select>
                      <button className="btn ml-auto" disabled={busy} onClick={() => run(() => api.fansForceOn(f.id), `Вентилятор ${f.id} раскручен.`)}>
                        Раскрутить
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>

            {msg && <div className="text-[12.5px] text-ink-2 mt-2">{msg}</div>}
            {err && <div className="text-[12.5px] text-coral mt-2">{err}</div>}

            <div className="mt-3">
              <Label
                title="Почему здесь нет процента оборотов"
                text="ACPI-интерфейс платы не даёт задать скважность ШИМ напрямую: её считает кривая Smart Fan из BIOS. Приложение управляет входными данными этой кривой — порогами остановки и тем, за каким датчиком следит вентилятор."
                rec="Саму форму кривой задавайте в BIOS один раз; здесь удобно менять поведение на ходу, не перезагружаясь."
                warn="Пороги обрезаются в коде: остановка не разрешается выше 55 °C, к 70 °C вентилятор обязан вращаться."
              >
                <span className="text-[12px] text-ink-2">Как это работает</span>
              </Label>
            </div>
          </Section>
        </>
      )}
    </div>
  );
}
