import { useEffect, useState, Component, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { useStore, type Page, type Toast } from "./store";
import Sidebar from "./components/Sidebar";
import Dashboard from "./pages/Dashboard";
import Network from "./pages/Network";
import Profiles from "./pages/Profiles";
import Cpu from "./pages/Cpu";
import Gpu from "./pages/Gpu";
import Fans from "./pages/Fans";
import Ai from "./pages/Ai";
import Audio from "./pages/Audio";
import Access from "./pages/Access";
import Processes from "./pages/Processes";
import Impact from "./pages/Impact";
import Settings from "./pages/Settings";
import { X, Copy, ChevronDown, ChevronUp } from "lucide-react";

export const PAGES: Page[] = ["dashboard", "network", "audio", "processes", "impact", "profiles", "cpu", "gpu", "fans", "ai", "access", "settings"];

class ErrorBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error ? `${e.message}\n${e.stack ?? ""}` : String(e) };
  }
  render() {
    if (this.state.error)
      return (
        <div className="panel p-4 m-6 text-[12.5px] border-coral/60">
          <div className="text-coral font-semibold mb-2">Ошибка отрисовки страницы</div>
          <pre className="num whitespace-pre-wrap selectable text-ink-2">{this.state.error}</pre>
          <div className="flex gap-2 mt-3">
            <button className="btn" onClick={() => this.setState({ error: null })}>
              Попробовать снова
            </button>
            <button className="btn" onClick={() => navigator.clipboard.writeText(this.state.error ?? "")}>
              <Copy size={13} /> Скопировать
            </button>
          </div>
        </div>
      );
    return this.props.children;
  }
}

export default function App() {
  const page = useStore((s) => s.page);
  const refresh = useStore((s) => s.refresh);
  const loadConfig = useStore((s) => s.loadConfig);
  const toasts = useStore((s) => s.toasts);
  const toast = useStore((s) => s.toast);
  const dismissAll = useStore((s) => s.dismissAll);

  useEffect(() => {
    loadConfig();
    refresh();
    // The backend emits "snapshot" after every collection; no extra polling timer needed.
    const unlistenP = listen("snapshot", () => refresh());
    const unlistenG = listen<boolean>("game-mode", (e) => {
      toast("info", e.payload ? "Игровой режим включён: фоновые приложения ограничены" : "Игровой режим выключен: лимиты сняты");
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismissAll();
      if (e.ctrlKey && !e.altKey && /^[0-9]$/.test(e.key)) {
        e.preventDefault();
        const n = e.key === "0" ? 9 : Number(e.key) - 1;
        if (PAGES[n]) useStore.getState().setPage(PAGES[n]);
      }
    };
    window.addEventListener("keydown", onKey);
    const onErr = (e: ErrorEvent) => toast("error", `JS: ${e.message}`);
    const onRej = (e: PromiseRejectionEvent) => toast("error", `Promise: ${String(e.reason)}`);
    window.addEventListener("error", onErr);
    window.addEventListener("unhandledrejection", onRej);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("error", onErr);
      window.removeEventListener("unhandledrejection", onRej);
      unlistenP.then((f) => f());
      unlistenG.then((f) => f());
    };
  }, []);

  return (
    <div className="flex h-full">
      <Sidebar />
      <main className="flex-1 min-w-0 overflow-y-auto">
        <div className="px-7 py-6 w-full max-w-[1900px] mx-auto">
          <ErrorBoundary key={page}>
            {page === "dashboard" && <Dashboard />}
            {page === "network" && <Network />}
            {page === "audio" && <Audio />}
            {page === "processes" && <Processes />}
            {page === "impact" && <Impact />}
            {page === "profiles" && <Profiles />}
            {page === "cpu" && <Cpu />}
            {page === "gpu" && <Gpu />}
            {page === "fans" && <Fans />}
            {page === "ai" && <Ai />}
            {page === "access" && <Access />}
            {page === "settings" && <Settings />}
          </ErrorBoundary>
        </div>
      </main>
      <div className="fixed bottom-4 right-4 flex flex-col gap-2 z-50 w-[420px] max-w-[calc(100vw-280px)]">
        {toasts.length > 1 && (
          <button className="btn btn-sm self-end" onClick={dismissAll}>
            Закрыть все (Esc)
          </button>
        )}
        {toasts.map((t) => (
          <ToastCard key={t.id} t={t} />
        ))}
      </div>
    </div>
  );
}

function ToastCard({ t }: { t: Toast }) {
  const dismiss = useStore((s) => s.dismissToast);
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const long = t.text.length > 140 || t.text.includes("\n");
  const color = t.kind === "error" ? "var(--color-coral)" : t.kind === "success" ? "var(--color-mint)" : "var(--color-teal)";
  return (
    <div className="toast text-[12.5px]" style={{ borderColor: `color-mix(in srgb, ${color} 55%, transparent)` }}>
      <div className="flex items-start gap-2 px-3 pt-2.5">
        <span className="dot mt-1.5 flex-none" style={{ background: color }} />
        <div className="flex-1 min-w-0">
          <div className="text-[10.5px] text-ink-3 uppercase tracking-wider mb-0.5">
            {t.kind === "error" ? "Ошибка" : t.kind === "success" ? "Готово" : "Инфо"} · {new Date(t.ts).toLocaleTimeString("ru-RU")}
          </div>
          <div className="toast-body leading-snug selectable whitespace-pre-wrap break-words" data-open={open} onClick={() => !open && setOpen(true)}>
            {t.text}
          </div>
        </div>
        <button onClick={() => dismiss(t.id)} className="text-ink-3 hover:text-ink cursor-pointer flex-none" title="Закрыть">
          <X size={15} />
        </button>
      </div>
      <div className="flex items-center gap-1 px-3 py-1.5">
        <button
          className="btn btn-sm"
          onClick={() => {
            navigator.clipboard.writeText(t.text).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            });
          }}
        >
          <Copy size={11} /> {copied ? "Скопировано" : "Копировать"}
        </button>
        {long && (
          <button className="btn btn-sm" onClick={() => setOpen(!open)}>
            {open ? <ChevronUp size={11} /> : <ChevronDown size={11} />} {open ? "Свернуть" : "Показать полностью"}
          </button>
        )}
        {t.kind === "error" && <span className="text-[10.5px] text-ink-3 ml-auto">не исчезнет, пока не закроешь · Esc — закрыть все</span>}
      </div>
    </div>
  );
}
