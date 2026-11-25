// The schedule's time grid, shared by the day view (one column per therapist) and the week view (one column per day).
// Click a free time to book it; drag a session to move it (to another time, therapist or day); drag its bottom edge
// to change its length (audit M4, M5, M6). Blocked time sits behind the sessions, and every card has the same layout.
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Home, Repeat, Users } from "lucide-react";
import { useApp } from "../lib/ctx";
import { hm, hourOf, pad, kindLabel, type Appointment } from "../lib/data";
import { apptItems, openMenu } from "./menus";

export interface GridCol {
  key: string;
  /** The day this column shows (midnight, local time). */
  date: Date;
  /** Bookings made by clicking here go to this therapist (day view with one column per therapist). */
  therapistId?: string | null;
  appts: Appointment[];
  /** Opening hours of that day as [open, close] in hours (e.g. [8, 20]); null = closed all day. */
  open: [number, number] | null;
  head?: ReactNode;
  headLabel?: string;
  onHead?: () => void;
  isToday: boolean;
}

/** Hours to show: the opening hours of the days shown and every appointment in them, whole hours (audit M5). */
export function gridRange(cols: GridCol[]): [number, number] {
  let s = 24, e = 0;
  for (const c of cols) {
    if (c.open) { s = Math.min(s, Math.floor(c.open[0])); e = Math.max(e, Math.ceil(c.open[1])); }
    for (const a of c.appts) {
      const sd = new Date(a.starts_at), ed = new Date(a.ends_at);
      s = Math.min(s, Math.floor(hourOf(a.starts_at)));
      // Ends at or after midnight → the end of the day.
      e = Math.max(e, ed.getDate() !== sd.getDate() ? 24 : Math.ceil(hourOf(a.ends_at)));
    }
  }
  if (s >= e) { s = 8; e = 20; }
  if (e - s < 4) e = Math.min(24, s + 4);
  return [Math.max(0, s), Math.min(24, e)];
}

/** "08:00" → 8, "13:30" → 13.5 */
export const hNum = (t: string) => { const [h, m] = t.split(":").map(Number); return h + (m || 0) / 60; };

/** Side-by-side lanes for sessions that overlap, including chains (A overlaps B, B overlaps C). */
function lanes(list: Appointment[]) {
  const out = new Map<string, { lane: number; of: number }>();
  const sorted = [...list].sort((a, b) => a.starts_at.localeCompare(b.starts_at) || b.ends_at.localeCompare(a.ends_at));
  let cluster: string[] = [], ends: number[] = [], clusterEnd = -Infinity;
  const close = () => { for (const id of cluster) out.get(id)!.of = ends.length; cluster = []; ends = []; };
  for (const a of sorted) {
    const s = new Date(a.starts_at).getTime(), e = new Date(a.ends_at).getTime();
    if (s >= clusterEnd) { close(); clusterEnd = -Infinity; }
    let lane = ends.findIndex((x) => x <= s);
    if (lane < 0) { lane = ends.length; ends.push(e); } else ends[lane] = e;
    out.set(a.id, { lane, of: 0 });
    cluster.push(a.id);
    clusterEnd = Math.max(clusterEnd, e);
  }
  close();
  return out;
}

const SNAP = 15;
type Drag = { a: Appointment; mode: "move" | "resize"; x0: number; y0: number; col0: number; on: boolean; col: number; start: Date; end: Date };

