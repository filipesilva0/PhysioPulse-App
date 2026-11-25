import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { ArrowDown, ArrowDownRight, ArrowUp, ArrowUpRight, CalendarDays } from "lucide-react";
import { useApp } from "../lib/ctx";
import { isErr } from "../lib/api";

/** The PhysioPulse mark (12 Nov 2026, brand/): a pulse line whose beat rises into the fingertips of an open hand,
 *  teal on a paper tile — the same as the app icon (colourway B). */
const MARK_D = "M-120 300 H150 C165 300 168 262 182 262 C196 262 200 338 212 338 C224 338 230.0 300 230.0 246 L230.0 178 A20.0 20.0 0 0 1 270.0 178 L270.0 236 A3.0 3.0 0 0 0 276.0 236 L276.0 132 A20.0 20.0 0 0 1 316.0 132 L316.0 236 A3.0 3.0 0 0 0 322.0 236 L322.0 150 A20.0 20.0 0 0 1 362.0 150 L362.0 236 A2.0 2.0 0 0 0 366.0 236 L366.0 196 A20.0 20.0 0 0 1 406.0 196 L406.0 250 C406.0 300 412.0 326 426.0 326 C440.0 326 440.0 274 454.0 274 C468.0 274 468.0 300 486.0 300 H780";
export function Mark({ className = "mark" }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 1024 1024" aria-hidden="true">
      <rect width="1024" height="1024" rx="230" fill="#F3F1EC" />
      <path transform="translate(-219.4 -12.4) scale(2.3)" d={MARK_D} fill="none" stroke="#0B7A70" strokeWidth="27.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** The mark's line alone, running edge to edge (the sign-in panel and the phone's lock screen). Colour = currentColor. */
export function PulseLine({ className = "pulse-line", width = 14 }: { className?: string; width?: number }) {
  return (
    <svg className={className} viewBox="-120 80 900 300" preserveAspectRatio="xMidYMid meet" aria-hidden="true">
      <path d={MARK_D} fill="none" stroke="currentColor" strokeWidth={width} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** The sign-in surface (owner, 12 Nov 2026): a paper sheet with the pulse-hand line cut out of it, so the brand
 *  teal behind shows through the cut, with a small shadow inside it like a real die-cut. `band`: where the line
 *  sits, as top and height in % of the sheet. The line runs off both edges. */
const CUT_D = MARK_D.replace(/^M-?\d+(\.\d+)? 300/, "M-3000 300").replace(/H780$/, "H3900");
export function CutoutSheet({ band = [6, 30], id = "pp-cut", stroke = 18 }: { band?: [number, number]; id?: string; stroke?: number }) {
  return (
    <svg className="cut-sheet" width="100%" height="100%" aria-hidden="true">
      <defs>
        <mask id={id} maskUnits="userSpaceOnUse" x="0" y="0" width="100%" height="100%">
          <rect width="100%" height="100%" fill="#fff" />
          <svg x="0" y={`${band[0]}%`} width="100%" height={`${band[1]}%`} viewBox="-120 80 900 300" preserveAspectRatio="xMidYMid meet" overflow="visible">
            <path d={CUT_D} fill="none" stroke="#000" strokeWidth={stroke} strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </mask>
        <filter id={`${id}-sh`} x="-5%" y="-5%" width="110%" height="110%">
          <feDropShadow dx="0" dy="3" stdDeviation="2" floodColor="#062F2B" floodOpacity="0.55" />
        </filter>
      </defs>
      <g filter={`url(#${id}-sh)`}><rect className="cut-paper" width="100%" height="100%" mask={`url(#${id})`} /></g>
    </svg>
  );
}

export function Avatar({ name, color = "var(--a)", size = "" }: { name: string; color?: string; size?: "" | "md" | "lg" }) {
  const ini = name.split(" ").map((w) => w[0]).join("").slice(0, 2);
  return <span className={`avatar ${size}`} style={{ ["--c" as string]: color }} aria-hidden="true">{ini}</span>;
}

export function Seg<T extends string>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map(([v, t]) => (
        <button key={v} aria-pressed={value === v} onClick={() => onChange(v)}>{t}</button>
      ))}
    </div>
  );
}

