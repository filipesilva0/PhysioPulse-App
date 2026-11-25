// Building blocks of the iPhone app's screens (phase 5), drawn after the owner's reference images
// (claude/references: the phone home and schedule, the grouped profile list, the soft sign-up screen):
// a warm grey canvas, near-white cards with a dark bar on the left, round icon buttons in the header,
// a centred title on inner screens, small grey section headers and rows that end in an arrow.
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ArrowRight, ChevronLeft, Clock, Home, Users } from "lucide-react";
import { useApp } from "../lib/ctx";
import { blockLabel, hm, kindLabel, statusLabel, type Appointment, type Therapist } from "../lib/data";

/** A round icon button (reference: the bell and calendar on the home screen, back and ⋯ on the schedule). */
export function Circle({ label, onClick, children, badge }: { label: string; onClick: () => void; children: ReactNode; badge?: number }) {
  return (
    <button className="p-circle" onClick={onClick} aria-label={badge ? `${label} (${badge})` : label} title={label}>
      {children}{badge ? <i className="p-badge" aria-hidden>{badge > 9 ? "9+" : badge}</i> : null}
    </button>
  );
}

/** A screen. `home`: a big greeting on the left with round buttons on the right (reference: home).
 *  Otherwise a centred title between a round leading and trailing button (reference: schedule, profile).
 *  The header stays at the top and blurs the content under it once the list scrolls. */
export function Screen({ title, sub, home, leading, trailing, children, label }: {
  title: string; sub?: ReactNode; home?: boolean; leading?: ReactNode; trailing?: ReactNode; children: ReactNode; label?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const el = ref.current; if (!el) return;
    const f = () => setScrolled(el.scrollTop > 4);
    el.addEventListener("scroll", f, { passive: true });
    return () => el.removeEventListener("scroll", f);
  }, []);
  useEffect(() => { ref.current?.scrollTo({ top: 0 }); }, [title]);
  return (
    <section className={`p-screen ${home ? "home" : "inner"}`} ref={ref} aria-label={label ?? title}>
      <header className={`p-head ${scrolled ? "scrolled" : ""}`}>
        {home ? (
          <div className="p-head-home">
            <div><h1>{title}</h1>{sub && <p>{sub}</p>}</div>
            <span className="p-head-btns">{trailing}</span>
          </div>
        ) : (
          <div className="p-head-row">
            <span className="p-head-lead">{leading}</span>
            <h1>{title}</h1>
            <span className="p-head-trail">{trailing}</span>
          </div>
        )}
      </header>
      {!home && sub && <div className="p-sub">{sub}</div>}
      <div className="p-body">{children}</div>
    </section>
  );
}

/** Back, as a round button with the arrow (reference: schedule). */
export const BackCircle = ({ label, onClick }: { label: string; onClick: () => void }) =>
  <Circle label={label} onClick={onClick}><ChevronLeft /></Circle>;

/** A group of rows under a small grey header (reference: profile — "Account", "Preferences", "Support"). */
export function Group({ title, footer, children, action }: { title?: string; footer?: ReactNode; children: ReactNode; action?: ReactNode }) {
  return (
    <div className="p-group">
      {(title || action) && <div className="p-group-h"><h2>{title}</h2>{action}</div>}
      <div className="p-list">{children}</div>
      {footer && <p className="p-group-f">{footer}</p>}
    </div>
  );
}

/** One row: icon, label, value, and an arrow when it opens something (reference: profile). */
export function Row({ icon, label, detail, value, onClick, chevron, tone, children, selected }: {
  icon?: ReactNode; label: ReactNode; detail?: ReactNode; value?: ReactNode; onClick?: () => void; chevron?: boolean; tone?: "bad" | "a"; children?: ReactNode; selected?: boolean;
}) {
  const inner = (
    <>
      {icon && <span className="p-row-ic" aria-hidden>{icon}</span>}
      <span className="p-row-main">
        <span className={`p-row-l ${tone ? `t-${tone}` : ""}`}>{label}</span>
        {detail && <span className="p-row-d">{detail}</span>}
        {children}
      </span>
      {value != null && <span className="p-row-v">{value}</span>}
      {chevron && <ArrowRight className="p-row-chev" aria-hidden />}
    </>
  );
  return onClick
    ? <button className={`p-row ${selected ? "sel" : ""}`} onClick={onClick} aria-pressed={selected === undefined ? undefined : selected}>{inner}</button>
    : <div className="p-row">{inner}</div>;
}