export function TimeGrid({ cols, H, compactWeek, sel, onSel, title, color, status, onBook, onMove, canDrag }: {
  cols: GridCol[]; H: number; compactWeek?: boolean; sel: string | null; onSel: (id: string) => void;
  title: (a: Appointment) => string; color: (a: Appointment) => string; status: (a: Appointment) => string;
  onBook: (at: Date, therapistId?: string | null) => void;
  /** A finished drag: the new start and end, and the column it was dropped in. */
  onMove: (a: Appointment, start: Date, end: Date, col: GridCol) => void;
  canDrag: (a: Appointment) => boolean;
}) {
  const app = useApp();
  const { L, lang } = app;
  const [start, end] = gridRange(cols);
  const colRefs = useRef<(HTMLDivElement | null)[]>([]);
  const [ghost, setGhost] = useState<{ col: number; min: number } | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const justDragged = useRef(false);
  const [now, setNow] = useState(() => new Date());
  useEffect(() => { const t = setInterval(() => setNow(new Date()), 60_000); return () => clearInterval(t); }, []);
  const nowH = now.getHours() + now.getMinutes() / 60;
  const total = (end - start) * H;

  /** Minutes since midnight at a y position inside a column, snapped to 15 minutes. */
  const minAt = (col: number, clientY: number, round: "floor" | "round" = "floor") => {
    const r = colRefs.current[col]?.getBoundingClientRect();
    if (!r) return start * 60;
    const m = start * 60 + ((clientY - r.top) / H) * 60;
    const v = (round === "floor" ? Math.floor(m / SNAP) : Math.round(m / SNAP)) * SNAP;
    return Math.max(start * 60, Math.min(end * 60 - SNAP, v));
  };
  const colAt = (clientX: number, fallback: number) => {
    const i = colRefs.current.findIndex((el) => { const r = el?.getBoundingClientRect(); return !!r && clientX >= r.left && clientX < r.right; });
    return i < 0 ? fallback : i;
  };
  const at = (col: number, min: number) => { const d = new Date(cols[col].date); d.setHours(0, min, 0, 0); return d; };

  // ── drag to move / resize ──
  const down = (e: React.PointerEvent, a: Appointment, col: number, mode: "move" | "resize") => {
    if (e.button !== 0 || !canDrag(a)) return;
    e.stopPropagation();
    const d: Drag = { a, mode, x0: e.clientX, y0: e.clientY, col0: col, on: false, col, start: new Date(a.starts_at), end: new Date(a.ends_at) };
    dragRef.current = d;
    const mv = (ev: PointerEvent) => move(ev);
    const fin = () => { window.removeEventListener("pointermove", mv); window.removeEventListener("pointerup", fin); window.removeEventListener("pointercancel", fin); up(); };
    window.addEventListener("pointermove", mv); window.addEventListener("pointerup", fin); window.addEventListener("pointercancel", fin);
  };
  const move = (e: PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    if (!d.on && Math.hypot(e.clientX - d.x0, e.clientY - d.y0) < 5) return;
    const dm = Math.round(((e.clientY - d.y0) / H) * 60 / SNAP) * SNAP;
    const s0 = new Date(d.a.starts_at), e0 = new Date(d.a.ends_at), len = e0.getTime() - s0.getTime();
    let nd: Drag;
    if (d.mode === "resize") {
      const ne = new Date(Math.max(s0.getTime() + SNAP * 60000, e0.getTime() + dm * 60000));
      nd = { ...d, on: true, end: ne };
    } else {
      const col = colAt(e.clientX, d.col);
      const sMin = Math.max(start * 60, Math.min(end * 60 - len / 60000, s0.getHours() * 60 + s0.getMinutes() + dm));
      const ns = at(col, sMin);
      nd = { ...d, on: true, col, start: ns, end: new Date(ns.getTime() + len) };
    }
    dragRef.current = nd; setDrag(nd); setGhost(null);
  };
  const up = () => {
    const d = dragRef.current;
    dragRef.current = null; setDrag(null);
    if (!d?.on) return;
    // The click that follows the drop lands on the column (mouse down and up were on different cards): ignore it.
    justDragged.current = true; setTimeout(() => { justDragged.current = false; }, 250);
    const same = d.start.getTime() === new Date(d.a.starts_at).getTime() && d.end.getTime() === new Date(d.a.ends_at).getTime() && cols[d.col].therapistId === cols[d.col0].therapistId;
    if (!same) onMove(d.a, d.start, d.end, cols[d.col]);
  };
  useEffect(() => {
    if (!drag) return;
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); dragRef.current = null; setDrag(null); } };
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  }, [drag]);

  const card = (a: Appointment, ci: number, lay: { lane: number; of: number } | null, preview?: { s: Date; e: Date }) => {
    const sIso = preview ? preview.s.toISOString() : a.starts_at, eIso = preview ? preview.e.toISOString() : a.ends_at;
    const h0 = Math.max(start, hourOf(sIso));
    const durH = (new Date(eIso).getTime() - new Date(sIso).getTime()) / 36e5;
    const top = (h0 - start) * H;
    const height = Math.max(compactWeek ? 18 : 26, Math.min(durH * H, total - top) - (compactWeek ? 2 : 4));
    const block = a.kind === "block";
    const gap = compactWeek ? 2 : 4;
    const left = lay ? `calc(${(lay.lane / lay.of) * 100}% + ${gap}px)` : `${gap}px`;
    const width = lay ? `calc(${100 / lay.of}% - ${gap * 2}px)` : `calc(100% - ${gap * 2}px)`;
    const time = `${hm(sIso)}–${hm(eIso)}`;
    const st = block ? kindLabel("block", lang) : status(a);
    // One layout for every card (audit M4): title, then time and status, then the type — as many rows as fit.
    const rows = compactWeek ? (height >= 38 ? 2 : 1) : height >= 82 ? 3 : height >= 42 ? 2 : 1;
    const icons = <>
      {a.series_id && <Repeat aria-label={L("Recorrente", "Recurring")} />}
      {a.kind === "home" && <Home aria-label={L("Domicílio", "Home visit")} />}
      {a.kind === "class" && <span className="cnt"><Users aria-label={L("Aula de grupo", "Group class")} />{a.attendees?.length ?? 0}/{a.capacity}</span>}
    </>;
    const draggable = canDrag(a);
    return (
      <button key={a.id + (preview ? "-p" : "")} data-appt={a.id}
        className={`gc ${block ? "block" : ""} ${sel === a.id ? "sel" : ""} ${preview ? "dragging" : ""} ${drag?.on && drag.a.id === a.id && !preview ? "ghosted" : ""} ${draggable ? "can-drag" : ""} ${a.status === "no_show" ? "faded" : ""}`}
        style={{ ["--tc" as string]: color(a), top: top + gap / 2, height, left, width }}
        aria-label={`${title(a)}, ${time}, ${a.therapist_name ?? L("Toda a clínica", "Whole clinic")}, ${st}`}
        onClick={() => { if (!justDragged.current) onSel(a.id); }}
        onContextMenu={(e) => { onSel(a.id); openMenu(app, e, apptItems(app, a)); }}
        onPointerDown={(e) => down(e, a, ci, "move")}>
        {rows === 1 ? (
          <span className="r1">{compactWeek && <span className="tm">{hm(sIso)}</span>}<span className="nm">{title(a)}</span>{icons}{!compactWeek && <span className="aside">{time} • {st}</span>}</span>
        ) : (
          <>
            <span className="r1"><span className="nm">{title(a)}</span>{icons}</span>
            <span className="r2">{time}{!block && ` • ${st}`}</span>
            {rows >= 3 && <span className="r2">{block ? (a.therapist_name ?? L("Toda a clínica", "Whole clinic")) : `${kindLabel(a.kind, lang)}${!cols[ci].therapistId && a.therapist_name ? ` • ${a.therapist_name}` : ""}`}</span>}
          </>
        )}
        {draggable && !block && <span className="grip" aria-hidden="true" onPointerDown={(e) => down(e, a, ci, "resize")} />}
      </button>
    );
  };

  const hasHead = cols.some((c) => c.head);
  return (
    <div className={`tg ${compactWeek ? "wk" : "dy"} ${drag?.on ? "is-dragging" : ""}`} style={{ ["--n" as string]: cols.length }}>
      {hasHead && <div className="tg-h corner" />}
      {hasHead && cols.map((c) => c.onHead
        ? <button key={c.key} className={`tg-h ${c.isToday ? "today" : ""}`} onClick={c.onHead} aria-label={c.headLabel}>{c.head}</button>
        : <div key={c.key} className="tg-h">{c.head}</div>)}
      <div className="tg-hours" style={{ height: total }}>
        {Array.from({ length: end - start }, (_, i) => i + start).map((h) => (
          <span key={h} className={cols.some((c) => c.isToday) && Math.floor(nowH) === h && !compactWeek ? "cur" : ""} style={{ top: (h - start) * H }}>{pad(h)}:00</span>
        ))}
      </div>
      {cols.map((c, ci) => {
        const blocks = c.appts.filter((a) => a.kind === "block");
        const real = c.appts.filter((a) => a.kind !== "block");
        const lay = lanes(real);
        const off: [number, number][] = c.open ? [[start, c.open[0]], [c.open[1], end]] : [[start, end]];
        return (
          <div key={c.key} ref={(el) => { colRefs.current[ci] = el; }} className={`tg-col ${c.open ? "" : "closed"}`} style={{ height: total, ["--hh" as string]: `${H}px` }}
            onMouseMove={(e) => { if (dragRef.current || e.target !== e.currentTarget) { if (ghost && e.target !== e.currentTarget) setGhost(null); return; } const m = minAt(ci, e.clientY); if (ghost?.col !== ci || ghost.min !== m) setGhost({ col: ci, min: m }); }}
            onMouseLeave={() => setGhost(null)}
            onClick={(e) => { if (justDragged.current) return; if (e.target !== e.currentTarget && !(e.target as HTMLElement).classList.contains("tg-ghost") && !(e.target as HTMLElement).classList.contains("tg-off")) return; onBook(at(ci, minAt(ci, e.clientY)), c.therapistId); }}>
            {off.filter(([a, b]) => b > a).map(([a, b], i) => <div key={i} className="tg-off" style={{ top: (a - start) * H, height: (b - a) * H }} />)}
            {c.isToday && nowH > start && nowH < end && <div className="nowline" style={{ top: (nowH - start) * H }} aria-hidden="true" />}
            {blocks.map((a) => card(a, ci, null))}
            {real.map((a) => card(a, ci, lay.get(a.id)!))}
            {drag?.on && drag.col === ci && card(drag.a, ci, null, { s: drag.start, e: drag.end })}
            {ghost?.col === ci && !drag && (
              <div className="tg-ghost" aria-hidden="true" style={{ top: ((ghost.min / 60) - start) * H + 1, height: Math.min(45 / 60 * H, total - ((ghost.min / 60) - start) * H) - 2 }}>
                + {pad(Math.floor(ghost.min / 60))}:{pad(ghost.min % 60)}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