export function Switch({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return <button className="switch" role="switch" aria-checked={on} aria-label={label} onClick={() => onChange(!on)} />;
}

export function Badge({ tone = "n", children }: { tone?: "ok" | "warn" | "bad" | "a" | "n" | "ai"; children: ReactNode }) {
  return <span className={`badge b-${tone}`}>{children}</span>;
}

export function Empty({ icon, title, text, action }: { icon: ReactNode; title: string; text?: string; action?: ReactNode }) {
  return (
    <div className="empty">
      {icon}
      <h2>{title}</h2>
      {text && <p className="small">{text}</p>}
      {action}
    </div>
  );
}

export function Stat({ label, value, note, tone, icon, hl }: { label: string; value: string; note?: string; tone?: "up" | "down"; icon?: ReactNode; hl?: boolean }) {
  return (
    <div className={`stat ${hl ? "hl" : ""}`}>
      <div className="top">{icon && <span className="ico">{icon}</span>}<span className="l">{label}</span></div>
      <div className="v">{value}</div>
      {note && <div className={`d ${hl ? "" : tone ?? ""}`}>{tone === "up" ? <ArrowUpRight /> : tone === "down" ? <ArrowDownRight /> : null}{note}</div>}
    </div>
  );
}

/** Week day strip with appointment dots (reference: phone and card examples). */
export function DayStrip({ days, value, onChange }: { days: { label: string; num: number; dots: number }[]; value: number; onChange: (i: number) => void }) {
  return (
    <div className="daystrip" role="group">
      {days.map((d, i) => (
        <button key={i} aria-pressed={value === i} onClick={() => onChange(i)} aria-label={`${d.label} ${d.num}`}>
          <span className="dn">{d.label}</span>
          <span className="dd">{d.num}</span>
          <span className="dots">{Array.from({ length: Math.min(3, d.dots) }, (_, k) => <i key={k} />)}</span>
        </button>
      ))}
    </div>
  );
}

export function Head({ title, sub, children }: { title: ReactNode; sub?: ReactNode; children?: ReactNode }) {
  return (
    <div className="head">
      <div>
        <h1>{title}</h1>
        {sub && <p className="sub">{sub}</p>}
      </div>
      {children && <div className="head-actions">{children}</div>}
    </div>
  );
}

/** macOS-style table: click a column heading to sort, again to reverse; alternating rows. */
export interface Col<R> { key: string; label: string; right?: boolean; sort?: (r: R) => string | number; cell: (r: R) => ReactNode; }
export function SortTable<R>({ cols, rows, initial }: { cols: Col<R>[]; rows: R[]; initial?: string }) {
  const [by, setBy] = useState<string | undefined>(initial);
  const [desc, setDesc] = useState(true);
  const sorted = useMemo(() => {
    const c = cols.find((x) => x.key === by);
    if (!c?.sort) return rows;
    const s = [...rows].sort((a, b) => {
      const x = c.sort!(a), y = c.sort!(b);
      return x < y ? -1 : x > y ? 1 : 0;
    });
    return desc ? s.reverse() : s;
  }, [rows, by, desc, cols]);
  return (
    <div className="tbl-wrap">
      <table>
        <thead>
          <tr>
            {cols.map((c) => (
              <th key={c.key} className={c.right ? "r" : ""} aria-sort={by === c.key ? (desc ? "descending" : "ascending") : undefined}>
                {c.sort ? (
                  <button onClick={() => (by === c.key ? setDesc(!desc) : (setBy(c.key), setDesc(true)))}>
                    {c.label}
                    {by === c.key && (desc ? <ArrowDown size={12} /> : <ArrowUp size={12} />)}
                  </button>
                ) : (
                  <span style={{ display: "block", padding: "8px 14px" }}>{c.label}</span>
                )}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((r, i) => (
            <tr key={i}>
              {cols.map((c) => <td key={c.key} className={c.right ? "r" : ""}>{c.cell(r)}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** How long a delete button has to be held (owner, 8 Nov 2026: "everything that's deletable should be a hold button, 5 sec"). */
export const HOLD_MS = 5000;

/** A delete button that only fires after it has been held down for 5 seconds (mouse, touch, Space or Enter).
 *  Red fill grows while held; letting go early cancels and says how it works. */
export function HoldButton({ label, onDone, disabled, wide = true, className = "" }: {
  label: string; onDone: () => unknown; disabled?: boolean; wide?: boolean; className?: string;
}) {
  const { L } = useApp();
  const [p, setP] = useState(0);
  const [hint, setHint] = useState(false);
  const [busy, setBusy] = useState(false);
  const start = useRef<number | null>(null);
  const raf = useRef(0);
  const hintT = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => { cancelAnimationFrame(raf.current); clearTimeout(hintT.current); }, []);
  const tick = () => {
    if (start.current == null) return;
    const v = Math.min(1, (performance.now() - start.current) / HOLD_MS);
    setP(v);
    if (v >= 1) {
      start.current = null;
      setBusy(true);
      Promise.resolve(onDone()).finally(() => { setBusy(false); setP(0); });
      return;
    }
    raf.current = requestAnimationFrame(tick);
  };
  const down = () => {
    if (disabled || busy || start.current != null) return;
    setHint(false);
    start.current = performance.now();
    raf.current = requestAnimationFrame(tick);
  };
  const up = () => {
    if (start.current == null) return;
    const early = performance.now() - start.current < HOLD_MS;
    start.current = null;
    cancelAnimationFrame(raf.current);
    setP(0);
    if (early) { setHint(true); clearTimeout(hintT.current); hintT.current = setTimeout(() => setHint(false), 2600); }
  };
  const left = Math.ceil((1 - p) * HOLD_MS / 1000);
  const holding = p > 0 && p < 1;
  const note = holding ? L(`Continue a premir… ${left}`, `Keep holding… ${left}`) : hint ? L("Mantenha premido 5 segundos", "Hold for 5 seconds") : "";
  const shown = wide && note ? note : label;
  return (
    <span className={`hold-wrap ${wide ? "wide" : ""}`}>
      <button type="button" className={`btn danger hold ${wide ? "wide" : ""} ${holding ? "on" : ""} ${className}`} disabled={disabled || busy} aria-busy={busy}
        style={{ ["--p" as string]: p }}
        aria-label={`${label}. ${L("Mantenha premido 5 segundos.", "Hold for 5 seconds.")}`}
        onPointerDown={(e) => { if (e.button === 0) { (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId); down(); } }}
        onPointerUp={up} onPointerCancel={up} onLostPointerCapture={up}
        onKeyDown={(e) => { if ((e.key === " " || e.key === "Enter") && !e.repeat) { e.preventDefault(); e.stopPropagation(); down(); } }}
        onKeyUp={(e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); up(); } }}
        onBlur={up} onContextMenu={(e) => e.preventDefault()}>
        <span className="hold-label">{shown}</span>
        <span className="hold-fill" aria-hidden="true">{shown}</span>
      </button>
      {/* Full-width buttons (alerts) say it in the button itself; others show a small note above. */}
      {!wide && <span className={`hold-hint ${note ? "show" : ""}`} aria-hidden="true">{note}</span>}
      <span className="sr-only" role="status" aria-live="polite">{hint ? L("Mantenha premido 5 segundos para apagar.", "Hold for 5 seconds to delete.") : ""}</span>
    </span>
  );
}

/** Sheet: modal to the window, Cancel + one action, Esc cancels.
 *  onAction may be async: the sheet stays open (button busy) until it finishes, closes on success,
 *  and shows the server's message inline if it fails. Return false to keep the sheet open. */
export function Sheet({ title, sub, children, action, onAction, wide, destructive, disabled, hold }: {
  title: string; sub?: string; children: ReactNode; action?: string; onAction?: () => unknown; wide?: boolean; destructive?: boolean; disabled?: boolean;
  /** A delete: the action button has to be held for 5 seconds (HoldButton). */
  hold?: boolean;
}) {
  const { closeSheet, cancelSheet, L } = useApp();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | undefined>();
  const run = async () => {
    setErr(undefined);
    try {
      const r = onAction?.();
      if (r instanceof Promise) { setBusy(true); if ((await r) === false) return; }
      else if (r === false) return;
      closeSheet();
    } catch (e) {
      setErr(isErr(e) ? e.message : String(e));
    } finally { setBusy(false); }
  };
  return (
    <div className={`sheet ${wide ? "wide" : ""}`} role="dialog" aria-modal="true" aria-label={title}>
      <div className="sheet-body">
        <h2>{title}</h2>
        {sub && <p className="sub small" style={{ marginBottom: 14 }}>{sub}</p>}
        {!sub && <div style={{ height: 10 }} />}
        {children}
        {err && <div className="err" role="alert" style={{ marginTop: 8 }}>{err}</div>}
      </div>
      <div className="sheet-foot">
        <button className="btn" onClick={cancelSheet}>{action ? L("Cancelar", "Cancel") : L("Fechar", "Close")}</button>
        {action && hold && <HoldButton label={action} onDone={run} disabled={disabled || busy} wide={false} />}
        {action && !hold && (
          <button className={`btn pri ${destructive ? "danger" : ""}`} disabled={disabled || busy} aria-busy={busy} onClick={run}>{action}</button>
        )}
      </div>
    </div>
  );
}

export function Field({ label, children, error }: { label: string; children: ReactNode;
  /** An inline message under the control that says how to fix it (audit M11). */
  error?: string }) {
  return (
    <label className={`field ${error ? "has-err" : ""}`}>
      <span>{label}</span>
      {children}
      {error && <small className="field-err" role="alert">{error}</small>}
    </label>
  );
}

export function Sparkline({ pts, max, label }: { pts: number[]; max: number; label: string }) {
  const w = 280, h = 64;
  const x = (i: number) => 10 + (i * (w - 20)) / Math.max(1, pts.length - 1);
  const y = (v: number) => h - 8 - (v / max) * (h - 16);
  return (
    <svg viewBox={`0 0 ${w} ${h}`} style={{ width: "100%", height: 64 }} role="img" aria-label={label}>
      <path d={pts.map((v, i) => `${i ? "L" : "M"}${x(i)} ${y(v)}`).join(" ")} fill="none" stroke="var(--a)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
      {pts.map((v, i) => <circle key={i} cx={x(i)} cy={y(v)} r="3.5" fill="var(--a)" />)}
    </svg>
  );
}

/** Small revenue vs expenses bar chart (SVG, theme colors). */
/** Two-series bar chart. `partial` marks the last month as still in progress (lighter bars and a note). */
export function Bars({ data, labelA, labelB, partial }: { data: { m: string; a: number; b: number }[]; labelA: string; labelB: string; partial?: string }) {
  const W = 560, H = 190, bw = 26;
  // A round top value (1, 2 or 5 × 10ⁿ) just above the largest bar, split into 4 grid lines.
  const top = Math.max(1, ...data.flatMap((d) => [d.a, d.b]));
  const step = (() => { const raw = top / 4, p = 10 ** Math.floor(Math.log10(raw)); return [1, 2, 2.5, 5, 10].map((k) => k * p).find((s) => s >= raw)!; })();
  const max = step * 4;
  const fmt = (v: number) => (v >= 1000 ? `${Math.round((v / 1000) * 10) / 10}k` : `${v}`);
  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H + 24}`} style={{ width: "100%", height: "auto", display: "block" }} role="img" aria-label={`${labelA} / ${labelB}`}>
        {[0, step, step * 2, step * 3, max].map((v) => (
          <g key={v}>
            <line x1="40" x2={W} y1={H - (v / max) * H + 4} y2={H - (v / max) * H + 4} stroke="var(--line)" />
            <text x="34" y={H - (v / max) * H + 8} textAnchor="end" fontSize="11" fill="var(--text3)">{fmt(v)}</text>
          </g>
        ))}
        {data.map((d, i) => {
          const x = 60 + i * ((W - 70) / data.length);
          return (
            <g key={i} opacity={partial && i === data.length - 1 ? 0.45 : 1}>
              <rect x={x} y={H - (d.a / max) * H + 4} width={bw} height={(d.a / max) * H} rx="3" fill="var(--a)" />
              <rect x={x + bw + 4} y={H - (d.b / max) * H + 4} width={bw} height={(d.b / max) * H} rx="3" fill="var(--text3)" opacity=".55" />
              <text x={x + bw + 2} y={H + 20} textAnchor="middle" fontSize="11.5" fill="var(--text2)">{d.m}</text>
            </g>
          );
        })}
      </svg>
      <div className="legend">
        <span><i style={{ background: "var(--a)" }} />{labelA}</span>
        <span><i style={{ background: "var(--text3)", opacity: 0.55 }} />{labelB}</span>
        {partial && <span className="muted">{partial}</span>}
      </div>
    </div>
  );
}

/** A decorative stand-in for a Stripe payment QR code (not scannable). */
export function FakeQR({ size = 148 }: { size?: number }) {
  const n = 25;
  const cells: [number, number][] = [];
  let s = 7;
  for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    const finder = (x < 7 && y < 7) || (x > n - 8 && y < 7) || (x < 7 && y > n - 8);
    if (!finder && s % 3 === 0) cells.push([x, y]);
  }
  const finder = (fx: number, fy: number) => (
    <g key={`${fx}-${fy}`}>
      <rect x={fx} y={fy} width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" transform="translate(.5 .5)" />
      <rect x={fx + 2} y={fy + 2} width="3" height="3" fill="currentColor" />
    </g>
  );
  return (
    <svg viewBox={`-1 -1 ${n + 2} ${n + 2}`} width={size} height={size} role="img" aria-label="Payment QR code (sample)" style={{ color: "var(--text)", background: "var(--raise)", borderRadius: 8 }}>
      {cells.map(([x, y]) => <rect key={`${x}.${y}`} x={x} y={y} width="1" height="1" fill="currentColor" />)}
      {finder(0, 0)}{finder(n - 7, 0)}{finder(0, n - 7)}
    </svg>
  );
}

// ───────── Dates and times ─────────
// Native <input type="date|time"> follow the computer's region (a Windows set to English (US) shows
// "11/09/2026" for 9 November and "05:00 PM"). The clinic is in Portugal, so dates are always
// dd/mm/aaaa and times 24-hour, in both languages. Values stay ISO: "2026-11-09" and "17:00".

/** The change handler gets `{ target: { value } }`, like an input's, so existing handlers keep working. */
type ValueChange = { onChange(e: { target: { value: string } }): void };

const pad = (n: number) => String(n).padStart(2, "0");
/** "2026-11-09" → "09/11/2026" */
export const dmy = (iso: string) => { const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso); return m ? `${m[3]}/${m[2]}/${m[1]}` : ""; };
/** "9/11/2026", "09-11-26", "9.11.2026", "09112026" → "2026-11-09"; null if it isn't a real date. */
export function parseDmy(text: string): string | null {
  const t = text.trim();
  const m = /^(\d{1,2})[/.\- ](\d{1,2})[/.\- ](\d{2}|\d{4})$/.exec(t) ?? /^(\d{2})(\d{2})(\d{4})$/.exec(t);
  if (!m) return null;
  const d = +m[1], mo = +m[2];
  let y = +m[3];
  if (m[3].length === 2) y += y > new Date().getFullYear() % 100 + 10 ? 1900 : 2000;
  const dt = new Date(Date.UTC(y, mo - 1, d));
  if (dt.getUTCFullYear() !== y || dt.getUTCMonth() !== mo - 1 || dt.getUTCDate() !== d) return null;
  return `${y}-${pad(mo)}-${pad(d)}`;
}
const addDays = (iso: string, n: number) => { const [y, m, d] = iso.split("-").map(Number); const t = new Date(Date.UTC(y, m - 1, d + n)); return `${t.getUTCFullYear()}-${pad(t.getUTCMonth() + 1)}-${pad(t.getUTCDate())}`; };

/** A date field that always reads dd/mm/aaaa. Type the date (↑/↓ move a day) or pick it from the calendar button. */
export function DateInput({ value, onChange, min, max, className = "input", style, ...rest }: ValueChange & {
  value: string; min?: string; max?: string; className?: string; style?: CSSProperties; "aria-label"?: string; id?: string; disabled?: boolean;
}) {
  const { L } = useApp();
  const [text, setText] = useState(dmy(value));
  const [bad, setBad] = useState(false);
  const native = useRef<HTMLInputElement>(null);
  const badRef = useRef(false);
  // A wrong date clears the value (so the sheet's main button turns off) but keeps what was typed on screen.
  useEffect(() => { if (badRef.current && !value) return; setText(dmy(value)); setBad(false); }, [value]);
  const inRange = (iso: string) => (!min || iso >= min) && (!max || iso <= max);
  const commit = (iso: string | null) => {
    if (iso && inRange(iso)) { badRef.current = false; setBad(false); setText(dmy(iso)); if (iso !== value) onChange({ target: { value: iso } }); }
    else if (!text.trim() && !iso) { badRef.current = false; setBad(false); if (value) onChange({ target: { value: "" } }); }
    else { badRef.current = true; setBad(true); if (value) onChange({ target: { value: "" } }); }
  };
  const pick = () => { const n = native.current; if (!n) return; try { n.showPicker(); } catch { n.focus(); n.click(); } };
  const label = rest["aria-label"];
  return (
    <span className={`dt-field ${className}`} style={style} data-bad={bad || undefined}>
      <input className="dt-text" inputMode="numeric" autoComplete="off" spellCheck={false} placeholder={L("dd/mm/aaaa", "dd/mm/yyyy")}
        value={text} id={rest.id} disabled={rest.disabled} aria-label={label} aria-invalid={bad || undefined}
        title={bad ? (min || max ? L("Escreva uma data válida dentro do intervalo permitido, como 09/11/2026.", "Type a valid date within the allowed range, like 09/11/2026.") : L("Escreva a data como 09/11/2026.", "Type the date like 09/11/2026.")) : undefined}
        onChange={(e) => { setText(e.target.value); setBad(false); badRef.current = false; }}
        onBlur={() => commit(text.trim() ? parseDmy(text) : null)}
        onKeyDown={(e) => {
          if (e.key === "Enter") { commit(text.trim() ? parseDmy(text) : null); }
          else if ((e.key === "ArrowUp" || e.key === "ArrowDown") && !e.altKey) {
            e.preventDefault();
            const base = parseDmy(text) ?? value ?? "";
            const today = new Date(); const t = `${today.getFullYear()}-${pad(today.getMonth() + 1)}-${pad(today.getDate())}`;
            commit(addDays(base || t, base ? (e.key === "ArrowUp" ? 1 : -1) : 0));
          } else if (e.key === "ArrowDown" && e.altKey) { e.preventDefault(); pick(); }
        }} />
      <button type="button" className="dt-btn" tabIndex={-1} disabled={rest.disabled} onClick={pick} aria-label={L("Escolher no calendário", "Choose from Calendar")}><CalendarDays size={14} aria-hidden="true" /></button>
      <input ref={native} type="date" className="dt-native" tabIndex={-1} aria-hidden="true" value={value} min={min} max={max}
        onChange={(e) => { if (e.target.value) commit(e.target.value); }} />
    </span>
  );
}

/** A time pop-up (24-hour, every `step` seconds, 07:00–22:00 plus the current value if it falls outside). */
export function TimeInput({ value, onChange, step = 900, className = "input", style, from = 7, to = 22, ...rest }: ValueChange & {
  value: string; step?: number; className?: string; style?: CSSProperties; from?: number; to?: number; "aria-label"?: string; disabled?: boolean;
}) {
  const { L } = useApp();
  const opts = useMemo(() => {
    const mins = Math.max(5, Math.round(step / 60));
    const out: string[] = [];
    for (let m = from * 60; m <= to * 60; m += mins) out.push(`${pad(Math.floor(m / 60))}:${pad(m % 60)}`);
    const v = value.slice(0, 5);
    if (v && !out.includes(v)) { out.push(v); out.sort(); }
    return out;
  }, [step, from, to, value]);
  return (
    <select className={`${className} tnum`} style={style} value={value.slice(0, 5)} aria-label={rest["aria-label"]} disabled={rest.disabled}
      onChange={(e) => onChange({ target: { value: e.target.value } })}>
      {!value && <option value="">—</option>}
      {opts.map((t) => <option key={t} value={t}>{t}</option>)}
    </select>
  );
}
