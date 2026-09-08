import { create } from "zustand";
import { api, type Config, type Snapshot } from "./lib/api";

export type Page = "dashboard" | "health" | "experiments" | "network" | "audio" | "processes" | "impact" | "profiles" | "cpu" | "gpu" | "fans" | "ai" | "access" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "error" | "success";
  text: string;
  ts: number;
}

interface Store {
  page: Page;
  setPage: (p: Page) => void;
  snap: Snapshot | null;
  cfg: Config | null;
  toasts: Toast[];
  errorLog: Toast[];
  toast: (kind: Toast["kind"], text: string) => void;
  dismissToast: (id: number) => void;
  dismissAll: () => void;
  refresh: () => Promise<void>;
  loadConfig: () => Promise<void>;
  saveConfig: (cfg: Config) => Promise<void>;
}

let toastSeq = 1;

export const useStore = create<Store>((set, get) => ({
  page: "dashboard",
  setPage: (page) => set({ page }),
  snap: null,
  cfg: null,
  toasts: [],
  errorLog: [],
  toast: (kind, text) => {
    const id = toastSeq++;
    const t: Toast = { id, kind, text, ts: Date.now() };
    set({ toasts: [...get().toasts, t].slice(-6), errorLog: kind === "error" ? [t, ...get().errorLog].slice(0, 50) : get().errorLog });
    // Errors stay until closed; everything else disappears on its own.
    if (kind !== "error") setTimeout(() => get().dismissToast(id), 5000);
  },
  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
  dismissAll: () => set({ toasts: [] }),
  refresh: async () => {
    try {
      const snap = await api.snapshot();
      if (snap && snap.ts) set({ snap });
    } catch (e) {
      console.error(e);
    }
  },
  loadConfig: async () => {
    const cfg = await api.config();
    set({ cfg });
  },
  saveConfig: async (cfg) => {
    await api.saveConfig(cfg);
    set({ cfg });
  },
}));
