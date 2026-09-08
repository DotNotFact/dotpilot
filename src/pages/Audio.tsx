import { useEffect, useState } from "react";
import { useStore } from "../store";
import { api, type AudioInfo, type AudioDevice } from "../lib/api";
import { Section, Tag, Tip, StatusPill } from "../components/ui";
import { Headphones, Mic, RefreshCw, ExternalLink, Bluetooth, Ban, Check, Sparkles, Volume2, RotateCcw } from "lucide-react";

const presets: { id: string; name: string; what: string; when: string }[] = [
  { id: "footsteps", name: "Шаги (FPS)", what: "Срезает бас до 250 Гц на 5–9 дБ и поднимает 2–5 кГц на 4–5,5 дБ: именно там живут шаги, перезарядка и шорох. Преамп −6 дБ, чтобы не было клиппинга.", when: "Warface, любой шутер. Музыка в этом режиме будет звучать плоско и резко — это нормально." },
  { id: "balanced", name: "Игра, мягко", what: "Тот же принцип, но вдвое слабее: бас −3…−5 дБ, презенс +2…+3 дБ.", when: "Если «Шаги» режет уши или в Minecraft хочется сохранить атмосферу." },
  { id: "voice", name: "Голос (Discord)", what: "Подъём 1–3 кГц, срез ниже 150 Гц и выше 8 кГц: разборчивость речи без гула.", when: "Созвоны и работа." },
  { id: "music", name: "Музыка", what: "Лёгкая V-кривая: чуть больше баса и воздуха, середина без изменений.", when: "Музыка, фильмы." },
  { id: "off", name: "Выключить", what: "DotPilot убирает свои строки из Equalizer APO. Остальные ваши настройки APO не трогаются.", when: "Проверить, как звучит без обработки." },
];

