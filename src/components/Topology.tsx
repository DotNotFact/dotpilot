import type { Snapshot, Config } from "../lib/api";
import { fmtBps } from "../lib/api";

/**
 * Signature element: a live route map.  Apps on the left, adapters in the middle,
 * destinations on the right.  Lines animate when the app is running.
 */
export default function Topology({ snap, cfg }: { snap: Snapshot; cfg: Config }) {
  const wifi = snap.adapters.find((a) => a.role === "wifi" && a.status === "Up") ?? snap.adapters.find((a) => a.role === "wifi");
  const eth = snap.adapters.find((a) => a.role === "ethernet" && a.status === "Up");
  const happAd = snap.adapters.find((a) => a.role === "happ" && a.status === "Up");
  const radmin = snap.adapters.find((a) => a.role === "radmin");
  const happ = snap.services.find((s) => s.id === "happ");
  const zapret = snap.services.find((s) => s.id === "zapret");

  const uplink = eth ?? wifi;
  const uplinkUp = !!uplink && uplink.status === "Up";
  const happActive = happ?.mode === "tun" || happ?.mode === "proxy";
  const radminUp = radmin?.status === "Up";

  const W = 880;
  const rowH = 46;
  const apps = cfg.apps;
  const H = Math.max(apps.length * rowH + 30, 5 * rowH + 30);
  const xApp = 150;
  const xAd = 430;
  const xDst = 720;

  const adapters = [
    { id: "uplink", label: uplink ? (uplink.role === "ethernet" ? "Ethernet" : "Wi-Fi") : "Wi-Fi", sub: uplink ? `${uplink.link_speed} · ${fmtBps(uplink.rx_bps)} ↓` : "нет адаптера", up: uplinkUp, color: "#35d0c6" },
    { id: "happ", label: "Happ VPN", sub: happ?.mode === "tun" ? `TUN ${happAd?.name ?? ""}` : happ?.mode === "proxy" ? "системный прокси" : happ?.mode === "idle" ? "ядро ждёт" : "выключен", up: happActive, color: "#9085e9" },
    { id: "radmin", label: "Radmin VPN", sub: radminUp ? radmin!.ipv4[0]?.split("/")[0] ?? "up" : "выключен", up: radminUp, color: "#f2b33d" },
    { id: "zapret", label: "zapret", sub: zapret?.gui_running ? "winws фильтрует DPI" : "выключен", up: !!zapret?.gui_running, color: "#5fd18c" },
  ];
  const adY = (i: number) => 40 + i * ((H - 60) / 3);

  const dsts = [
    { id: "inet", label: "Интернет", sub: uplink?.dns[0] ? `DNS ${uplink.dns[0]}` : "", y: adY(0) + 10 },
    { id: "blocked", label: "Блокировки РКН", sub: "через Happ / zapret", y: adY(1) + 20 },
    { id: "lan", label: "LAN друзей", sub: "26.0.0.0/8", y: adY(2) + 20 },
  ];

  const status = snap.apps;

  type Edge = { from: [number, number]; to: [number, number]; color: string; active: boolean; dim?: boolean };
  const edges: Edge[] = [];
  apps.forEach((a, i) => {
    const y = 30 + i * rowH;
    const st = status.find((s) => s.id === a.id);
    const running = !!st?.running;
    const inHapp = !!st?.in_happ_list;
    const viaVpn = a.policy === "vpn" && (happActive || inHapp);
    if (viaVpn) {
      edges.push({ from: [xApp + 8, y], to: [xAd - 8, adY(1)], color: "#9085e9", active: running });
    } else {
      edges.push({ from: [xApp + 8, y], to: [xAd - 8, adY(0)], color: a.color, active: running });
    }
    if (a.policy === "radmin") {
      edges.push({ from: [xApp + 8, y], to: [xAd - 8, adY(2)], color: "#f2b33d", active: running && radminUp, dim: !radminUp });
    }
    if (a.kind === "browser" && zapret?.gui_running) {
      edges.push({ from: [xApp + 8, y], to: [xAd - 8, adY(3)], color: "#5fd18c", active: running, dim: true });
    }
  });
  // adapters → destinations
  const adEdges: Edge[] = [
    { from: [xAd + 8, adY(0)], to: [xDst - 8, dsts[0].y], color: "#35d0c6", active: uplinkUp },
    { from: [xAd + 8, adY(1)], to: [xDst - 8, dsts[1].y], color: "#9085e9", active: happActive },
    { from: [xAd + 8, adY(3)], to: [xDst - 8, dsts[1].y], color: "#5fd18c", active: !!zapret?.gui_running, dim: true },
    { from: [xAd + 8, adY(2)], to: [xDst - 8, dsts[2].y], color: "#f2b33d", active: radminUp },
  ];

  const path = (e: Edge) => {
    const [x1, y1] = e.from;
    const [x2, y2] = e.to;
    const mx = (x1 + x2) / 2;
    return `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`;
  };

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="w-full h-auto" role="img" aria-label="Карта маршрутов приложений">
      <defs>
        <filter id="glow">
          <feGaussianBlur stdDeviation="2.5" result="b" />
          <feMerge>
            <feMergeNode in="b" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      {/* column headers */}
      <text x={xApp} y={12} textAnchor="end" className="fill-ink-3" fontSize="10" letterSpacing="1.5">
        ПРИЛОЖЕНИЯ
      </text>
      <text x={xAd} y={12} textAnchor="middle" className="fill-ink-3" fontSize="10" letterSpacing="1.5">
        КАНАЛЫ
      </text>
      <text x={xDst} y={12} textAnchor="start" className="fill-ink-3" fontSize="10" letterSpacing="1.5">
        НАЗНАЧЕНИЕ
      </text>

      {[...edges, ...adEdges].map((e, i) => (
        <g key={i}>
          <path d={path(e)} fill="none" stroke={e.color} strokeOpacity={e.active ? 0.9 : e.dim ? 0.12 : 0.22} strokeWidth={e.active ? 1.8 : 1.2} className={e.active ? "flow" : ""} filter={e.active ? "url(#glow)" : undefined} />
        </g>
      ))}

      {apps.map((a, i) => {
        const y = 30 + i * rowH;
        const st = status.find((s) => s.id === a.id);
        const running = !!st?.running;
        return (
          <g key={a.id}>
            <circle cx={xApp} cy={y} r={5} fill={running ? a.color : "#0b0e13"} stroke={a.color} strokeWidth={1.5} />
            <text x={xApp - 14} y={y + 4} textAnchor="end" fontSize="12.5" className={running ? "fill-ink" : "fill-ink-2"} fontWeight={running ? 600 : 400}>
              {a.name}
            </text>
            <text x={xApp - 14} y={y + 17} textAnchor="end" fontSize="10" className="fill-ink-3">
              {a.policy === "direct" ? "напрямую" : a.policy === "vpn" ? "через Happ" : "Radmin приоритет"}
              {running && st ? ` · ${st.procs.length} проц.` : ""}
            </text>
          </g>
        );
      })}

      {adapters.map((ad, i) => (
        <g key={ad.id}>
          <rect x={xAd - 62} y={adY(i) - 17} width={124} height={34} rx={8} fill="#121721" stroke={ad.up ? ad.color : "#222c3d"} strokeWidth={1.2} />
          <circle cx={xAd - 50} cy={adY(i)} r={3.5} fill={ad.up ? ad.color : "#2d3a4f"} />
          <text x={xAd - 40} y={adY(i) - 2} fontSize="12" fontWeight={600} className={ad.up ? "fill-ink" : "fill-ink-2"}>
            {ad.label}
          </text>
          <text x={xAd - 40} y={adY(i) + 11} fontSize="9.5" className="fill-ink-3">
            {ad.sub.length > 22 ? ad.sub.slice(0, 22) + "…" : ad.sub}
          </text>
        </g>
      ))}

      {dsts.map((d) => (
        <g key={d.id}>
          <circle cx={xDst} cy={d.y} r={5} fill="#0b0e13" stroke="#5f6b80" strokeWidth={1.5} />
          <text x={xDst + 14} y={d.y + 4} fontSize="12.5" className="fill-ink">
            {d.label}
          </text>
          <text x={xDst + 14} y={d.y + 17} fontSize="10" className="fill-ink-3">
            {d.sub}
          </text>
        </g>
      ))}
    </svg>
  );
}
