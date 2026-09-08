import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { api, fmtBps, type AppEntry, type Connection, type Policy, type Tweak } from "../lib/api";
import { Section, Switch, Tag, StatusPill, Tip, Label } from "../components/ui";
import { Play, Square, ExternalLink, Plus, Trash2, RefreshCw, ChevronDown, ChevronUp, Wifi, Shield, Gamepad2, Zap, FolderOpen } from "lucide-react";

const policyLabel: Record<Policy, string> = { direct: "Напрямую (Wi-Fi)", vpn: "Через Happ VPN", radmin: "Radmin в приоритете" };
const policyColor: Record<Policy, string> = { direct: "#35d0c6", vpn: "#9085e9", radmin: "#f2b33d" };

export default function Network() {
  const snap = useStore((s) => s.snap);
  const cfg = useStore((s) => s.cfg);
  const saveConfig = useStore((s) => s.saveConfig);
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  const [draft, setDraft] = useState<AppEntry[] | null>(null);
  const [dirty, setDirty] = useState(false);
  const [applying, setApplying] = useState(false);
  const [report, setReport] = useState<string[] | null>(null);
  const [tweaks, setTweaks] = useState<Tweak[] | null>(null);

  useEffect(() => {
    if (cfg && !draft) setDraft(cfg.apps.map((a) => ({ ...a, exe_paths: [...a.exe_paths] })));
  }, [cfg]);
  useEffect(() => {
    api.tweaks().then(setTweaks).catch(() => setTweaks([]));
  }, []);

  if (!snap || !cfg || !draft) return <div className="text-ink-3">Загрузка…</div>;

  const update = (id: string, patch: Partial<AppEntry>) => {
    setDraft(draft.map((a) => (a.id === id ? { ...a, ...patch } : a)));
    setDirty(true);
  };
  const remove = (id: string) => {
    setDraft(draft.filter((a) => a.id !== id));
    setDirty(true);
  };
  const addApp = async () => {
    const file = await open({ multiple: false, filters: [{ name: "Программы", extensions: ["exe"] }] });
    if (!file) return;
    const p = String(file);
    const name = p.split("\\").pop()?.replace(/\.exe$/i, "") ?? "Приложение";
    const id = name.toLowerCase().replace(/[^a-z0-9]+/g, "-") + "-" + Date.now().toString(36);
    setDraft([...draft, { id, name, exe_paths: [p], kind: "tool", policy: "direct", dscp: null, throttle_mbps: null, background: false, color: "#3987e5", note: "" }]);
    setDirty(true);
  };
  const addPath = async (id: string) => {
    const file = await open({ multiple: false, filters: [{ name: "Программы", extensions: ["exe"] }] });
    if (!file) return;
    const a = draft.find((x) => x.id === id)!;
    update(id, { exe_paths: [...a.exe_paths, String(file)] });
  };
  const save = async () => {
    try {
      await saveConfig({ ...cfg, apps: draft });
      setDirty(false);
      toast("success", "Политики сохранены. Нажми «Применить», чтобы создать QoS-правила.");
    } catch (e) {
      toast("error", String(e));
    }
  };
  const apply = async () => {
    try {
      setApplying(true);
      if (dirty) await saveConfig({ ...cfg, apps: draft });
      setDirty(false);
      const r = await api.applyPolicies();
      setReport(r.lines);
      toast(r.ok ? "success" : "error", r.ok ? "Политики применены" : "Часть политик не применилась, смотри отчёт");
      refresh();
    } catch (e) {
      toast("error", String(e));
    } finally {
      setApplying(false);
    }
  };

  const gm = snap.game_mode;

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between gap-4">
        <div>
          <div className="eyebrow">Сеть</div>
          <h1 className="text-[24px] mt-1">Кто и как выходит в сеть</h1>
          <p className="text-ink-2 text-[12.5px] mt-1 max-w-[760px] leading-snug">
            Для каждого приложения выбирается канал и приоритет. «Напрямую» — трафик идёт по Wi-Fi мимо VPN; «Через Happ» — приложение попадает в список туннеля Happ; «Radmin в приоритете» — сеть друзей 26.x.x.x через Radmin, остальное напрямую. Приоритет (DSCP 46) переводит пакеты игры в голосовую очередь Wi-Fi роутера, а лимиты фоновых приложений включаются автоматически, когда запущена игра.
          </p>
        </div>
        <div className="flex gap-2">
          <Tip title="Убрать все QoS-правила" text="Удаляет правила DotPilot-* из Windows и выключает игровой режим. Политики в списке остаются, их можно применить снова." side="right">
            <button className="btn" onClick={() => api.removeQos().then(() => { toast("success", "QoS-правила удалены"); refresh(); }).catch((e) => toast("error", String(e)))}>
              Снять QoS
            </button>
          </Tip>
          <button className="btn" onClick={addApp}>
            <Plus size={14} /> Добавить приложение
          </button>
          <button className="btn" disabled={!dirty} onClick={save}>
            Сохранить
          </button>
          <Tip title="Применить политики" text="Сохраняет настройки и пересоздаёт QoS-правила Windows (DotPilot-*): DSCP-приоритеты и лимиты. Проверяет метрику Radmin. Если включена синхронизация с Happ — обновляет список приложений туннеля." rec="Нажимай после любого изменения каналов, приоритетов или лимитов." side="right">
            <button className="btn btn-primary" disabled={applying} onClick={apply}>
              <Zap size={14} /> {applying ? "Применяю…" : "Применить политики"}
            </button>
          </Tip>
        </div>
      </header>

      {!snap.admin && (
        <div className="panel p-3 text-[12.5px] border-coral/50 text-coral">Приложение запущено без прав администратора: QoS, метрики интерфейсов и службы менять нельзя. Перезапусти DotPilot от имени администратора.</div>
      )}

      {report && (
        <div className="panel p-3 text-[12px]">
          <div className="flex justify-between mb-1">
            <span className="eyebrow">Отчёт применения</span>
            <button className="text-ink-3 hover:text-ink cursor-pointer" onClick={() => setReport(null)}>
              скрыть
            </button>
          </div>
          <pre className="num whitespace-pre-wrap selectable text-ink-2">{report.join("\n")}</pre>
        </div>
      )}

      <div className="grid grid-cols-[1fr_340px] gap-4 items-start">
        <div className="flex flex-col gap-3">
          {draft.map((a) => (
            <AppCard key={a.id} app={a} status={snap.apps.find((s) => s.id === a.id)} onChange={(p) => update(a.id, p)} onRemove={() => remove(a.id)} onAddPath={() => addPath(a.id)} happTun={snap.happ.tun_enabled} />
          ))}
        </div>

        <div className="flex flex-col gap-4">
          <Section title="Игровой режим" sub={gm.trigger}>
            <div className="flex items-center justify-between mb-3">
              <span className="text-[13px]">{gm.active ? "Активен: фоновые лимиты включены" : "Неактивен"}</span>
              <Tip title="Игровой режим вручную" text="Включает лимиты фоновых приложений и приоритет игр прямо сейчас, независимо от того, запущена ли игра. Ручной выбор перекрывает автоматику до кнопки «Вернуть автоматический режим»." side="right">
                <Switch on={gm.active} onChange={(v) => api.setGameMode(v).then(refresh)} />
              </Tip>
            </div>
            <div className="flex items-center justify-between text-[12.5px] mb-2">
              <span className="text-ink-2">Включать автоматически при запуске игры</span>
              <Tip title="Автоматика" text="DotPilot каждые 2 секунды проверяет процессы. Как только запускается приложение с типом «игра», режим включается; когда игра закрыта — выключается." rec="Оставь включённым: тогда про лимиты можно не вспоминать." side="right">
                <Switch on={cfg.auto_game_mode} onChange={(v) => saveConfig({ ...cfg, auto_game_mode: v }).then(() => api.setGameMode(null)).then(refresh)} />
              </Tip>
            </div>
            {gm.manual !== null && (
              <button className="btn btn-sm w-full justify-center" onClick={() => api.setGameMode(null).then(refresh)}>
                Вернуть автоматический режим
              </button>
            )}
            <div className="text-[11.5px] text-ink-3 mt-2 leading-snug">Игровой режим создаёт QoS-правила ограничения скорости для приложений с пометкой «фоновое» (Claude Code, Docker). Правила исчезают, когда игра закрыта.</div>
          </Section>

          <Section title="Службы и туннели">
            <div className="flex flex-col gap-3">
              {snap.services.map((s) => (
                <div key={s.id} className="panel-2 p-3">
                  <div className="flex items-center justify-between">
                    <StatusPill ok={s.service_state === "Running" || s.gui_running} warn={s.mode === "idle"} text={s.name} />
                    <span className="num text-[11px] text-ink-3">
                      {s.service} · {s.service_state === "Missing" ? "нет службы" : s.service_state}
                    </span>
                  </div>
                  <div className="text-[11.5px] text-ink-2 mt-1 leading-snug">{s.detail}</div>
                  {s.id === "happ" && s.proxied_apps.length > 0 && (
                    <div className="text-[11px] text-ink-3 mt-1">
                      В туннеле Happ: {s.proxied_apps.map((p) => p.split("\\").pop()).join(", ")}
                    </div>
                  )}
                  <div className="flex gap-1.5 mt-2 flex-wrap">
                    {s.service_state !== "Missing" && (
                      <>
                        <Tip title={`Запустить службу ${s.service}`} text={s.id === "happ" ? "Поднимает фоновый демон Happ (happd). Само подключение VPN включается в окне Happ." : s.id === "zapret" ? "Запускает winws.exe: обход DPI снова работает для YouTube, Discord и игровых портов." : "Запускает службу Radmin VPN: адаптер 26.x поднимается, друзья видят тебя в сети."}>
                          <button className="btn btn-sm" disabled={s.service_state === "Running"} onClick={() => api.serviceControl(s.id, "start").then((r) => toast("success", `${s.name}: ${r}`)).catch((e) => toast("error", String(e))).finally(refresh)}>
                            <Play size={12} /> Служба
                          </button>
                        </Tip>
                        <Tip title={`Остановить ${s.service}`} text={s.id === "happ" ? "Останавливает демон и ядро xray: VPN и системный прокси перестают работать. Полезно, если Happ мешает игре." : s.id === "zapret" ? "Останавливает winws. Проверь так, не даёт ли zapret потери в игре: он обрабатывает и игровые UDP-порты." : "Останавливает Radmin: сеть 26.x пропадает. Иногда снижает пинг, если Radmin перехватывал широковещательный трафик."} warn={s.id === "happ" ? "Claude Code и браузер могут потерять доступ к заблокированным сайтам." : undefined}>
                          <button className="btn btn-sm btn-danger" disabled={s.service_state !== "Running"} onClick={() => api.serviceControl(s.id, "stop").then((r) => toast("success", `${s.name}: ${r}`)).catch((e) => toast("error", String(e))).finally(refresh)}>
                            <Square size={12} /> Стоп
                          </button>
                        </Tip>
                      </>
                    )}
                    {s.id !== "zapret" && (
                      <button className="btn btn-sm" onClick={() => api.serviceControl(s.id, "launch").then(() => toast("info", `${s.name}: окно запущено`)).catch((e) => toast("error", String(e)))}>
                        <ExternalLink size={12} /> Открыть
                      </button>
                    )}
                    {s.id === "zapret" && s.service_state === "Missing" && (
                      <button className="btn btn-sm" onClick={() => api.serviceControl(s.id, s.gui_running ? "stop" : "start").then((r) => toast("success", r)).catch((e) => toast("error", String(e))).finally(refresh)}>
                        {s.gui_running ? <Square size={12} /> : <Play size={12} />} {s.gui_running ? "Остановить winws" : "Запустить general.bat"}
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </Section>

          <ProxyCard />

          <TelegramCard />

          <Section title="Быстрые действия">
            <div className="grid grid-cols-2 gap-1.5">
              {[
                ["flushdns", "Очистить DNS", "Сбрасывает кэш DNS. Помогает, когда сайт «не открывается» после смены VPN или zapret, а у других всё работает. Безопасно, мгновенно."],
                ["arp", "Очистить ARP", "Забывает MAC-адреса соседей по сети. Нужно, если после смены роутера или Radmin-сессии пакеты уходят «в никуда»."],
                ["renew", "Обновить DHCP", "Отпускает и заново получает IP-адрес от роутера. Связь пропадёт на 2–5 секунд."],
                ["wifi-reconnect", "Переподключить Wi-Fi", "Отключает и подключает Wi-Fi заново: заново выбирается канал и диапазон. Полезно при внезапно выросшем пинге до роутера."],
                ["winsock", "Сброс Winsock", "Сбрасывает сетевой стек приложений (LSP). Лечит странные ошибки после VPN-клиентов и антивирусов. Нужна перезагрузка."],
                ["ipreset", "Сброс TCP/IP", "Полный сброс настроек TCP/IP до заводских. Сотрёт вручную заданные IP/DNS. Только если ничего другое не помогло. Нужна перезагрузка."],
              ].map(([id, label, hint]) => (
                <Tip key={id} title={label} text={hint} className="w-full">
                  <button className="btn btn-sm justify-center w-full" onClick={() => api.quickAction(id).then((r) => toast("success", r)).catch((e) => toast("error", String(e)))}>
                    {label}
                  </button>
                </Tip>
              ))}
            </div>
          </Section>
        </div>
      </div>

      <Section title="Адаптеры и метрики" sub="Меньше метрика — выше приоритет маршрута. Wi-Fi должен быть ниже виртуальных, Radmin — самый низкий только для сети 26.x.">
        <table className="w-full text-[12.5px]">
          <thead>
            <tr className="text-left text-ink-3 text-[11px] uppercase tracking-wider">
              <th className="py-1.5 font-semibold">Адаптер</th>
              <th className="font-semibold">Роль</th>
              <th className="font-semibold">Статус</th>
              <th className="font-semibold">IPv4</th>
              <th className="font-semibold">Скорость</th>
              <th className="font-semibold">Метрика</th>
              <th className="font-semibold">Трафик</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {snap.adapters
              .filter((a) => a.status !== "Not Present")
              .map((a) => (
                <AdapterRow key={a.name} a={a} onMetric={(m) => api.setInterfaceMetric(a.if_index, m).then(() => toast("success", `Метрика «${a.name}» обновлена`)).catch((e) => toast("error", String(e)))} onToggle={(on) => api.setAdapterEnabled(a.name, on).then(() => toast("success", `«${a.name}» ${on ? "включён" : "выключен"}`)).catch((e) => toast("error", String(e)))} />
              ))}
          </tbody>
        </table>
      </Section>

      <Section
        title="Твики Windows для низкой задержки"
        sub="Системные настройки, влияющие на пинг. Часть требует перезагрузки."
        right={
          <div className="flex gap-1.5">
            <Tip title="Вернуть значения Windows" text="Все шесть настроек возвращаются к заводским: троттлинг 10, MMCSS 20%, Nagle включён, autotuning normal, ECN выкл, DSCP-ключ удалён." side="right">
              <button className="btn btn-sm" onClick={() => api.tweaksReset().then((l) => { toast("success", l.join("\n")); api.tweaks().then(setTweaks); }).catch((e) => toast("error", String(e)))}>
                По умолчанию
              </button>
            </Tip>
            <button className="btn btn-sm" onClick={() => api.tweaks().then(setTweaks)}>
              <RefreshCw size={12} /> Обновить
            </button>
          </div>
        }
      >
        <div className="grid grid-cols-2 gap-2">
          {(tweaks ?? []).map((t) => (
            <div key={t.id} className="panel-2 p-3 flex items-start gap-3">
              <div className="flex-1">
                <div className="text-[13px] font-medium flex items-center gap-2">
                  {t.name}
                  {t.reboot && <Tag color="var(--color-amber)">перезагрузка</Tag>}
                </div>
                <div className="text-[11.5px] text-ink-2 mt-0.5 leading-snug">{t.description}</div>
              </div>
              <Switch on={!!t.state} onChange={(v) => api.setTweak(t.id, v).then(() => api.tweaks().then(setTweaks)).then(() => toast("success", `${t.name}: ${v ? "включено" : "выключено"}`)).catch((e) => toast("error", String(e)))} />
            </div>
          ))}
          {tweaks === null && <div className="text-ink-3 text-[12px]">Читаю настройки…</div>}
        </div>
      </Section>

      <Section title="Таблица маршрутов" sub="Первые совпадения по длине префикса и метрике. 0.0.0.0/0 — маршрут по умолчанию.">
        <div className="max-h-[260px] overflow-y-auto">
          <table className="w-full text-[12px] num">
            <thead className="sticky top-0 bg-panel">
              <tr className="text-left text-ink-3 text-[10.5px] uppercase tracking-wider">
                <th className="py-1 font-semibold">Назначение</th>
                <th className="font-semibold">Шлюз</th>
                <th className="font-semibold">Интерфейс</th>
                <th className="font-semibold">Метрика</th>
              </tr>
            </thead>
            <tbody>
              {snap.routes
                .filter((r) => !r.prefix.startsWith("224.") && !r.prefix.startsWith("255.") && !r.prefix.startsWith("127."))
                .map((r, i) => (
                  <tr key={i} className="border-t border-line/60">
                    <td className="py-1">{r.prefix}</td>
                    <td className="text-ink-2">{r.next_hop}</td>
                    <td className="text-ink-2">{r.interface}</td>
                    <td className="text-ink-3">
                      {r.route_metric}+{r.interface_metric}
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      </Section>
    </div>
  );
}

function AppCard({ app, status, onChange, onRemove, onAddPath, happTun }: { app: AppEntry; status?: { running: boolean; procs: { pid: number; name: string; cpu: number; mem_mb: number }[]; in_happ_list: boolean; qos_rules: number }; onChange: (p: Partial<AppEntry>) => void; onRemove: () => void; onAddPath: () => void; happTun: boolean }) {
  const [openConn, setOpenConn] = useState(false);
  const [conns, setConns] = useState<Connection[] | null>(null);
  const [loading, setLoading] = useState(false);
  const toast = useStore((s) => s.toast);
  const loadConns = async () => {
    setLoading(true);
    try {
      setConns(await api.connections(app.id));
    } catch (e) {
      toast("error", String(e));
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    if (openConn) loadConns();
  }, [openConn]);

  const kindIcon = app.kind === "game" ? <Gamepad2 size={15} /> : app.kind === "browser" ? <Wifi size={15} /> : <Shield size={15} />;
  const mismatch = app.policy === "vpn" && status && !status.in_happ_list;
  const mismatchDirect = app.policy !== "vpn" && status && status.in_happ_list;

  return (
    <div className="panel p-4" style={{ borderLeft: `3px solid ${app.color}` }}>
      <div className="flex items-start gap-3">
        <div className="mt-0.5" style={{ color: app.color }}>
          {kindIcon}
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <input className="input !w-auto !py-0.5 !px-1.5 font-semibold text-[14px] !bg-transparent !border-transparent hover:!border-line-2" value={app.name} onChange={(e) => onChange({ name: e.target.value })} />
            {status?.running ? <Tag color="var(--color-mint)">запущено · {status.procs.length}</Tag> : <Tag>не запущено</Tag>}
            {status && status.qos_rules > 0 && <Tag color="var(--color-teal)">QoS ×{status.qos_rules}</Tag>}
            {status?.in_happ_list && <Tag color="var(--color-violet)">в списке Happ</Tag>}
            {mismatch && <Tag color="var(--color-amber)">Happ ещё не знает об этом</Tag>}
            {mismatchDirect && <Tag color="var(--color-amber)">Happ всё ещё туннелирует</Tag>}
          </div>
          <div className="text-[11.5px] text-ink-3 mt-1 num flex flex-col gap-0.5">
            {app.exe_paths.map((p, i) => (
              <div key={i} className="flex items-center gap-1.5">
                <FolderOpen size={11} className="flex-none" />
                <span className="truncate flex-1">{p}</span>
                <button className="text-ink-3 hover:text-coral cursor-pointer" title="Убрать путь" onClick={() => onChange({ exe_paths: app.exe_paths.filter((_, j) => j !== i) })}>
                  <Trash2 size={11} />
                </button>
              </div>
            ))}
            <button className="text-teal hover:underline text-left cursor-pointer" onClick={onAddPath}>
              + путь к exe
            </button>
          </div>
        </div>
        <div className="flex gap-1">
          <button className="btn btn-sm" title="Запустить" onClick={() => api.launchApp(app.id).then(() => toast("info", `${app.name} запускается`)).catch((e) => toast("error", String(e)))}>
            <Play size={12} />
          </button>
          <button className="btn btn-sm btn-danger" title="Удалить из списка" onClick={onRemove}>
            <Trash2 size={12} />
          </button>
        </div>
      </div>

      <div className="grid grid-cols-[1.3fr_1fr_1fr_1fr] gap-3 mt-3 items-end">
        <label className="flex flex-col gap-1">
          <Label title="Канал: каким путём приложение выходит в сеть" text="«Напрямую (Wi-Fi)» — трафик минует все VPN; DotPilot проверяет, что exe нет в списке Happ. «Через Happ VPN» — exe добавляется в список туннеля Happ (нужен режим TUN и включённая синхронизация в настройках). «Radmin в приоритете» — сеть друзей 26.x.x.x идёт через Radmin, интернет напрямую; DotPilot держит метрику Radmin минимальной." rec="Игры — напрямую; браузер — напрямую, а «через Happ» только когда нужны заблокированные сайты; Minecraft с друзьями — Radmin.">
            Канал
          </Label>
          <select className="input" value={app.policy} onChange={(e) => onChange({ policy: e.target.value as Policy })} style={{ borderColor: policyColor[app.policy] + "66" }}>
            {(Object.keys(policyLabel) as Policy[]).map((p) => (
              <option key={p} value={p}>
                {policyLabel[p]}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <Label title="Приоритет (DSCP)" text="Метка в заголовке каждого пакета. Wi-Fi роутер с WMM ставит пакеты с DSCP 46 в голосовую очередь (AC_VO) и отправляет их раньше остальных, даже когда канал забит загрузкой. 8 (CS1) — наоборот, «фон»: пакеты уходят последними." rec="Игры и Discord — 46. Docker, Claude Code, торренты — 8. Браузер — без метки." warn="Работает только если в «Доступ» выдан ключ DSCP вне домена; по проводу через свитч без QoS эффекта нет.">
            Приоритет (DSCP)
          </Label>
          <select className="input" value={app.dscp ?? ""} onChange={(e) => onChange({ dscp: e.target.value === "" ? null : Number(e.target.value) })}>
            <option value="">нет</option>
            <option value="46">46 · игровой / голос (EF)</option>
            <option value="34">34 · видео (AF41)</option>
            <option value="26">26 · важный (AF31)</option>
            <option value="8">8 · фон (CS1)</option>
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <Label title="Лимит скорости в игровом режиме" text="Пока запущена игра, Windows режет исходящую скорость этого приложения до указанной цифры (QoS Throttle). Так Docker или Claude Code не забьют Wi-Fi и не поднимут пинг. Когда игра закрыта, лимита нет." rec="30–40 Мбит/с для Docker и Claude Code: работа продолжается, а игре остаётся запас.">
            Лимит в игре, Мбит/с
          </Label>
          <input className="input" type="number" min={1} max={2000} placeholder="без лимита" value={app.throttle_mbps ?? ""} onChange={(e) => onChange({ throttle_mbps: e.target.value === "" ? null : Number(e.target.value) })} />
        </label>
        <Tip title="Фоновое приложение" text="Помечает приложение как «фон»: в игровом режиме к нему применяется лимит скорости из поля слева. Без этой галочки лимит не действует." rec="Включи для всего, что качает в фоне: Docker, Claude Code, лаунчеры, облачные диски." side="right">
          <label className="flex items-center gap-2 pb-2">
            <Switch on={app.background} onChange={(v) => onChange({ background: v })} />
            <span className="text-[12px] text-ink-2">Фоновое: ограничивать в игровом режиме</span>
          </label>
        </Tip>
      </div>
      <div className="flex items-center gap-2 mt-2">
        <Tip title="Тип приложения" text="«Игра» — запуск этого приложения автоматически включает игровой режим (лимиты фоновых, приоритет DSCP). Остальные типы только для группировки на карте маршрутов.">
        <select className="input !w-auto !py-0.5 text-[11.5px]" value={app.kind} onChange={(e) => onChange({ kind: e.target.value })}>
          <option value="game">игра (включает игровой режим)</option>
          <option value="browser">браузер</option>
          <option value="dev">разработка</option>
          <option value="tool">другое</option>
        </select>
        </Tip>
        <input className="input !py-0.5 text-[11.5px] text-ink-2" placeholder="заметка" value={app.note} onChange={(e) => onChange({ note: e.target.value })} />
      </div>
      {app.policy === "vpn" && !happTun && <div className="text-[11.5px] text-amber mt-2">Happ сейчас не в TUN-режиме: в системный прокси попадают только приложения, которые его уважают (браузеры, Electron). Включи в Happ режим «TUN» и per-app.</div>}

      <button className="mt-3 text-[12px] text-ink-2 hover:text-ink flex items-center gap-1 cursor-pointer" onClick={() => setOpenConn(!openConn)}>
        {openConn ? <ChevronUp size={13} /> : <ChevronDown size={13} />} Соединения и процессы
      </button>
      {openConn && (
        <div className="mt-2 panel-2 p-2.5 text-[11.5px] num">
          {status?.procs.length ? (
            <div className="flex flex-wrap gap-1.5 mb-2">
              {status.procs.map((p) => (
                <span key={p.pid} className="tag bg-line text-ink-2 normal-case tracking-normal">
                  {p.name} #{p.pid} · {p.cpu.toFixed(0)}% · {p.mem_mb} МБ
                </span>
              ))}
            </div>
          ) : (
            <div className="text-ink-3 mb-2">Процессы не запущены.</div>
          )}
          <div className="flex justify-between items-center mb-1">
            <span className="text-ink-3">{loading ? "Загрузка…" : `${conns?.length ?? 0} соединений`}</span>
            <button className="btn btn-sm" onClick={loadConns}>
              <RefreshCw size={11} /> Обновить
            </button>
          </div>
          <div className="max-h-[180px] overflow-y-auto">
            <table className="w-full">
              <tbody>
                {(conns ?? []).map((c, i) => (
                  <tr key={i} className="border-t border-line/50">
                    <td className="py-0.5 text-ink-3 w-10">{c.proto}</td>
                    <td className="text-ink-2">{c.local}</td>
                    <td>{c.remote}</td>
                    <td className="text-ink-3">{c.state}</td>
                    <td className="text-right" style={{ color: c.via.toLowerCase().includes("happ") || c.via === "vgate0" ? "var(--color-violet)" : c.via.toLowerCase().includes("radmin") ? "var(--color-amber)" : "var(--color-teal)" }}>
                      {c.via}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

function ProxyCard() {
  const snap = useStore((s) => s.snap);
  const cfg = useStore((s) => s.cfg);
  const saveConfig = useStore((s) => s.saveConfig);
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  if (!snap || !cfg) return null;
  const p = snap.proxy;
  const act = (a: Parameters<typeof api.proxyFix>[0]) => api.proxyFix(a).then((r) => { toast("success", r.trim()); setTimeout(refresh, 1500); }).catch((e) => toast("error", String(e)));
  const ok = p.problems.length === 0;
  return (
    <Section
      title="Прокси Happ и переменные окружения"
      sub={`127.0.0.1:${p.port}: ${p.alive ? "отвечает" : "не отвечает"} · системный прокси ${p.env.system_enabled ? "вкл" : "выкл"} · HTTP_PROXY ${p.env.env_http || "не задан"}`}
      right={
        <Tip title="Прокси-страж" text="Каждые 5 секунд проверяет порт Happ. Если порт мёртв, а переменные HTTP_PROXY/HTTPS_PROXY или системный прокси на него указывают — временно убирает их, чтобы лаунчеры, node, git и curl не получали «отказано в подключении». Когда Happ снова отвечает — возвращает переменные с исключениями для локальных сетей и Radmin." rec="Оставь включённым: это и есть лечение конфликта «Claude Code работает через переменные, а Minecraft без VPN не видит версии»." side="right">
          <label className="flex items-center gap-2 text-[12px]">
            <Switch on={cfg.proxy_guard} onChange={(v) => saveConfig({ ...cfg, proxy_guard: v }).then(() => toast("success", v ? "Прокси-страж включён" : "Прокси-страж выключен"))} />
            страж
          </label>
        </Tip>
      }
    >
      <div className={`text-[12.5px] leading-snug mb-2 ${ok ? "text-mint" : "text-amber"}`}>
        {ok ? "Конфликтов нет: переменные и системный прокси согласованы с состоянием Happ, Radmin и локальные сети в исключениях." : p.problems.map((x, i) => <div key={i}>• {x}</div>)}
        {p.guard_holding && <div className="text-ink-2 mt-1">Страж временно убрал переменные прокси и вернёт их, когда Happ поднимется.</div>}
      </div>
      <div className="text-[11.5px] text-ink-3 leading-snug mb-2">
        Как это устроено: Happ в режиме «системный прокси» слушает 127.0.0.1:{p.port}. Браузеры берут системный прокси сами, а Claude Code (node) — только из переменных HTTP_PROXY/HTTPS_PROXY. Переменные глобальные: их наследует и лаунчер Minecraft, и всё остальное, поэтому при выключенном Happ они ломают сеть у всех программ. DotPilot запускает «прямые» приложения (Minecraft, Warface) с очищенными переменными и с отключённым системным прокси для Java, а «через Happ» — с переменными, только если порт жив.
      </div>
      <div className="flex gap-1.5 flex-wrap">
        <Tip title="Убрать переменные" text="Удаляет HTTP_PROXY/HTTPS_PROXY/ALL_PROXY из переменных пользователя. Claude Code перестанет ходить через Happ, пока не вернёшь.">
          <button className="btn btn-sm" disabled={!p.env.env_http && !p.env.env_https} onClick={() => act("clear-env")}>Убрать переменные</button>
        </Tip>
        <Tip title="Вернуть переменные" text={`Ставит HTTP_PROXY/HTTPS_PROXY = http://127.0.0.1:${p.port} и NO_PROXY с локальными сетями и Radmin (26.*, 25.*).`}>
          <button className="btn btn-sm" disabled={!p.alive} onClick={() => act("set-env")}>Вернуть переменные</button>
        </Tip>
        <Tip title="Исключения" text="Добавляет 26.* и 25.* (Radmin) в исключения системного прокси и в NO_PROXY: друзья по Radmin и Minecraft-серверы в LAN идут напрямую.">
          <button className="btn btn-sm" disabled={p.exceptions_ok && p.env.env_no_proxy.includes("26.")} onClick={() => act("no-proxy")}>Исключить Radmin и LAN</button>
        </Tip>
        <Tip title="Системный прокси" text={p.env.system_enabled ? "Выключает системный прокси Windows. Happ включит его обратно при следующем подключении." : "Включает системный прокси на порт Happ."}>
          <button className="btn btn-sm" onClick={() => act(p.env.system_enabled ? "disable-system" : "enable-system")}>{p.env.system_enabled ? "Выключить системный прокси" : "Включить системный прокси"}</button>
        </Tip>
      </div>
    </Section>
  );
}

function TelegramCard() {
  const toast = useStore((s) => s.toast);
  const cfg = useStore((s) => s.cfg);
  const saveConfig = useStore((s) => s.saveConfig);
  const [st, setSt] = useState<import("../lib/api").TgwsStatus | null>(null);
  const [link, setLink] = useState("");
  const load = () => api.tgwsStatus().then(setSt).catch(() => setSt(null));
  useEffect(() => {
    load();
    const t = setInterval(load, 6000);
    return () => clearInterval(t);
  }, []);
  useEffect(() => {
    if (cfg && !link) setLink(cfg.tg_proxy_link);
  }, [cfg]);
  if (!cfg) return null;
  const act = (a: "start" | "stop" | "launch-telegram") => api.tgwsControl(a).then((r) => { toast("success", `TgWsProxy: ${r}`); load(); }).catch((e) => toast("error", String(e)));
  return (
    <Section title="Telegram через TgWsProxy" sub={st ? (st.running ? `TgWsProxy работает (pid ${st.pid})${st.ports.length ? ` · слушает ${st.ports.join(", ")}` : " · порты пока не открыты"}` : st.exe_exists ? "TgWsProxy не запущен" : "TgWsProxy_windows.exe не найден") : "…"}>
      <div className="text-[11.5px] text-ink-2 leading-snug mb-2">Локальный прокси для Telegram работает только для самого Telegram и не трогает остальные программы (в отличие от Happ в режиме TUN). DotPilot запускает его скрыто, следит, что он жив, и может добавить прокси в Telegram по ссылке tg://.</div>
      <div className="flex gap-1.5 flex-wrap mb-2">
        <Tip title="Запустить TgWsProxy" text="Стартует exe с рабочего стола в скрытом окне. Порт, который он откроет, появится в подзаголовке.">
          <button className="btn btn-sm" disabled={!st?.exe_exists || st?.running} onClick={() => act("start")}>
            <Play size={12} /> Запустить
          </button>
        </Tip>
        <button className="btn btn-sm btn-danger" disabled={!st?.running} onClick={() => act("stop")}>
          <Square size={12} /> Остановить
        </button>
        <Tip title="Открыть Telegram" text={st?.telegram_running ? "Telegram уже запущен." : "Запускает Telegram Desktop."}>
          <button className="btn btn-sm" onClick={() => act("launch-telegram")}>
            <ExternalLink size={12} /> Telegram
          </button>
        </Tip>
        <label className="flex items-center gap-2 text-[12px] ml-auto">
          <Tip title="Автозапуск с DotPilot" text="При старте DotPilot прокси поднимается сам, если ещё не работает." side="right">
            <Switch on={cfg.tgws_autostart} onChange={(v) => saveConfig({ ...cfg, tgws_autostart: v }).then(() => toast("success", v ? "TgWsProxy будет стартовать вместе с DotPilot" : "Автозапуск TgWsProxy выключен"))} />
          </Tip>
          автозапуск
        </label>
      </div>
      <div className="flex gap-1.5 items-center">
        <Tip title="Ссылка прокси для Telegram" text="tg://socks?server=127.0.0.1&port=ПОРТ — для SOCKS5; tg://proxy?server=127.0.0.1&port=ПОРТ&secret=… — для MTProto. Порт бери из подзаголовка, секрет — из настроек TgWsProxy. Открытие ссылки добавит прокси в Telegram и предложит включить." className="flex-1">
          <input className="input num" placeholder="tg://socks?server=127.0.0.1&port=…" value={link} onChange={(e) => setLink(e.target.value)} />
        </Tip>
        <button className="btn btn-sm" disabled={!link.startsWith("tg://")} onClick={() => saveConfig({ ...cfg, tg_proxy_link: link }).then(() => api.openUri(link)).then(() => toast("success", "Ссылка передана Telegram: подтверди добавление прокси в его окне")).catch((e) => toast("error", String(e)))}>
          Добавить в Telegram
        </button>
      </div>
    </Section>
  );
}

function AdapterRow({ a, onMetric, onToggle }: { a: import("../lib/api").Adapter; onMetric: (m: number | null) => void; onToggle: (on: boolean) => void }) {
  const [m, setM] = useState<string>(a.metric?.toString() ?? "");
  useEffect(() => setM(a.metric?.toString() ?? ""), [a.metric]);
  const roleLabel: Record<string, string> = { wifi: "Wi-Fi", ethernet: "Ethernet", happ: "Happ / TUN", radmin: "Radmin", virtual: "виртуальный", other: "другое" };
  const roleColor: Record<string, string> = { wifi: "var(--color-teal)", ethernet: "var(--color-teal)", happ: "var(--color-violet)", radmin: "var(--color-amber)", virtual: "var(--color-ink-3)", other: "var(--color-ink-3)" };
  return (
    <tr className="border-t border-line/60">
      <td className="py-1.5">
        <div className="font-medium">{a.name}</div>
        <div className="text-[11px] text-ink-3 truncate max-w-[260px]">{a.description}</div>
      </td>
      <td>
        <Tag color={roleColor[a.role]}>{roleLabel[a.role]}</Tag>
      </td>
      <td>
        <StatusPill ok={a.status === "Up"} text={a.status} />
      </td>
      <td className="num text-ink-2">{a.ipv4.join(", ") || "—"}</td>
      <td className="num text-ink-2">{a.link_speed || "—"}</td>
      <td>
        <Tip title="Метрика интерфейса" text="Стоимость маршрута через этот адаптер: при равных маршрутах Windows выбирает адаптер с меньшей метрикой. Пусто = Windows считает сама по скорости линка." rec="Wi-Fi 25–45, Ethernet 5–25, VPN-адаптеры выше 50, чтобы интернет по умолчанию шёл через физическую сеть. Radmin — 1, но он влияет только на сеть 26.x." warn="Слишком низкая метрика у VPN-адаптера уводит весь трафик в туннель.">
          <div className="flex items-center gap-1">
            <input className="input !w-16 !py-0.5 num" value={m} onChange={(e) => setM(e.target.value)} onBlur={() => m !== (a.metric?.toString() ?? "") && onMetric(m === "" ? null : Number(m))} placeholder="авто" />
            {a.automatic_metric && <span className="text-[10px] text-ink-3">авто</span>}
          </div>
        </Tip>
      </td>
      <td className="num text-ink-2 text-[11.5px]">
        {a.status === "Up" ? (
          <>
            {fmtBps(a.rx_bps)} ↓<br />
            {fmtBps(a.tx_bps)} ↑
          </>
        ) : (
          "—"
        )}
      </td>
      <td className="text-right">
        <Tip title="Включить / выключить адаптер" text="Выключенный адаптер исчезает для Windows целиком: ни маршрутов, ни DNS. Так можно быстро убрать мешающий VPN-адаптер." warn="Не выключай Wi-Fi, через который сейчас идёт интернет." side="right">
          <Switch on={a.status === "Up" || a.status === "Disconnected"} onChange={onToggle} />
        </Tip>
      </td>
    </tr>
  );
}
