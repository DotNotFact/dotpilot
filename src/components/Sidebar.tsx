import { useStore, type Page } from "../store";
import {
  Activity,
  Gauge,
  LayoutDashboard,
  Network,
  Headphones,
  SlidersHorizontal,
  Cpu,
  MonitorCog,
  Fan,
  Sparkles,
  KeyRound,
  Settings,
  ShieldCheck,
  ShieldAlert,
} from "lucide-react";
import type { ReactNode } from "react";
import { Tip } from "./ui";

const items: { id: Page; label: string; icon: ReactNode; soon?: boolean; hint: string }[] = [
  { id: "dashboard", label: "Главная", icon: <LayoutDashboard size={17} />, hint: "Состояние сети и ПК одним взглядом: пинг, потери, Wi-Fi, карта маршрутов, экспорт данных." },
  { id: "network", label: "Сеть", icon: <Network size={17} />, hint: "Кто через какой канал выходит в сеть: политики приложений, QoS, службы Happ / zapret / Radmin, адаптеры, твики." },
  { id: "audio", label: "Звук", icon: <Headphones size={17} />, hint: "Наушники: профили Bluetooth, устройство по умолчанию, эквалайзер для шагов." },
  { id: "processes", label: "Процессы", icon: <Activity size={17} />, hint: "Диспетчер: кто грузит CPU, память и видеокарту; подозрительные процессы; завершение." },
  { id: "impact", label: "Влияние", icon: <Gauge size={17} />, hint: "Тест: сколько CPU и памяти отнимает сам DotPilot в простое, при работе и в трее." },
  { id: "profiles", label: "Профили", icon: <SlidersHorizontal size={17} />, hint: "Готовые режимы ПК: Стандарт, Игра, Работа, Экономия. Схема питания + сеть + службы." },
  { id: "cpu", label: "Процессор", icon: <Cpu size={17} />, hint: "Загрузка по потокам, частота, память." },
  { id: "gpu", label: "Видеокарта", icon: <MonitorCog size={17} />, hint: "Температура, мощность, частоты и память NVIDIA." },
  { id: "fans", label: "Вентиляторы", icon: <Fan size={17} />, soon: true, hint: "Кривые охлаждения появятся позже (нужен LibreHardwareMonitor)." },
  { id: "ai", label: "Диагностика Claude", icon: <Sparkles size={17} />, hint: "Отправить снимок сети Claude и получить план действий." },
  { id: "access", label: "Доступ", icon: <KeyRound size={17} />, hint: "Разрешения Windows и зависимости, которые приложение выдаёт себе само." },
  { id: "settings", label: "Настройки", icon: <Settings size={17} />, hint: "Пути к программам, API-ключ, цели пинга." },
];

