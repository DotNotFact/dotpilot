import type { ReactNode } from "react";

export function Section({ title, sub, right, children, className = "" }: { title: string; sub?: string; right?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`panel p-4 ${className}`}>
      <div className="flex items-start justify-between gap-3 mb-3">
        <div>
          <h2 className="text-[14.5px]">{title}</h2>
          {sub && <div className="text-[12px] text-ink-2 mt-0.5">{sub}</div>}
        </div>
        {right}
      </div>
      {children}
    </section>
  );
}

export function Tile({ label, value, unit, hint, color, children }: { label: string; value: ReactNode; unit?: string; hint?: ReactNode; color?: string; children?: ReactNode }) {
  return (
    <div className="panel-2 px-3.5 py-3 min-w-0">
      <div className="eyebrow">{label}</div>
      <div className="flex items-baseline gap-1 mt-1">
        <span className="num text-[22px] leading-none font-medium" style={{ color }}>
          {value}
        </span>
        {unit && <span className="text-[11.5px] text-ink-3">{unit}</span>}
      </div>
      {hint && <div className="text-[11.5px] text-ink-2 mt-1 truncate">{hint}</div>}
      {children}
    </div>
  );
}

export function Switch({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return <button type="button" role="switch" aria-checked={on} className="switch" data-on={on} disabled={disabled} onClick={() => onChange(!on)} />;
}

export function Tag({ children, color = "var(--color-ink-3)", bg }: { children: ReactNode; color?: string; bg?: string }) {
  return (
    <span className="tag" style={{ color, background: bg ?? `color-mix(in srgb, ${color} 14%, transparent)` }}>
      {children}
    </span>
  );
}

export function StatusPill({ ok, warn, text }: { ok: boolean; warn?: boolean; text: string }) {
  const c = ok ? "var(--color-mint)" : warn ? "var(--color-amber)" : "var(--color-ink-3)";
  return (
    <span className="inline-flex items-center gap-1.5 text-[12px]">
      <span className={`dot ${ok ? "dot-live" : ""}`} style={{ background: c }} />
      {text}
    </span>
  );
}

export function Bar({ value, max = 100, color = "var(--color-teal)", height = 6 }: { value: number; max?: number; color?: string; height?: number }) {
  const pct = Math.max(0, Math.min(100, (value / max) * 100));
  return (
    <div className="w-full rounded-full bg-line overflow-hidden" style={{ height }}>
      <div className="h-full rounded-full transition-[width] duration-500" style={{ width: `${pct}%`, background: color }} />
    </div>
  );
}

/**
 * Hover hint. Wrap any control: `<Tip title="…" text="…" rec="…">…</Tip>`.
 * `mark` adds a small "?" badge (for labels). `side="right"` anchors the box to the right edge.
 */
export function Tip({ title, text, rec, warn, children, mark, side, up, className = "" }: { title: string; text: string; rec?: string; warn?: string; children?: ReactNode; mark?: boolean; side?: "left" | "right"; up?: boolean; className?: string }) {
  return (
    <span className={`tip ${side === "right" ? "tip-right" : ""} ${up ? "tip-up" : ""} ${className}`} tabIndex={mark ? 0 : undefined}>
      {children}
      {mark && <span className="hint-mark">?</span>}
      <span className="tip-box" role="tooltip">
        <b>{title}</b>
        <span>{text}</span>
        {warn && <span className="warn">{warn}</span>}
        {rec && <em>Совет: {rec}</em>}
      </span>
    </span>
  );
}

/** Eyebrow label with a "?" hint. */
export function Label({ children, title, text, rec, warn }: { children: ReactNode; title: string; text: string; rec?: string; warn?: string }) {
  return (
    <Tip title={title} text={text} rec={rec} warn={warn} mark>
      <span className="eyebrow">{children}</span>
    </Tip>
  );
}

/** Minimal Markdown renderer (headings, lists, code, bold, inline code, hr). */
export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r/g, "").split("\n");
  const out: ReactNode[] = [];
  let i = 0;
  let key = 0;
  const inline = (s: string): ReactNode[] => {
    const parts: ReactNode[] = [];
    const re = /(`[^`]+`|\*\*[^*]+\*\*|_[^_]+_)/g;
    let last = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(s))) {
      if (m.index > last) parts.push(s.slice(last, m.index));
      const tok = m[0];
      if (tok.startsWith("`")) parts.push(<code key={key++}>{tok.slice(1, -1)}</code>);
      else if (tok.startsWith("**")) parts.push(<strong key={key++}>{tok.slice(2, -2)}</strong>);
      else parts.push(<em key={key++}>{tok.slice(1, -1)}</em>);
      last = m.index + tok.length;
    }
    if (last < s.length) parts.push(s.slice(last));
    return parts;
  };
  while (i < lines.length) {
    const l = lines[i];
    if (l.startsWith("```")) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++;
      out.push(
        <pre key={key++}>
          <code>{buf.join("\n")}</code>
        </pre>,
      );
      continue;
    }
    const h = /^(#{1,3})\s+(.*)/.exec(l);
    if (h) {
      const T = h[1].length === 1 ? "h1" : h[1].length === 2 ? "h2" : "h3";
      out.push(<T key={key++}>{inline(h[2])}</T>);
      i++;
      continue;
    }
    if (/^\s*[-*]\s+/.test(l)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*[-*]\s+/, ""));
      out.push(<ul key={key++}>{items.map((it, j) => <li key={j}>{inline(it)}</li>)}</ul>);
      continue;
    }
    if (/^\s*\d+[.)]\s+/.test(l)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+[.)]\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*\d+[.)]\s+/, ""));
      out.push(<ol key={key++}>{items.map((it, j) => <li key={j}>{inline(it)}</li>)}</ol>);
      continue;
    }
    if (/^---+$/.test(l.trim())) {
      out.push(<hr key={key++} />);
      i++;
      continue;
    }
    if (l.trim() === "") {
      i++;
      continue;
    }
    const buf: string[] = [l];
    i++;
    while (i < lines.length && lines[i].trim() !== "" && !/^(#{1,3}\s|```|\s*[-*]\s|\s*\d+[.)]\s|---)/.test(lines[i])) buf.push(lines[i++]);
    out.push(<p key={key++}>{inline(buf.join(" "))}</p>);
  }
  return <div className="md selectable text-[13px]">{out}</div>;
}