/** Section title with an optional "See all" on the right (reference: "Today's tasks — See all"). */
export function SecTitle({ title, action }: { title: string; action?: ReactNode }) {
  return <div className="p-sec-h"><h2>{title}</h2>{action}</div>;
}

/** Empty state: what's missing and, when there's one, the next step. */
export function PEmpty({ icon, title, text, action }: { icon: ReactNode; title: string; text?: string; action?: ReactNode }) {
  return <div className="p-empty"><span aria-hidden>{icon}</span><b>{title}</b>{text && <p>{text}</p>}{action}</div>;
}

export const apptTitle = (x: Appointment, lang: "pt" | "en") =>
  x.kind === "class" ? x.title || kindLabel("class", lang) : x.kind === "block" ? blockLabel(x.title, lang) : x.patient_name ?? "—";

/** "now" while it runs, "past" when it ended without being marked, else its status. */
export const apptState = (x: Appointment, now = new Date()) =>
  x.status !== "booked" ? x.status : new Date(x.ends_at) <= now ? "past" : new Date(x.starts_at) <= now ? "now" : "next";

/** An appointment as a card (references: "Today's tasks" and the schedule): dark bar, title, status in grey,
 *  the therapist's first name (or class places) and the time. `when`: "day" shows the start, "range" start – end. `note` shows the admin note in an inner box (reference: expanded card). */
export function ApptCard({ x, onClick, when = "range", note }: { x: Appointment; ths?: Therapist[]; onClick: () => void; when?: "day" | "range"; note?: boolean }) {
  const { L, lang } = useApp();
  const st = apptState(x);
  const status = st === "now" ? L("A decorrer", "In progress") : st === "next" ? L("A seguir", "Upcoming") : st === "past" ? L("Por fechar", "Not marked") : statusLabel(x.status, lang);
  const who = x.kind === "class" ? `${x.attendees?.length ?? 0}/${x.capacity ?? "—"}` : (x.therapist_name ?? "").split(" ")[0];
  const isBlock = x.kind === "block";
  return (
    <div className={`p-card appt st-${st} ${isBlock ? "block" : ""}`}>
      <button className="p-card-hit" onClick={onClick} aria-label={`${apptTitle(x, lang)}, ${hm(x.starts_at)}, ${status}`} />
      <span className="p-card-bar" aria-hidden />
      <span className="p-card-main" aria-hidden>
        <span className="p-card-t">{x.kind === "class" ? <Users /> : x.kind === "home" ? <Home /> : null}{apptTitle(x, lang)}</span>
        <span className="p-card-s">{isBlock ? kindLabel("block", lang) : `${status}${x.kind !== "session" ? ` · ${kindLabel(x.kind, lang)}` : ""}`}</span>
        <span className="p-card-foot">
          <span className="p-card-who">{who}</span>
          <span className="p-card-time"><Clock /><span className="num">{when === "day" ? hm(x.starts_at) : `${hm(x.starts_at)} – ${hm(x.ends_at)}`}</span></span>
        </span>
        {note && x.admin_note && <span className="p-card-note">{x.admin_note}</span>}
      </span>
    </div>
  );
}

/** The top of a sheet: Cancel/Close on the leading side, the title, and the action on the trailing side. */
export function SheetHead({ title, onClose, closeLabel, action }: { title: string; onClose: () => void; closeLabel?: string; action?: ReactNode }) {
  const { L } = useApp();
  return (
    <div className="p-sheet-head">
      <span className="p-grabber" aria-hidden />
      <button className="p-link" onClick={onClose}>{closeLabel ?? L("Fechar", "Close")}</button>
      <b>{title}</b>
      <span className="p-sheet-act">{action}</span>
    </div>
  );
}

/** Initials in a circle (references use photos; the clinic keeps no staff photos, so initials in the person's colour). */
export function Avatar({ name, tone, size = 40 }: { name: string; tone?: string; size?: number }) {
  const ini = name.split(" ").filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase();
  void tone; // one quiet style for everyone (owner, 12 Nov: no coloured circles)
  return <span className="p-avatar" style={{ width: size, height: size, fontSize: Math.round(size * 0.36) }} aria-hidden>{ini}</span>;
}