export default function Sidebar() {
  const page = useStore((s) => s.page);
  const setPage = useStore((s) => s.setPage);
  const snap = useStore((s) => s.snap);

  const wifi = snap?.adapters.find((a) => a.role === "wifi" && a.status === "Up");
  const happ = snap?.services.find((s) => s.id === "happ");
  const radmin = snap?.adapters.find((a) => a.role === "radmin");
  const zapret = snap?.services.find((s) => s.id === "zapret");
  const gm = snap?.game_mode.active;

  return (
    <aside className="w-[236px] flex-none flex flex-col border-r border-line bg-panel/60">
      <div className="px-5 pt-5 pb-4">
        <div className="flex items-center gap-2.5">
          <Logo />
          <div>
            <div className="font-display font-semibold text-[15px] leading-none tracking-tight">DotPilot</div>
            <div className="text-[10.5px] text-ink-3 mt-1">центр управления ПК</div>
          </div>
        </div>
      </div>

      <nav className="px-3 flex flex-col gap-0.5">
        {items.map((it, idx) => (
          <Tip key={it.id} title={idx < 10 ? `${it.label} · Ctrl+${(idx + 1) % 10}` : it.label} text={it.hint} className="w-full">
            <button
              onClick={() => setPage(it.id)}
              className={`w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-left cursor-pointer transition-colors ${
                page === it.id ? "bg-panel-2 text-ink border border-line-2" : "text-ink-2 hover:text-ink hover:bg-panel-2/60 border border-transparent"
              }`}
            >
              <span className={page === it.id ? "text-teal" : ""}>{it.icon}</span>
              <span className="flex-1">{it.label}</span>
              {it.soon && <span className="tag bg-line text-ink-3">скоро</span>}
            </button>
          </Tip>
        ))}
      </nav>

      <div className="mt-auto px-4 pb-4">
        <div className="eyebrow mb-2">Линия связи</div>
        <div className="panel-2 p-3 flex flex-col gap-2 text-[12px]">
          <Row label="Wi-Fi" ok={!!wifi} value={wifi ? wifi.link_speed : "нет"} hint="Скорость линка Wi-Fi. Ниже 300 Mbps на Wi-Fi 7 — слабый сигнал или 2.4 ГГц." />
          <Row
            label="Happ"
            ok={happ?.mode === "tun" || happ?.mode === "proxy"}
            warn={happ?.mode === "idle"}
            value={happ?.mode === "tun" ? "TUN" : happ?.mode === "proxy" ? "прокси" : happ?.mode === "idle" ? "ожидание" : "выкл"}
            hint="Режим Happ VPN: TUN — в туннель идут выбранные приложения; прокси — только те, кто уважает системный прокси (браузеры); ожидание — ядро работает, трафик не перехватывается."
          />
          <Row label="Radmin" ok={radmin?.status === "Up"} value={radmin?.status === "Up" ? radmin.ipv4[0]?.split("/")[0] ?? "up" : "выкл"} hint="Адаптер Radmin VPN и твой адрес в сети 26.x.x.x для игры с друзьями." />
          <Row label="zapret" ok={!!zapret?.gui_running} value={zapret?.gui_running ? "winws" : "выкл"} hint="winws.exe обходит DPI-блокировки для YouTube/Discord. Работает в фоне для всех приложений." />
          <div className="border-t border-line-2 my-0.5" />
          <Row label="Игровой режим" ok={!!gm} value={gm ? "вкл" : "выкл"} accent="amber" hint="Включён — фоновые приложения (Claude Code, Docker) ограничены по скорости, игры помечены приоритетом DSCP." />
        </div>
        <div className="mt-3 flex items-center gap-1.5 text-[11px] text-ink-3">
          {snap?.admin ? (
            <>
              <ShieldCheck size={13} className="text-mint" /> права администратора
            </>
          ) : (
            <>
              <ShieldAlert size={13} className="text-coral" /> без прав администратора
            </>
          )}
        </div>
        <div className="mt-1 text-[10.5px] text-ink-3">Крестик сворачивает в трей · ПКМ по значку — быстрые действия</div>
      </div>
    </aside>
  );
}

function Row({ label, ok, warn, value, accent, hint }: { label: string; ok: boolean; warn?: boolean; value: string; accent?: "amber"; hint: string }) {
  const color = ok ? (accent === "amber" ? "var(--color-amber)" : "var(--color-mint)") : warn ? "var(--color-amber)" : "var(--color-ink-3)";
  return (
    <Tip title={label} text={hint} up className="w-full">
      <div className="flex items-center gap-2 w-full">
        <span className={`dot ${ok ? "dot-live" : ""}`} style={{ background: color }} />
        <span className="text-ink-2 flex-1">{label}</span>
        <span className="num text-[11.5px] text-ink">{value}</span>
      </div>
    </Tip>
  );
}

function Logo() {
  return (
    <svg width="34" height="34" viewBox="0 0 34 34" fill="none" aria-hidden>
      <rect x="1" y="1" width="32" height="32" rx="9" fill="#121721" stroke="#2d3a4f" />
      <circle cx="17" cy="17" r="10.5" stroke="#35d0c6" strokeOpacity=".45" />
      <circle cx="17" cy="17" r="6" stroke="#35d0c6" strokeOpacity=".7" />
      <path d="M17 17 L26 9" stroke="#35d0c6" strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="17" cy="17" r="2" fill="#f2b33d" />
      <circle cx="23" cy="12" r="1.6" fill="#f2b33d" />
    </svg>
  );
}
