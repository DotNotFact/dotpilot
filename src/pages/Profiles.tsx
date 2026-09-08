import { useState } from "react";
import { useStore } from "../store";
import { api, type Profile } from "../lib/api";
import { Section, Tag, Tip } from "../components/ui";
import { Gamepad2, Briefcase, Leaf, SlidersHorizontal, Check, Zap } from "lucide-react";

const icons: Record<string, React.ReactNode> = {
  gamepad: <Gamepad2 size={20} />,
  briefcase: <Briefcase size={20} />,
  leaf: <Leaf size={20} />,
  sliders: <SlidersHorizontal size={20} />,
};

export default function Profiles() {
  const snap = useStore((s) => s.snap);
  const cfg = useStore((s) => s.cfg);
  const saveConfig = useStore((s) => s.saveConfig);
  const loadConfig = useStore((s) => s.loadConfig);
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  const [busy, setBusy] = useState<string | null>(null);
  const [edit, setEdit] = useState<Profile | null>(null);

  if (!snap || !cfg) return <div className="text-ink-3">Загрузка…</div>;

  const apply = async (p: Profile) => {
    try {
      setBusy(p.id);
      const lines = await api.applyProfile(p.id);
      await loadConfig();
      toast("success", `Профиль «${p.name}»: ${lines.join(" · ")}`);
      refresh();
    } catch (e) {
      toast("error", String(e));
    } finally {
      setBusy(null);
    }
  };
  const saveEdit = async () => {
    if (!edit) return;
    await saveConfig({ ...cfg, profiles: cfg.profiles.map((p) => (p.id === edit.id ? edit : p)) });
    setEdit(null);
    toast("success", "Профиль сохранён");
  };

  const activePlan = snap.power_plans.find((p) => p.active);

  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Профили</div>
        <h1 className="text-[24px] mt-1">Один клик — весь ПК под задачу</h1>
        <p className="text-ink-2 text-[12.5px] mt-1 max-w-[760px] leading-snug">Профиль переключает схему питания Windows, игровой режим сети и состояние zapret / Happ. Позже сюда добавятся кривые вентиляторов, лимиты мощности CPU/GPU и разгон.</p>
      </header>

      <div className="grid grid-cols-4 gap-3">
        {cfg.profiles.map((p) => {
          const active = cfg.active_profile === p.id;
          const plan = snap.power_plans.find((x) => x.guid === p.power_plan);
          return (
            <div key={p.id} className="panel p-4 flex flex-col gap-3" style={{ borderColor: active ? "var(--color-teal-2)" : undefined }}>
              <div className="flex items-center justify-between">
                <span className="text-teal">{icons[p.icon] ?? icons.sliders}</span>
                {active && (
                  <Tag color="var(--color-teal)">
                    <Check size={11} /> активен
                  </Tag>
                )}
              </div>
              <div>
                <h2 className="text-[16px]">{p.name}</h2>
                <p className="text-[12px] text-ink-2 mt-1 leading-snug min-h-[54px]">{p.description}</p>
              </div>
              <div className="text-[11.5px] text-ink-3 flex flex-col gap-0.5">
                <div>Питание: {plan?.name ?? "—"}</div>
                <div>Игровой режим: {p.game_mode ? "включён" : "авто"}</div>
                <div>zapret: {p.zapret_running === null ? "не трогать" : p.zapret_running ? "вкл" : "выкл"}</div>
                <div>Happ: {p.happ_running === null ? "не трогать" : p.happ_running ? "вкл" : "выкл"}</div>
              </div>
              <div className="flex gap-1.5 mt-auto">
                <Tip title={`Применить «${p.name}»`} text={`Переключит схему питания на «${plan?.name ?? "текущую"}», игровой режим сети — ${p.game_mode ? "включит вручную" : "вернёт в автомат"}${p.zapret_running === null ? "" : p.zapret_running ? ", запустит zapret" : ", остановит zapret"}${p.happ_running === null ? "" : p.happ_running ? ", включит службу Happ" : ", выключит службу Happ"}.`} rec={p.id === "gaming" ? "Перед Warface." : p.id === "work" ? "Перед сессией Claude Code / Docker." : p.id === "eco" ? "На ночь или при работе от ИБП." : "Обычное состояние ПК."} className="flex-1">
                  <button className="btn btn-primary w-full justify-center" disabled={busy !== null} onClick={() => apply(p)}>
                    <Zap size={13} /> {busy === p.id ? "…" : "Применить"}
                  </button>
                </Tip>
                <button className="btn" onClick={() => setEdit({ ...p })}>
                  Изменить
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {edit && (
        <Section title={`Редактирование: ${edit.name}`} right={<button className="btn btn-sm" onClick={() => setEdit(null)}>закрыть</button>}>
          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1">
              <span className="eyebrow">Название</span>
              <input className="input" value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} />
            </label>
            <label className="flex flex-col gap-1">
              <span className="eyebrow">Схема питания</span>
              <select className="input" value={edit.power_plan} onChange={(e) => setEdit({ ...edit, power_plan: e.target.value })}>
                <option value="">не менять</option>
                {snap.power_plans.map((p) => (
                  <option key={p.guid} value={p.guid}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 col-span-2">
              <span className="eyebrow">Описание</span>
              <input className="input" value={edit.description} onChange={(e) => setEdit({ ...edit, description: e.target.value })} />
            </label>
            <TriState label="Игровой режим сети" value={edit.game_mode ? true : null} labels={["включить", "авто"]} onChange={(v) => setEdit({ ...edit, game_mode: v === true })} />
            <TriState label="zapret" value={edit.zapret_running} onChange={(v) => setEdit({ ...edit, zapret_running: v })} />
            <TriState label="Happ (служба)" value={edit.happ_running} onChange={(v) => setEdit({ ...edit, happ_running: v })} />
          </div>
          <div className="mt-3 flex gap-2">
            <button className="btn btn-primary" onClick={saveEdit}>
              Сохранить профиль
            </button>
          </div>
        </Section>
      )}

      <Section title="Схема питания Windows" sub={`Сейчас: ${activePlan?.name ?? "неизвестно"}`}>
        <div className="flex gap-2 flex-wrap">
          {snap.power_plans.map((p) => (
            <Tip key={p.guid} title={p.name} text={/Высокая|High/i.test(p.name) ? "Процессор не сбрасывает частоту в простое, USB и PCIe не засыпают. Минимальные задержки ввода и сети, выше нагрев и энергопотребление." : /Сбаланс|Balanced/i.test(p.name) ? "Частоты и питание меняются по нагрузке. Для 7900X это почти не влияет на FPS, но Wi-Fi-адаптер может уходить в энергосбережение и добавлять джиттер." : /Экономия|saver/i.test(p.name) ? "Всё ради тишины и низкого нагрева: частоты ограничены, устройства засыпают. Пинг и FPS будут хуже." : "Схема стороннего ПО (Driver Booster). Обычно копия «Высокой производительности» с отключёнными таймаутами устройств."} rec="Для игр — Высокая производительность; для работы допустима Сбалансированная, если не мешает джиттер Wi-Fi.">
              <button className={`btn ${p.active ? "btn-primary" : ""}`} onClick={() => api.setPowerPlan(p.guid).then(() => toast("success", `Схема «${p.name}» активна`)).then(refresh).catch((e) => toast("error", String(e)))}>
                {p.name}
              </button>
            </Tip>
          ))}
        </div>
      </Section>

      <Section title="Дорожная карта профилей" sub="Что появится в следующих версиях">
        <ul className="text-[12.5px] text-ink-2 list-disc ml-5 leading-relaxed">
          <li>Лимит мощности и кривые вентиляторов через LibreHardwareMonitor / ATK HUB.</li>
          <li>PBO / Curve Optimizer для Ryzen 9 7900X (через Ryzen Master SDK) и лимиты NVIDIA (nvidia-smi -pl).</li>
          <li>Автопереключение профиля по запущенному приложению: игра → «Игра», Claude Code → «Работа».</li>
        </ul>
      </Section>
    </div>
  );
}

function TriState({ label, value, onChange, labels = ["включить", "выключить"] }: { label: string; value: boolean | null; onChange: (v: boolean | null) => void; labels?: [string, string] }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="eyebrow">{label}</span>
      <div className="flex gap-1">
        <button className={`btn btn-sm ${value === true ? "btn-primary" : ""}`} onClick={() => onChange(true)}>
          {labels[0]}
        </button>
        {labels[1] !== "авто" && (
          <button className={`btn btn-sm ${value === false ? "btn-primary" : ""}`} onClick={() => onChange(false)}>
            {labels[1]}
          </button>
        )}
        <button className={`btn btn-sm ${value === null ? "btn-primary" : ""}`} onClick={() => onChange(null)}>
          {labels[1] === "авто" ? "авто" : "не трогать"}
        </button>
      </div>
    </div>
  );
}
