// Choose a patient by typing (name, number, phone, email or NIF), searched on the server (audit M7).
// Replaces the dropdowns that listed every patient: those stopped at 500 and were hard to use past a few dozen.
import { useEffect, useId, useRef, useState } from "react";
import { Search, X } from "lucide-react";
import { useApp } from "../lib/ctx";
import { useApi, type PatientRow } from "../lib/data";

export function PatientPicker({ value, onChange, exclude, label, placeholder, autoFocus, emptyLabel }: {
  /** The chosen patient's id ("" = none). */
  value: string;
  onChange: (p: PatientRow | null) => void;
  /** Ids that can't be chosen here (e.g. already in the class). */
  exclude?: Set<string>;
  label: string;
  placeholder?: string;
  autoFocus?: boolean;
  /** Shown instead of "Change" when clearing means something (e.g. "No patient (walk-in)"). */
  emptyLabel?: string;
}) {
  const { L } = useApp();
  const id = useId();
  const [q, setQ] = useState("");
  const [dq, setDq] = useState("");
  const [open, setOpen] = useState(false);
  const [hi, setHi] = useState(0);
  const [picked, setPicked] = useState<PatientRow | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { const t = setTimeout(() => setDq(q.trim()), 200); return () => clearTimeout(t); }, [q]);
  // A patient chosen before the sheet opened (from their file): fetch just that row for its name.
  const { data: one } = useApi<PatientRow[]>(value && picked?.id !== value ? `/patients?id=${value}` : null);
  useEffect(() => { if (one?.[0]) setPicked(one[0]); }, [one]);
  useEffect(() => { if (!value) setPicked(null); }, [value]);
  const { data: found, loading } = useApi<PatientRow[]>(dq && !value ? `/patients?q=${encodeURIComponent(dq)}&limit=8` : null);
  const list = (found ?? []).filter((p) => !exclude?.has(p.id));
  useEffect(() => setHi(0), [dq]);

  const choose = (p: PatientRow) => { setPicked(p); setQ(""); setDq(""); setOpen(false); onChange(p); };
  const clear = () => { setPicked(null); onChange(null); setTimeout(() => input.current?.focus(), 0); };

  if (value) {
    const p = picked?.id === value ? picked : null;
    return (
      <div className="pp-sel" aria-label={label}>
        <span className="grow">
          <span className="t">{p ? p.name : L("A carregar…", "Loading…")}</span>
          {p && <span className="s">n.º {p.number}{p.phone ? ` · ${p.phone}` : ""}{p.birth_date ? ` · ${p.birth_date.split("-").reverse().join("/")}` : ""}</span>}
        </span>
        <button type="button" className="btn sm" onClick={clear} aria-label={`${emptyLabel ?? L("Mudar…", "Change…")} (${p?.name ?? label})`}>{emptyLabel ? <><X size={13} />{emptyLabel}</> : L("Mudar…", "Change…")}</button>
      </div>
    );
  }
  const show = open && !!dq;
  return (
    <div className="pp">
      <span className="pp-field">
        <Search aria-hidden="true" />
        <input ref={input} className="input" role="combobox" aria-label={label} aria-expanded={show} aria-controls={`${id}-list`} aria-autocomplete="list"
          aria-activedescendant={show && list[hi] ? `${id}-${list[hi].id}` : undefined} autoFocus={autoFocus} autoComplete="off" value={q}
          placeholder={placeholder ?? L("Nome, n.º, telefone ou NIF", "Name, number, phone or NIF")}
          onChange={(e) => { setQ(e.target.value); setOpen(true); }} onFocus={() => setOpen(true)} onBlur={() => setTimeout(() => setOpen(false), 150)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") { e.preventDefault(); setOpen(true); setHi((h) => Math.min(h + 1, list.length - 1)); }
            else if (e.key === "ArrowUp") { e.preventDefault(); setHi((h) => Math.max(h - 1, 0)); }
            else if (e.key === "Enter" && show && list[hi]) { e.preventDefault(); e.stopPropagation(); choose(list[hi]); }
            else if (e.key === "Escape" && show) { e.stopPropagation(); e.nativeEvent.stopImmediatePropagation(); setOpen(false); }
          }} />
      </span>
      {show && (
        <div className="pp-list frosted" role="listbox" id={`${id}-list`} aria-label={label}>
          {!found && loading && <div className="none">{L("A procurar…", "Searching…")}</div>}
          {found && !list.length && <div className="none">{L(`Nenhum utente encontrado para “${dq}”.`, `No patient found for “${dq}”.`)}</div>}
          {list.map((p, i) => (
            <div key={p.id} id={`${id}-${p.id}`} role="option" aria-selected={i === hi} className={`mi ${i === hi ? "hi" : ""}`}
              onMouseDown={(e) => { e.preventDefault(); choose(p); }} onMouseEnter={() => setHi(i)}>
              <span className="grow">{p.name}{!p.active && <span className="muted"> · {L("inativo", "inactive")}</span>}</span>
              <span className="sub2">n.º {p.number}{p.phone ? ` · ${p.phone}` : ""}</span>
            </div>
          ))}
          {list.length === 8 && <div className="none">{L("Escreva mais para afinar a procura.", "Type more to narrow the search.")}</div>}
        </div>
      )}
    </div>
  );
}