export default function Audio() {
  const toast = useStore((s) => s.toast);
  const setPage = useStore((s) => s.setPage);
  const snap = useStore((s) => s.snap);
  const cfg = useStore((s) => s.cfg);
  const [info, setInfo] = useState<AudioInfo | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string[]>([]);
  const [customProducts, setCustomProducts] = useState<string[]>([]);
  // Preselect detected Bluetooth products that match the configured patterns (EDIFIER, Redmi Buds …).
  useEffect(() => {
    if (!cfg || !info || selected.length) return;
    const detected = [...new Set(info.devices.filter((d) => d.flow === "playback" && d.bluetooth).map((d) => d.product))];
    const pre = detected.filter((p) => cfg.headphone_products.some((h) => p.toLowerCase().includes(h.toLowerCase())));
    setSelected(pre.length ? pre : detected);
  }, [cfg, info]);

  const load = async () => {
    try {
      const i = await api.audioInfo();
      setInfo(i);
      if (i.eq.device_filter && !filter) setFilter(i.eq.device_filter);
    } catch (e) {
      toast("error", String(e));
    }
  };
  useEffect(() => {
    load();
  }, []);

  const run = async (key: string, fn: () => Promise<string>, okMsg?: string) => {
    setBusy(key);
    try {
      const r = await fn();
      toast("success", okMsg ?? r);
      await load();
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(null);
    }
  };

  const playback = info?.devices.filter((d) => d.flow === "playback") ?? [];
  const capture = info?.devices.filter((d) => d.flow === "capture") ?? [];
  const products = [...new Set(playback.filter((d) => d.bluetooth).map((d) => d.product))];
  const activeHf = playback.filter((d) => d.hands_free && d.state !== "disabled" && d.state !== "notpresent");
  const defaultDev = playback.find((d) => d.is_default);

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between gap-4">
        <div>
          <div className="eyebrow">Звук</div>
          <h1 className="text-[24px] mt-1 flex items-center gap-2">
            <Headphones size={22} className="text-teal" /> Наушники и шаги в игре
          </h1>
          <p className="text-ink-2 text-[12.5px] mt-1 max-w-[820px] leading-snug">
            Почему в Edifier Stax Spirit S3 шаги слышно хуже, чем в Redmi Buds 6 Pro: у Bluetooth-наушников в Windows два профиля. «Наушники» — стерео A2DP, качество нормальное. «Головной телефон / Hands-Free» — телефонный профиль HFP: моно, 8–16 кГц, без верхних частот. Как только Discord или игра берёт микрофон с гарнитуры, Windows переключает всё на HFP, и шаги исчезают. Плюс у планарных S3 ровная АЧХ с сильным басом, который маскирует 2–5 кГц, где и слышны шаги.
          </p>
        </div>
        <div className="flex gap-2">
          <button className="btn" onClick={load}>
            <RefreshCw size={14} /> Обновить
          </button>
          <Tip title="Параметры → Звук" text="Открывает системные настройки звука: выбор устройства, пространственный звук, микшер громкости приложений." side="right">
            <button className="btn" onClick={() => api.openSound("settings")}>
              <ExternalLink size={14} /> Параметры звука
            </button>
          </Tip>
          <Tip title="Классическая панель (mmsys.cpl)" text="Здесь у каждого устройства есть вкладки «Улучшения» (выравнивание громкости, пространственный звук) и «Дополнительно» (частота дискретизации, монопольный режим)." rec="Для наушников: 24 бит / 48000 Гц, снять «Разрешить приложениям монопольный режим», если игра перехватывает звук." side="right">
            <button className="btn" onClick={() => api.openSound("classic")}>
              <Volume2 size={14} /> Панель устройств
            </button>
          </Tip>
        </div>
      </header>

      {!info && <div className="text-ink-3 text-[12.5px]">Читаю аудиоустройства…</div>}

      {info && (
        <>
          <div className="panel p-4 flex items-start gap-4" style={{ borderColor: "rgba(53,208,198,.45)" }}>
            <div className="flex-1">
              <div className="text-[15px] font-semibold flex items-center gap-2">
                <Sparkles size={16} className="text-teal" /> Настроить шаги одной кнопкой
              </div>
              <div className="text-[12.5px] text-ink-2 mt-1 leading-snug">
                Для выбранных наушников DotPilot: 1) отключит телефонный профиль Hands-Free, чтобы звук всегда оставался в стерео; 2) подключит Equalizer APO к их выходу (как это делает Configurator, с резервной копией настроек устройства); 3) запишет пресет «Шаги (FPS)» только для этих устройств; 4) перезапустит службу Windows Audio (2–3 секунды тишины).
              </div>
              <div className="flex flex-wrap gap-2 mt-2">
                {[...new Set([...products, ...customProducts])].map((p) => (
                  <label key={p} className="flex items-center gap-1.5 text-[12.5px] cursor-pointer">
                    <input type="checkbox" checked={selected.includes(p)} onChange={(e) => setSelected(e.target.checked ? [...selected, p] : selected.filter((x) => x !== p))} />
                    {p}
                  </label>
                ))}
                <input className="input !w-44 !py-0.5 text-[12px]" placeholder="добавить (часть названия)" onKeyDown={(e) => { if (e.key === "Enter" && e.currentTarget.value.trim()) { const v = e.currentTarget.value.trim(); setCustomProducts([...customProducts, v]); setSelected([...selected, v]); e.currentTarget.value = ""; } }} />
              </div>
              {!snap?.admin && <div className="text-[11.5px] text-amber mt-2">Нужны права администратора: страница «Доступ» → «Перезапустить от администратора».</div>}
            </div>
            <div className="flex flex-col gap-2">
              <Tip title="Применить" text="Все четыре шага выполняются подряд, каждый пишется в журнал. Если что-то не понравится — «Сбросить звук» вернёт всё как было." side="right">
                <button className="btn btn-primary" disabled={busy !== null || !selected.length || !snap?.admin} onClick={() => run("fix", async () => { const lines = await api.soundFixFootsteps(selected, "footsteps"); return lines.join("\n"); })}>
                  <Sparkles size={14} /> Настроить шаги
                </button>
              </Tip>
              <Tip title="Сбросить звук к исходному" text="Выключает пресет DotPilot, отключает Equalizer APO от устройств, где его подключил DotPilot (восстанавливает резервную копию), включает обратно Hands-Free и перезапускает Windows Audio. Настройки HeSuVi и других программ не трогаются." side="right">
                <button className="btn" disabled={busy !== null || !snap?.admin} onClick={() => run("reset", async () => (await api.soundReset()).join("\n"))}>
                  <RotateCcw size={14} /> Сбросить звук
                </button>
              </Tip>
            </div>
          </div>
          {activeHf.length > 0 && (
            <div className="panel p-3 text-[12.5px] border-amber/50 flex items-start gap-3">
              <span className="text-amber mt-0.5">
                <Ban size={16} />
              </span>
              <div className="flex-1">
                <div className="text-amber font-medium">Найдены включённые телефонные профили (Hands-Free): {activeHf.map((d) => d.product).join(", ")}</div>
                <div className="text-ink-2 mt-0.5 leading-snug">Пока они включены, Windows может в любой момент переключить наушники в моно-режим HFP. У тебя есть отдельный USB-микрофон fifine, поэтому телефонный профиль гарнитур не нужен: выключи его, и наушники всегда останутся в стерео A2DP.</div>
              </div>
              <Tip title="Отключить Hands-Free у всех гарнитур" text="Выполняет Disable-PnpDevice для каждого endpoint «Головной телефон». Микрофон наушников перестанет быть доступен приложениям, звук останется в стерео." rec="Обратимо: кнопка «Включить» у устройства ниже." side="right">
                <button className="btn btn-primary" disabled={busy !== null} onClick={() => run("hf", async () => { for (const d of activeHf) await api.audioSetEnabled(d.instance_id, false); return "Телефонные профили отключены"; })}>
                  <Ban size={14} /> Отключить все Hands-Free
                </button>
              </Tip>
            </div>
          )}

          <div className="grid grid-cols-[1.4fr_1fr] gap-4 items-start">
            <Section title="Устройства вывода" sub={info.module_installed ? `По умолчанию: ${defaultDev?.name ?? "неизвестно"}` : "Чтобы переключать устройство по умолчанию, установи модуль AudioDeviceCmdlets на странице «Доступ»."} right={!info.module_installed ? <button className="btn btn-sm" onClick={() => setPage("access")}>Открыть «Доступ»</button> : undefined}>
              <div className="flex flex-col gap-2">
                {playback.map((d) => (
                  <DeviceRow key={d.id} d={d} busy={busy} moduleInstalled={info.module_installed} onDefault={(role) => run(d.id + role, () => api.audioSetDefault(d.id, role), role === "comm" ? `${d.name}: устройство связи по умолчанию` : `${d.name}: устройство вывода по умолчанию`)} onToggle={(on) => run(d.id + "t", () => api.audioSetEnabled(d.instance_id, on), `: `)} onApo={(attach) => run(d.id + "apo", () => api.apoToggle(d.id, attach))} />
                ))}
              </div>
              <div className="eyebrow mt-4 mb-2">Микрофоны</div>
              <div className="flex flex-col gap-2">
                {capture.map((d) => (
                  <DeviceRow key={d.id} d={d} busy={busy} moduleInstalled={info.module_installed} onDefault={(role) => run(d.id + role, () => api.audioSetDefault(d.id, role), `${d.name}: микрофон по умолчанию`)} onToggle={(on) => run(d.id + "t", () => api.audioSetEnabled(d.instance_id, on), `: `)} onApo={(attach) => run(d.id + "apo", () => api.apoToggle(d.id, attach))} />
                ))}
              </div>
            </Section>

            <div className="flex flex-col gap-4">
              <Section title="Эквалайзер (Equalizer APO)" sub={info.eq.installed ? `Пресет сейчас: ${presets.find((p) => p.id === info.eq.preset)?.name ?? info.eq.preset}${info.eq.device_filter ? ` · только для «${info.eq.device_filter}»` : " · для всех устройств"}` : "Equalizer APO не установлен"}>
                {!info.eq.installed ? (
                  <button className="btn" onClick={() => api.openSound("apo-download")}>
                    <ExternalLink size={14} /> Скачать Equalizer APO
                  </button>
                ) : (
                  <div className="flex flex-col gap-2">
                    <label className="flex flex-col gap-1">
                      <Tip title="Для какого устройства" text="Equalizer APO применяет блок настроек только к устройствам, в названии которых есть эта строка. Пусто = ко всем устройствам, где APO установлен." rec="Впиши «EDIFIER», чтобы пресет «Шаги» не портил звук на колонках." mark>
                        <span className="eyebrow">Фильтр устройства</span>
                      </Tip>
                      <div className="flex gap-1">
                        <input className="input" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="все устройства" list="products" />
                        <datalist id="products">
                          {products.map((p) => (
                            <option key={p} value={p} />
                          ))}
                        </datalist>
                      </div>
                    </label>
                    <div className="grid grid-cols-1 gap-1.5">
                      {presets.map((p) => (
                        <Tip key={p.id} title={p.name} text={p.what} rec={p.when} className="w-full">
                          <button className={`btn w-full justify-between ${info.eq.preset === p.id ? "btn-primary" : ""}`} disabled={busy !== null} onClick={() => run("eq" + p.id, () => api.eqApply(p.id, filter))}>
                            <span>{p.name}</span>
                            {info.eq.preset === p.id && <Check size={13} />}
                          </button>
                        </Tip>
                      ))}
                    </div>
                    <div className="text-[11.5px] text-ink-3 leading-snug mt-1">
                      Не слышно разницы? Открой Configurator и отметь галочкой наушники: APO подключается к каждому устройству отдельно.{" "}
                      <button className="text-teal hover:underline cursor-pointer" onClick={() => api.openSound("apo")}>
                        Открыть Configurator
                      </button>
                    </div>
                  </div>
                )}
              </Section>

              <Section title="Что ещё улучшит шаги" sub="Проверено на практике, по убыванию эффекта">
                <ol className="text-[12.5px] text-ink-2 list-decimal ml-5 leading-relaxed flex flex-col gap-1">
                  <li>
                    <b className="text-ink">Провод для Stax Spirit S3.</b> У S3 есть USB-C вход: по кабелю это USB-звуковая карта без Bluetooth-кодека, задержки и сжатия. Для шутера это лучший вариант.
                  </li>
                  <li>
                    <b className="text-ink">Выключить Hands-Free</b> (кнопка выше) и в Discord выбрать микрофон fifine, а не гарнитуру.
                  </li>
                  <li>
                    <b className="text-ink">Game mode в приложении Edifier</b> снижает задержку, но переключает кодек на упрощённый: детали пропадают. Со «Шагами» в APO лучше держать Game mode выключенным, если задержка не мешает.
                  </li>
                  <li>
                    <b className="text-ink">Пространственный звук:</b> Windows Sonic для наушников часто помогает понять направление, но размывает дальние шаги. Пробуй вкл/выкл в «Параметрах звука».
                  </li>
                  <li>
                    <b className="text-ink">В игре:</b> звук «наушники/стерео», без «динамик» и без встроенных пресетов «музыка»; голос союзников тише, эффекты громче.
                  </li>
                </ol>
              </Section>
            </div>
          </div>
        </>
      )}
      <div className="text-[11.5px] text-ink-3 flex items-center gap-1.5">
        <Sparkles size={12} /> Раздел учитывает твои устройства: Edifier Stax Spirit S3, Redmi Buds 6 Pro, микрофон fifine, NVIDIA Broadcast.
      </div>
    </div>
  );
}

