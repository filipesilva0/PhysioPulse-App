import { createContext, useContext } from "react";
import type { ReactNode } from "react";

/** The assistant panel, when open, takes the trailing side of the content panel. */
export const AssistSlot = createContext<ReactNode | null>(null);

/**
 * The content panel. Pages with sections (Schedule, Finance, Settings) list them in a column on the leading side,
 * inside a white rounded card like the bento boxes (owner, 13 Nov 2026); every other page is one content area.
 */
export function Layout({ sub, children, label }: { sub?: ReactNode; children: ReactNode; label: string }) {
  const assist = useContext(AssistSlot);
  return (
    <div className={`panel ${sub ? "has-side" : ""} ${assist ? "has-assist" : ""}`}>
      {sub && <aside className="side" aria-label={label}><nav className="side-card">{sub}</nav></aside>}
      <main className="main" id="main">{children}</main>
      {assist && <aside className="assist-pane" aria-label="Assistant">{assist}</aside>}
    </div>
  );
}

/** A section link in the panel's sections column. */
export function SubItem({ on, onClick, children, count, icon }: { on: boolean; onClick: () => void; children: ReactNode; count?: number; icon?: ReactNode }) {
  return (
    <button className="sb-sub" aria-current={on ? "true" : undefined} onClick={onClick}>
      {icon}<span className="ell">{children}</span>{!!count && <span className="cnt">{count}</span>}
    </button>
  );
}
