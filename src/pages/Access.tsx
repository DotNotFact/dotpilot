import { useEffect, useState } from "react";
import { useStore } from "../store";
import { api, type PermItem } from "../lib/api";
import { Section, Tag, Tip, StatusPill } from "../components/ui";
import { KeyRound, RefreshCw, ShieldCheck, Zap, Undo2 } from "lucide-react";

export default function Access() {
  const toast = useStore((s) => s.toast);
  const snap = useStore((s) => s.snap);
  const [items, setItems] = useState<PermItem[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = () => api.permsStatus().then(setItems).catch((e) => toast("error", String(e)));
  useEffect(() => {
    load();
  }, []);

  const grant = async (it: PermItem, on: boolean) => {
    setBusy(it.id);
    try {
      if (it.id === "admin") {
        toast("info", "Перезапуск с правами администратора: подтверди запрос UAC");
        await api.relaunchElevated();
        return;
      }
      const r = await api.permsGrant(it.id, on);
      toast("success", r.trim());
      await load();
      useStore.getState().refresh();
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(null);
    }
  };

  const missing = (items ?? []).filter((i) => i.state === false && i.can_grant && i.id !== "admin" && i.id !== "apo");
  const grantAll = async () => {
    for (const it of missing) await grant(it, true);
  };

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-end justify-between gap-4">
        <div>
          <div className="eyebrow">Доступ</div>
          <h1 className="text-[24px] mt-1 flex items-center gap-2">
            <KeyRound size={22} className="text-teal" /> Разрешения и зависимости
          </h1>
          <p className="text-ink-2 text-[12.5px] mt-1 max-w-[800px] leading-snug">Всё, что DotPilot должен получить от Windows, чтобы работать полностью. Каждый пункт можно выдать одной кнопкой: приложение само выполняет нужный скрипт (реестр, службы, планировщик, PowerShell Gallery). Наведи на пункт, чтобы понять, что именно меняется.</p>
        </div>
        <div className="flex gap-2">
          <button className="btn" onClick={load}>
            <RefreshCw size={14} /> Проверить
          </button>
          <Tip title="Выдать всё недостающее" text={missing.length ? `Будут выполнены: ${missing.map((m) => m.name).join(", ")}.` : "Все доступные разрешения уже выданы."} rec="Права администратора и Equalizer APO выдаются отдельно, потому что требуют подтверждения UAC или установщика." side="right">
            <button className="btn btn-primary" disabled={!missing.length || busy !== null || !snap?.admin} onClick={grantAll}>
              <Zap size={14} /> Выдать всё ({missing.length})
            </button>
          </Tip>
        </div>
      </header>

      {!snap?.admin && (
        <div className="panel p-3 text-[12.5px] border-amber/50 text-amber flex items-center justify-between gap-3">
          <span>Без прав администратора можно выдать только «Модуль управления звуком». Остальное требует перезапуска с UAC.</span>
          <button className="btn btn-sm" onClick={() => api.relaunchElevated()}>
            <ShieldCheck size={13} /> Перезапустить от администратора
          </button>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {(items ?? []).map((it) => (
          <div key={it.id} className="panel p-4 flex flex-col gap-2">
            <div className="flex items-start justify-between gap-2">
              <div>
                <div className="text-[14px] font-semibold flex items-center gap-2">
                  {it.name}
                  {it.needs_admin && (
                    <Tip title="Нужны права администратора" text="Скрипт пишет в HKLM или управляет службами, поэтому DotPilot должен быть запущен от администратора.">
                      <Tag color="var(--color-amber)">admin</Tag>
                    </Tip>
                  )}
                </div>
                <div className="text-[11.5px] mt-1">
                  <StatusPill ok={it.state === true} warn={it.state === null} text={it.state === true ? "выдано" : it.state === false ? "не выдано" : "неизвестно"} />
                </div>
              </div>
              <div className="flex gap-1">
                {it.state !== true && it.can_grant && (
                  <Tip title="Что произойдёт" text={it.effect} side="right">
                    <button className="btn btn-primary btn-sm" disabled={busy !== null || (it.needs_admin && !snap?.admin)} onClick={() => grant(it, true)}>
                      {busy === it.id ? "…" : it.id === "admin" ? "Перезапустить" : it.id === "apo" ? "Скачать" : "Выдать"}
                    </button>
                  </Tip>
                )}
                {it.state === true && ["location", "nla", "autostart"].includes(it.id) && (
                  <Tip title="Откатить" text="Возвращает настройку в исходное состояние." side="right">
                    <button className="btn btn-sm" disabled={busy !== null || !snap?.admin} onClick={() => grant(it, false)}>
                      <Undo2 size={12} /> Откатить
                    </button>
                  </Tip>
                )}
              </div>
            </div>
            <div className="text-[12.5px] text-ink-2 leading-snug">{it.description}</div>
            <div className="text-[11.5px] text-ink-3 leading-snug">
              <span className="text-ink-2">Что сделает кнопка:</span> {it.effect}
            </div>
            <div className="text-[11px] text-ink-3 num">{it.detail}</div>
          </div>
        ))}
        {items === null && <div className="text-ink-3 text-[12.5px]">Проверяю состояние…</div>}
      </div>

      <Section title="Что уже сделано автоматически" sub="Без отдельных кнопок">
        <ul className="text-[12.5px] text-ink-2 list-disc ml-5 leading-relaxed">
          <li>Манифест exe требует права администратора: Windows сама показывает UAC при запуске. Задача планировщика убирает этот запрос.</li>
          <li>PowerShell запускается с ExecutionPolicy Bypass только для скриптов DotPilot; глобальная политика не меняется.</li>
          <li>ICMP-пинг идёт через системный IcmpSendEcho и не требует raw-сокетов и правил брандмауэра.</li>
          <li>Ключ «Do not use NLA» ставится автоматически при каждом применении политик QoS.</li>
        </ul>
      </Section>
    </div>
  );
}