function DeviceRow({ d, busy, moduleInstalled, onDefault, onToggle, onApo }: { d: AudioDevice; busy: string | null; moduleInstalled: boolean; onDefault: (role: "playback" | "comm") => void; onToggle: (on: boolean) => void; onApo: (attach: boolean) => void }) {
  const stateText: Record<string, string> = { active: "активно", disabled: "отключено", unplugged: "не подключено", notpresent: "нет", unknown: "?" };
  const off = d.state === "disabled";
  return (
    <div className={`panel-2 px-3 py-2 flex items-center gap-3 ${off ? "opacity-60" : ""}`}>
      <span className={d.hands_free ? "text-amber" : d.flow === "capture" ? "text-blue" : "text-teal"}>{d.flow === "capture" ? <Mic size={15} /> : d.bluetooth ? <Bluetooth size={15} /> : <Headphones size={15} />}</span>
      <div className="flex-1 min-w-0">
        <div className="text-[13px] flex items-center gap-2 flex-wrap">
          <span className="truncate">{d.name}</span>
          {d.is_default && <Tag color="var(--color-mint)">по умолчанию</Tag>}
          {d.is_default_comm && <Tag color="var(--color-blue)">связь</Tag>}
          {d.hands_free && (
            <Tip title="Телефонный профиль (HFP)" text="Моно, 8–16 кГц, используется, когда приложение берёт микрофон гарнитуры. В этом режиме шаги в игре не слышны." rec="Отключить, если есть отдельный микрофон.">
              <Tag color="var(--color-amber)">hands-free · моно</Tag>
            </Tip>
          )}
          {d.apo && (
            <Tip title="Equalizer APO подключён" text="Пресеты эквалайзера действуют на это устройство. Резервная копия настроек устройства сохранена в реестре DotPilot.">
              <Tag color="var(--color-teal)">APO</Tag>
            </Tip>
          )}
          {d.enhancements_disabled && (
            <Tip title="Улучшения звука выключены" text="У устройства стоит «Отключить все улучшения»: Equalizer APO и любой эквалайзер не работают. Кнопка «Настроить шаги» включает улучшения обратно.">
              <Tag color="var(--color-coral)">улучшения выкл</Tag>
            </Tip>
          )}
        </div>
        <div className="text-[11px] text-ink-3">
          <StatusPill ok={d.state === "active"} warn={d.state === "unplugged"} text={stateText[d.state] ?? d.state} />
        </div>
      </div>
      <div className="flex gap-1">
        {moduleInstalled && d.state === "active" && !d.is_default && (
          <Tip title="Сделать устройством по умолчанию" text="Весь звук Windows и игр пойдёт сюда. Приложения со своим выбором устройства (Discord) не затрагиваются." side="right">
            <button className="btn btn-sm" disabled={busy !== null} onClick={() => onDefault("playback")}>
              По умолчанию
            </button>
          </Tip>
        )}
        {moduleInstalled && d.state === "active" && !d.is_default_comm && d.flow === "playback" && (
          <Tip title="Устройство связи" text="Куда Windows направляет звонки и голосовые чаты, если приложение следует системной настройке «связь»." side="right">
            <button className="btn btn-sm" disabled={busy !== null} onClick={() => onDefault("comm")}>
              Связь
            </button>
          </Tip>
        )}
        {d.flow === "playback" && !d.hands_free && d.state !== "notpresent" && (
          <Tip title={d.apo ? "Отключить Equalizer APO" : "Подключить Equalizer APO"} text={d.apo ? "Вернуть устройству исходные аудиоэффекты Windows из резервной копии." : "Зарегистрировать Equalizer APO на этом устройстве (то же, что галочка в Configurator). Перезапускает Windows Audio."} side="right">
            <button className="btn btn-sm" disabled={busy !== null} onClick={() => onApo(!d.apo)}>
              {d.apo ? "APO выкл" : "APO вкл"}
            </button>
          </Tip>
        )}
        <Tip title={off ? "Включить устройство" : "Отключить устройство"} text={off ? "Устройство снова появится в списке Windows." : "Windows перестанет видеть это устройство, пока его не включить обратно здесь или в Диспетчере устройств."} side="right">
          <button className={`btn btn-sm ${off ? "" : "btn-danger"}`} disabled={busy !== null || d.state === "notpresent"} onClick={() => onToggle(off)}>
            {off ? "Включить" : "Отключить"}
          </button>
        </Tip>
      </div>
    </div>
  );
}
