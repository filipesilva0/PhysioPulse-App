// Phase 3 sheets: record a payment, sell a package, add an invoice, void a payment,
// expenses, products and salaries. All amounts are sent to the server in cents.
import { useEffect, useId, useMemo, useState } from "react";
import { Download, FileText, Paperclip, Sparkles, X } from "lucide-react";
import { useApp, eur } from "../lib/ctx";
import { api, download, isErr, viewFile } from "../lib/api";
import { useApi, ymd } from "../lib/data";
import {
  c2e, centsField, parseEuros, pctBp, employerSs, owedLabel, owedKey, productName, fileToBase64, METHODS, EXPENSE_METHODS, CATEGORIES,
  type Expense, type Method, type Owed, type Payment, type PatientMoney, type Product, type SalaryRow, type ExpenseMethod,
} from "../lib/money";
import { Field, Sheet, DateInput } from "./ui";
import { PatientPicker } from "./PatientPicker";
import { AiCard, aiOffText, useAiStatus, type AiState } from "./Ai";

const HAND_METHODS = METHODS.filter(([m]) => m !== "stripe");

/** A money field that accepts "12,50" or "12.50". Until it's valid it's outlined in red and says how to fix it (audit M11);
 *  `compact` (inside a list row) puts the message in the tooltip instead. */
function Money({ value, onChange, label, compact }: { value: string; onChange: (v: string) => void; label: string; compact?: boolean }) {
  const { L } = useApp();
  const v = value.trim();
  const msg = v === "" ? "" : /^-/.test(v) ? L("O valor não pode ser negativo.", "The amount can’t be negative.")
    : parseEuros(v) === null ? L("Escreva um valor como 12,50.", "Write an amount like 12.50.") : "";
  const id = useId();
  return (
    <>
      <input className="input num" inputMode="decimal" value={value} onChange={(e) => onChange(e.target.value)} aria-label={label} aria-invalid={!!msg || undefined}
        aria-describedby={msg && !compact ? id : undefined} title={compact ? msg || undefined : undefined} style={{ textAlign: "right", ...(msg ? { borderColor: "var(--bad)" } : {}) }} />
      {msg && !compact && <small id={id} className="field-err" role="alert">{msg}</small>}
    </>
  );
}

function MethodPicker({ value, onChange }: { value: Method; onChange: (m: Method) => void }) {
  const { L, lang } = useApp();
  return (
    <Field label={L("Método", "Method")}>
      <div className="radio-row" role="radiogroup" aria-label={L("Método", "Method")}>
        {HAND_METHODS.map(([v, t]) => (
          <label key={v} className="check"><input type="radio" name="method" checked={value === v} onChange={() => onChange(v)} />{lang === "pt" ? t[0] : t[1]}</label>
        ))}
      </div>
    </Field>
  );
}

/** Pick a PDF or photo and upload it to the server as an invoice or receipt. */
function FilePick({ kind, day, fileId, name, onDone }: { kind: "invoice" | "expense"; day: string; fileId: string | null; name: string | null; onDone: (id: string, name: string) => void }) {
  const { L } = useApp();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const pick = async (f: File | undefined) => {
    if (!f) return;
    setErr("");
    if (f.size > 10 * 1024 * 1024) { setErr(L("O ficheiro tem de ter menos de 10 MB.", "Files must be under 10 MB.")); return; }
    setBusy(true);
    try {
      const r = await api<{ id: string; filename: string }>("POST", "/finance/files", { kind, filename: f.name, day, data: await fileToBase64(f) });
      onDone(r.id, r.filename);
    } catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  return (
    <div>
      <label className="btn" style={{ cursor: "pointer" }} aria-busy={busy}>
        <Paperclip />{busy ? L("A enviar…", "Uploading…") : fileId ? L("Trocar ficheiro…", "Replace File…") : L("Anexar PDF ou foto…", "Attach PDF or Photo…")}
        <input type="file" accept="application/pdf,image/jpeg,image/png,image/heic,image/webp" hidden onChange={(e) => pick(e.target.files?.[0])} />
      </label>
      {fileId && <span className="small muted" style={{ marginLeft: 10 }}><FileText size={13} style={{ verticalAlign: "-2px" }} /> {name}</span>}
      {err && <div className="err" role="alert" style={{ marginTop: 6 }}>{err}</div>}
    </div>
  );
}

/** Record money received: tick what's being paid (from what the patient owes) and/or add another item. */
export function RecordPayment({ patientId, appointmentId }: { patientId?: string; appointmentId?: string }) {
  const { L, lang, toast } = useApp();
  const [pid, setPid] = useState(patientId ?? "");
  const { data: money } = useApi<PatientMoney>(pid ? `/patients/${pid}/money` : null);
  const [picked, setPicked] = useState<Record<string, string>>({});
  const [other, setOther] = useState({ on: false, text: "", amount: "" });
  const [method, setMethod] = useState<Method>("mbway");
  const [day, setDay] = useState(ymd(new Date()));
  const [invoice, setInvoice] = useState("");

  // Preselect: the appointment it was opened from, or the only thing owed.
  useEffect(() => {
    if (!money) return;
    const pre = money.owed.filter((o) => (appointmentId ? o.appointment_id === appointmentId : money.owed.length === 1));
    setPicked(Object.fromEntries(pre.map((o) => [owedKey(o), centsField(o.due_cents, lang)])));
    if (!money.owed.length) setOther((x) => ({ ...x, on: true }));
  }, [money, appointmentId]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { setPicked({}); setOther({ on: !pid, text: "", amount: "" }); }, [pid]);

  const items = useMemo(() => {
    const out: { amount_cents: number | null; appointment_id?: string; plan_id?: string; covers_month?: string; description?: string }[] = [];
    for (const o of money?.owed ?? []) {
      const v = picked[owedKey(o)];
      if (v === undefined) continue;
      out.push({ amount_cents: parseEuros(v), appointment_id: o.appointment_id ?? undefined, plan_id: o.plan_id ?? undefined, covers_month: o.month ?? undefined });
    }
    if (other.on) out.push({ amount_cents: parseEuros(other.amount), description: other.text.trim() });
    return out;
  }, [picked, other, money]);
  const total = items.reduce((t, i) => t + (i.amount_cents ?? 0), 0);
  const ready = items.length > 0 && items.every((i) => (i.amount_cents ?? 0) > 0) && (!other.on || !!other.text.trim());

  const submit = async () => {
    const r = await api<{ ids: string[]; numbers: number[] }>("POST", "/payments", {
      patient_id: pid || null, method, received_on: day, invoice_number: invoice.trim(), items,
    });
    toast(L(`Pagamento registado: ${eur(c2e(total), lang)}`, `Payment recorded: ${eur(c2e(total), lang)}`),
      () => { Promise.all(r.ids.map((id) => api("POST", `/payments/${id}/void`, { reason: L("Desfeito logo a seguir", "Undone right away") }))).catch((e) => toast(isErr(e) ? e.message : String(e))); },
      L("Registar pagamento", "Record Payment"));
  };

  return (
    <Sheet title={L("Registar pagamento", "Record Payment")} action={total > 0 ? L(`Registar ${eur(c2e(total), lang)}`, `Record ${eur(c2e(total), lang)}`) : L("Registar", "Record")}
      onAction={submit} disabled={!ready}>
      <div className="grid2">
        <Field label={L("Utente", "Patient")}>
          <PatientPicker value={pid} label={L("Utente", "Patient")} onChange={(p) => setPid(p?.id ?? "")} emptyLabel={L("Sem ficha", "No Patient")}
            placeholder={L("Procurar, ou deixar vazio (avulso)", "Search, or leave empty (walk-in)")} />
        </Field>
        <Field label={L("Data", "Date")}><DateInput className="input" value={day} max={ymd(new Date())} onChange={(e) => setDay(e.target.value)} /></Field>
      </div>
      {pid && (
        <Field label={L("O que está a pagar", "What’s being paid")}>
          <div className="rows" style={{ marginTop: 2 }}>
            {!money ? <div className="row"><span className="muted small">{L("A carregar…", "Loading…")}</span></div> : money.owed.length === 0 ? (
              <div className="row"><span className="muted small">{L("Nada em dívida. Use “Outro” para registar outro valor.", "Nothing owed. Use “Other” to record another amount.")}</span></div>
            ) : money.owed.map((o: Owed) => {
              const k = owedKey(o), on = k in picked;
              return (
                <div key={k} className="row">
                  <label className="check grow" style={{ margin: 0 }}>
                    <input type="checkbox" checked={on} onChange={() => setPicked((p) => { const n = { ...p }; if (on) delete n[k]; else n[k] = centsField(o.due_cents, lang); return n; })} />
                    <span>{owedLabel(o, lang)}{o.due_cents < o.price_cents && <span className="muted small"> · {L(`falta ${eur(c2e(o.due_cents), lang)} de ${eur(c2e(o.price_cents), lang)}`, `${eur(c2e(o.due_cents), lang)} left of ${eur(c2e(o.price_cents), lang)}`)}</span>}</span>
                  </label>
                  {on ? <span style={{ width: 96 }}><Money compact value={picked[k]} onChange={(v) => setPicked((p) => ({ ...p, [k]: v }))} label={owedLabel(o, lang)} /></span>
                    : <span className="amt muted">{eur(c2e(o.due_cents), lang)}</span>}
                </div>
              );
            })}
            <div className="row">
              <label className="check grow" style={{ margin: 0 }}><input type="checkbox" checked={other.on} onChange={() => setOther((x) => ({ ...x, on: !x.on }))} />{L("Outro", "Other")}</label>
            </div>
          </div>
        </Field>
      )}
      {other.on && (
        <div className="grid2" style={{ gridTemplateColumns: "minmax(0,1fr) 120px" }}>
          <Field label={L("Descrição", "Description")}><input className="input" value={other.text} onChange={(e) => setOther((x) => ({ ...x, text: e.target.value }))} placeholder={L("Ex.: aula avulsa, venda de material", "E.g. drop-in class, kinesiology tape")} /></Field>
          <Field label={L("Valor (€)", "Amount (€)")}><Money value={other.amount} onChange={(v) => setOther((x) => ({ ...x, amount: v }))} label={L("Valor", "Amount")} /></Field>
        </div>
      )}
      <MethodPicker value={method} onChange={setMethod} />
      <Field label={L("N.º da fatura (opcional)", "Invoice number (optional)")}>
        <input className="input" value={invoice} onChange={(e) => setInvoice(e.target.value)} placeholder={L("Ex.: FR 2026/14 — pode juntar depois", "E.g. FR 2026/14 — you can add it later")} />
      </Field>
      <p className="small muted">{L("Pagamentos com Stripe (QR ou link) ficam registados sozinhos quando a Stripe for ligada.", "Stripe payments (QR or link) will record themselves once Stripe is connected.")}</p>
    </Sheet>
  );
}

/** Sell a package or monthly plan; optionally take the first payment straight away. */
export function SellPlan({ patientId }: { patientId?: string }) {
  const { L, lang, toast } = useApp();
  const { data: products } = useApi<Product[]>("/products");
  const [pid, setPid] = useState(patientId ?? "");
  const [prodId, setProdId] = useState("");
  const [start, setStart] = useState(ymd(new Date()));
  const [share, setShare] = useState("");
  const [payNow, setPayNow] = useState(true);
  const [amount, setAmount] = useState("");
  const [method, setMethod] = useState<Method>("mbway");
  const sellable = products?.filter((p) => p.kind !== "visit") ?? [];
  const prod = sellable.find((p) => p.id === prodId);
  useEffect(() => { if (prod) setAmount(centsField(Math.ceil(prod.price_cents / prod.instalments), lang)); setShare(""); }, [prodId]); // eslint-disable-line react-hooks/exhaustive-deps
  const ready = !!pid && !!prod && (!payNow || (parseEuros(amount) ?? 0) > 0);
  const detail = (p: Product) => [
    p.sessions ? (p.kind === "monthly" ? L(`${p.sessions} por mês`, `${p.sessions} a month`) : L(`${p.sessions} sessões`, `${p.sessions} sessions`)) : "",
    p.valid_months ? L(`válido ${p.valid_months} ${p.valid_months === 1 ? "mês" : "meses"}`, `valid ${p.valid_months} month${p.valid_months === 1 ? "" : "s"}`) : "",
    p.instalments > 1 ? L(`${p.instalments} prestações`, `${p.instalments} instalments`) : "",
    p.shareable ? L("partilhável", "shareable") : "",
  ].filter(Boolean).join(" · ");
  const submit = async () => {
    const r = await api<{ id: string; attached: number }>("POST", `/patients/${pid}/plans`, { product_id: prodId, starts_on: start, shared_with: share || null });
    if (payNow) await api("POST", "/payments", { patient_id: pid, method, items: [{ plan_id: r.id, amount_cents: parseEuros(amount) }] });
    toast(r.attached ? L(`Vendido; ${r.attached} sessão(ões) por pagar passaram para o pacote`, `Sold; ${r.attached} unpaid session(s) moved onto it`) : L("Pacote vendido", "Package sold"));
  };
  return (
    <Sheet title={L("Vender pacote ou plano", "Sell Package or Plan")} action={L("Vender", "Sell")} onAction={submit} disabled={!ready}>
      <div className="grid2">
        <Field label={L("Utente", "Patient")}>
          <PatientPicker value={pid} label={L("Utente", "Patient")} autoFocus={!pid} onChange={(p) => { setPid(p?.id ?? ""); setShare(""); }} />
        </Field>
        <Field label={L("Começa a", "Starts on")}><DateInput className="input" value={start} onChange={(e) => setStart(e.target.value)} /></Field>
      </div>
      <Field label={L("Pacote ou plano", "Package or plan")}>
        <select className="input" value={prodId} onChange={(e) => setProdId(e.target.value)}>
          <option value="" disabled>{L("Escolha", "Choose")}</option>
          {sellable.map((p) => <option key={p.id} value={p.id}>{productName(p, lang)} — {eur(c2e(p.price_cents), lang)}{p.kind === "monthly" ? L("/mês", "/month") : ""}</option>)}
        </select>
      </Field>
      {prod && <p className="small muted" style={{ marginTop: -6, marginBottom: 12 }}>{detail(prod)}</p>}
      {prod?.shareable && (
        <Field label={L("Partilhar com um familiar (opcional)", "Share with a family member (optional)")}>
          <PatientPicker value={share} label={L("Partilhar com", "Share with")} exclude={new Set([pid])} emptyLabel={L("Ninguém", "No One")}
            placeholder={L("Ninguém (procure para partilhar)", "No one (search to share)")} onChange={(p) => setShare(p?.id ?? "")} />
        </Field>
      )}
      <p className="small muted" style={{ marginBottom: 12 }}>{L("Sessões já realizadas e por pagar desde a data de início passam a usar este pacote.", "Unpaid sessions done since the start date will use this package.")}</p>
      <label className="check"><input type="checkbox" checked={payNow} onChange={(e) => setPayNow(e.target.checked)} />{L("Registar o pagamento agora", "Record the payment now")}</label>
      {payNow && (
        <>
          <Field label={prod && prod.instalments > 1 ? L(`Valor (1.ª de ${prod.instalments} prestações)`, `Amount (1st of ${prod.instalments} instalments)`) : L("Valor (€)", "Amount (€)")}>
            <Money value={amount} onChange={setAmount} label={L("Valor", "Amount")} />
          </Field>
          <MethodPicker value={method} onChange={setMethod} />
        </>
      )}
    </Sheet>
  );
}

/** Link the invoice issued in the certified invoicing software: its number and, optionally, the PDF. */
export function InvoiceSheet({ payment }: { payment: Payment }) {
  const { L, lang, toast } = useApp();
  const [num, setNum] = useState(payment.invoice_number);
  const [file, setFile] = useState<{ id: string | null; name: string | null }>({ id: payment.invoice_file_id, name: payment.invoice_file_id ? L("PDF anexado", "PDF attached") : null });
  return (
    <Sheet title={L("Fatura", "Invoice")} sub={`${payment.description} · ${eur(c2e(payment.amount_cents), lang)}${payment.patient_name ? ` · ${payment.patient_name}` : ""}`}
      action={L("Guardar", "Save")} disabled={!num.trim()}
      onAction={async () => { await api("PATCH", `/payments/${payment.id}`, { invoice_number: num.trim(), ...(file.id && file.id !== payment.invoice_file_id ? { invoice_file_id: file.id } : {}) }); toast(L("Fatura registada", "Invoice recorded")); }}>
      <p className="small muted" style={{ marginBottom: 12 }}>{L("Emita a fatura no programa de faturação certificado e registe aqui o número. O PDF vai para a pasta Finanças no servidor e para a exportação do contabilista.", "Issue the invoice in your certified invoicing software and record its number here. The PDF goes into the Finance folder on the server and into the accountant export.")}</p>
      <Field label={L("N.º da fatura", "Invoice number")}><input className="input" value={num} onChange={(e) => setNum(e.target.value)} placeholder="FR 2026/14" autoFocus /></Field>
      <FilePick kind="invoice" day={payment.received_on} fileId={file.id} name={file.name} onDone={(id, name) => setFile({ id, name })} />
    </Sheet>
  );
}

/** The invoice PDF attached to a payment, shown inside the app (like a patient document). */
export function InvoiceViewer({ payment }: { payment: Payment }) {
  const { L, lang } = useApp();
  return <FileViewer fileId={payment.invoice_file_id!} title={L(`Fatura ${payment.invoice_number}`, `Invoice ${payment.invoice_number}`)}
    sub={`${payment.description} · ${eur(c2e(payment.amount_cents), lang)}${payment.patient_name ? ` · ${payment.patient_name}` : ""}`}
    filename={`${payment.invoice_number.replace(/\//g, "-") || L("fatura", "invoice")}.pdf`} />;
}

/** A finance file (invoice, receipt, report PDF) shown inside the app, with Save a Copy. */
export function FileViewer({ fileId, title, sub, filename }: { fileId: string; title: string; sub?: string; filename: string }) {
  const { L, toast, closeSheet, cancelSheet } = useApp();
  const [url, setUrl] = useState("");
  const [mime, setMime] = useState("");
  const [err, setErr] = useState("");
  useEffect(() => {
    let u = "";
    viewFile(fileId).then((b) => { u = URL.createObjectURL(b); setMime(b.type); setUrl(u); }).catch((e) => setErr(isErr(e) ? e.message : String(e)));
    return () => { if (u) URL.revokeObjectURL(u); };
  }, [fileId]);
  const save = () => download(fileId, filename).then((w) => toast(L(`Guardado em ${w}`, `Saved to ${w}`))).catch((e) => toast(isErr(e) ? e.message : String(e)));
  return (
    <div className="sheet viewer" role="dialog" aria-modal="true" aria-label={title}>
      <div className="viewer-bar">
        <div style={{ minWidth: 0, flex: 1 }}>
          <h2 className="ell">{title}</h2>
          {sub && <p className="small muted ell">{sub}</p>}
        </div>
        <button className="btn" onClick={save}><Download />{L("Guardar uma cópia…", "Save a Copy…")}</button>
        <button className="btn icon" onClick={cancelSheet} aria-label={L("Fechar", "Close")}><X /></button>
      </div>
      <div className="viewer-body">
        {err ? <p className="err" role="alert" style={{ margin: 20 }}>{err}</p>
          : !url ? <div className="skel" style={{ height: "100%", width: "100%", borderRadius: 0 }} aria-label={L("A abrir…", "Opening…")} />
          : mime === "application/pdf" ? <iframe title={title} src={url} />
          : mime.startsWith("image/") ? <img src={url} alt={title} />
          : <div className="viewer-none"><FileText /><p>{L("Este tipo de ficheiro não pode ser mostrado na app.", "This kind of file can’t be shown in the app.")}</p><p className="small muted">{L("Guarde uma cópia para o abrir noutra aplicação.", "Save a copy to open it in another app.")}</p></div>}
      </div>
    </div>
  );
}

/** Void a payment entered by mistake. It stays in the records, marked as voided, with the reason. */
export function VoidPayment({ payment }: { payment: Payment }) {
  const { L, lang, toast } = useApp();
  const [reason, setReason] = useState("");
  return (
    <Sheet title={L("Anular pagamento", "Void Payment")} sub={`${payment.description} · ${eur(c2e(payment.amount_cents), lang)}`} action={L("Anular pagamento", "Void Payment")} destructive
      disabled={!reason.trim()} onAction={async () => { await api("POST", `/payments/${payment.id}/void`, { reason: reason.trim() }); toast(L("Pagamento anulado", "Payment voided")); }}>
      <p className="small muted" style={{ marginBottom: 12 }}>{L("Os pagamentos nunca são apagados: este fica nos registos, marcado como anulado. Se já foi emitida fatura, anule-a também no programa de faturação.", "Payments are never deleted: this one stays in the records, marked as voided. If an invoice was issued, cancel it in the invoicing software too.")}</p>
      <Field label={L("Motivo", "Reason")}><input className="input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder={L("Ex.: valor errado, registado em duplicado", "E.g. wrong amount, recorded twice")} autoFocus /></Field>
    </Sheet>
  );
}

interface ReadReceipt {
  fields: { supplier: string; supplier_nif: string; spent_on: string | null; amount_cents: number | null; vat_cents: number | null; category: string; document_ref: string; description: string; method: ExpenseMethod | null };
  missing: string[]; model: string;
}

/** Add or edit an expense; attach the receipt photo or PDF, and (owner) let the local AI read it into the fields. */
export function ExpenseSheet({ expense }: { expense?: Expense }) {
  const { L, lang, toast, role } = useApp();
  const ai = useAiStatus();
  const [reading, setReading] = useState<AiState<{ before: typeof f; filled: string[]; missing: string[] }>>({ state: "idle" });
  const [aiRead, setAiRead] = useState(!!expense?.ai_read);
  const [f, setF] = useState({
    spent_on: expense?.spent_on ?? ymd(new Date()), supplier: expense?.supplier ?? "", supplier_nif: expense?.supplier_nif ?? "", category: expense?.category ?? "supplies",
    description: expense?.description ?? "", amount: expense ? centsField(expense.amount_cents, lang) : "", vat: expense?.vat_cents != null ? centsField(expense.vat_cents, lang) : "",
    method: (expense?.method ?? "transfer") as ExpenseMethod, document_ref: expense?.document_ref ?? "",
  });
  const [file, setFile] = useState<{ id: string | null; name: string | null }>({ id: expense?.file_id ?? null, name: expense?.filename ?? null });
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }));
  const amount = parseEuros(f.amount), vat = f.vat ? parseEuros(f.vat) : null;
  const ready = !!f.spent_on && (amount ?? 0) > 0 && (f.vat === "" || vat !== null);
  const submit = async () => {
    const body = { spent_on: f.spent_on, supplier: f.supplier.trim(), supplier_nif: f.supplier_nif.trim(), category: f.category, description: f.description.trim(),
      amount_cents: amount, vat_cents: f.vat ? vat : null, method: f.method, document_ref: f.document_ref.trim(), file_id: file.id, ai_read: aiRead };
    if (expense) await api("PATCH", `/expenses/${expense.id}`, body); else await api("POST", "/expenses", body);
    toast(expense ? L("Despesa atualizada", "Expense updated") : L("Despesa registada", "Expense recorded"));
  };
  /** The AI reads the attached receipt; what it found replaces the fields (Discard puts them back). */
  const read = async (fileId = file.id) => {
    if (!fileId) return;
    setReading({ state: "working" });
    try {
      const r = await api<ReadReceipt>("POST", "/ai/read-receipt", { file_id: fileId });
      const x = r.fields, before = f, filled: string[] = [];
      const next = { ...f };
      const put = (k: keyof typeof f, v: string | null | undefined) => { if (v != null && v !== "") { (next[k] as string) = v; filled.push(k); } };
      put("supplier", x.supplier); put("supplier_nif", x.supplier_nif); put("spent_on", x.spent_on);
      put("amount", x.amount_cents != null ? centsField(x.amount_cents, lang) : null); put("vat", x.vat_cents != null ? centsField(x.vat_cents, lang) : null);
      put("category", x.category); put("document_ref", x.document_ref); put("description", x.description); put("method", x.method);
      setF(next); setAiRead(true);
      setReading({ state: "done", result: { before, filled, missing: r.missing } });
    } catch (e) { setReading({ state: "error", message: isErr(e) ? e.message : String(e) }); }
  };
  const discard = () => { if (reading.state === "done") { setF(reading.result.before); setAiRead(!!expense?.ai_read); } setReading({ state: "idle" }); };
  const filled = reading.state === "done" ? reading.result.filled : [];
  const mark = (k: string) => filled.includes(k) ? " ai-filled" : "";
  const missingLabel: Record<string, string> = { supplier: L("fornecedor", "supplier"), spent_on: L("data", "date"), amount_cents: L("valor", "amount") };
  const owner = role === "owner";
  return (
    <Sheet wide title={expense ? L("Editar despesa", "Edit Expense") : L("Nova despesa", "New Expense")} action={L("Guardar", "Save")} onAction={submit} disabled={!ready || reading.state === "working"}>
      <div className="receipt-row">
        <FilePick kind="expense" day={f.spent_on || ymd(new Date())} fileId={file.id} name={file.name} onDone={(id, name) => { setFile({ id, name }); setReading({ state: "idle" }); }} />
        {owner && file.id && (
          <button className="btn ai" onClick={() => read()} disabled={reading.state === "working" || (!!ai.data && !ai.data.available)}><Sparkles />{L("Ler o talão com a IA", "Read Receipt with AI")}</button>
        )}
      </div>
      {owner && !file.id && <p className="small muted" style={{ margin: "6px 0 12px" }}>{L("Anexe primeiro o talão ou a fatura: a IA local lê o fornecedor, a data e os valores e preenche os campos. Nada sai da clínica.", "Attach the receipt or invoice first: the local AI reads the supplier, date and amounts and fills in the fields. Nothing leaves the clinic.")}</p>}
      {owner && file.id && ai.data && !ai.data.available && reading.state === "idle" && <p className="small muted" style={{ margin: "6px 0 12px" }}>{aiOffText(ai.data, L)}</p>}
      {reading.state !== "idle" && (
        <div style={{ margin: "10px 0 14px" }}>
          <AiCard lines={2}
            title={reading.state === "error" ? L("A IA não conseguiu ler o talão", "The AI couldn’t read the receipt") : L("Preenchido pela IA local a partir do talão — confirme cada campo antes de guardar", "Filled in by the local AI from the receipt — check each field before saving")}
            working={reading.state === "working" ? L("A ler o fornecedor, a data, o valor e o IVA do talão… pode demorar um minuto.", "Reading the supplier, date, amount and VAT from the receipt… this can take a minute.") : undefined}
            error={reading.state === "error" ? reading.message : undefined}
            actions={<>
              <button className="btn sm" onClick={() => read()}>{L("Tentar de novo", "Retry")}</button>
              {reading.state === "done" ? <button className="btn sm" onClick={discard}>{L("Desfazer preenchimento", "Undo Fill")}</button>
                : <button className="btn sm" onClick={() => setReading({ state: "idle" })}>{L("Fechar", "Close")}</button>}
            </>}>
            {reading.state === "done" && reading.result.missing.length > 0 && <p className="small">{L(`Não consegui ler: ${reading.result.missing.map((m) => missingLabel[m] ?? m).join(", ")}. Escreva à mão.`, `Couldn’t read: ${reading.result.missing.map((m) => missingLabel[m] ?? m).join(", ")}. Type it in.`)}</p>}
            {reading.state === "done" && reading.result.missing.length === 0 && <p className="small">{L("Os campos com contorno violeta vieram do talão.", "Fields with a violet outline came from the receipt.")}</p>}
          </AiCard>
        </div>
      )}
      <div className="grid3">
        <Field label={L("Data", "Date")}><DateInput className={"input" + mark("spent_on")} value={f.spent_on} onChange={set("spent_on")} /></Field>
        <Field label={L("Valor com IVA (€)", "Amount incl. VAT (€)")}><span className={mark("amount")}><Money value={f.amount} onChange={(v) => setF((x) => ({ ...x, amount: v }))} label={L("Valor", "Amount")} /></span></Field>
        <Field label={L("IVA (€, opcional)", "VAT (€, optional)")}><span className={mark("vat")}><Money value={f.vat} onChange={(v) => setF((x) => ({ ...x, vat: v }))} label={L("IVA", "VAT")} /></span></Field>
      </div>
      <div className="grid2">
        <Field label={L("Fornecedor", "Supplier")}><input className={"input" + mark("supplier")} value={f.supplier} onChange={set("supplier")} placeholder="Fisiomed Lda." /></Field>
        <Field label={L("NIF do fornecedor", "Supplier’s tax number")}><input className={"input num" + mark("supplier_nif")} value={f.supplier_nif} onChange={set("supplier_nif")} inputMode="numeric" placeholder="500 000 000" /></Field>
      </div>
      <div className="grid3">
        <Field label={L("Categoria", "Category")}>
          <select className={"input" + mark("category")} value={f.category} onChange={set("category")}>{CATEGORIES.map(([k, t]) => <option key={k} value={k}>{lang === "pt" ? t[0] : t[1]}</option>)}</select>
        </Field>
        <Field label={L("Pago por", "Paid by")}>
          <select className={"input" + mark("method")} value={f.method} onChange={set("method")}>{EXPENSE_METHODS.map(([k, t]) => <option key={k} value={k}>{lang === "pt" ? t[0] : t[1]}</option>)}</select>
        </Field>
        <Field label={L("N.º do documento", "Document number")}><input className={"input" + mark("document_ref")} value={f.document_ref} onChange={set("document_ref")} placeholder="FT 2026/381" /></Field>
      </div>
      <Field label={L("Descrição (opcional)", "Description (optional)")}><input className={"input" + mark("description")} value={f.description} onChange={set("description")} /></Field>
      {aiRead && reading.state !== "done" && <p className="ls-ai-note"><Sparkles size={11} />{L("Dados lidos do talão pela IA local", "Details read from the receipt by the local AI")}</p>}
    </Sheet>
  );
}

/** Owner: add or change a product in the price list. Packages already sold keep their price. */
export function ProductSheet({ product }: { product?: Product }) {
  const { L, lang, toast } = useApp();
  const [f, setF] = useState({
    kind: product?.kind ?? "pack", visit_kind: product?.visit_kind ?? "session", applies_to: product?.applies_to ?? "session",
    name_pt: product?.name_pt ?? "", name_en: product?.name_en ?? "", price: product ? centsField(product.price_cents, lang) : "",
    sessions: product?.sessions != null ? String(product.sessions) : "", valid_months: product?.valid_months != null ? String(product.valid_months) : "",
    shareable: product?.shareable ?? false, instalments: String(product?.instalments ?? 1), active: product?.active ?? true,
    vat_exempt: product?.vat_exempt ?? true,
  });
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }));
  // A new class product starts as not exempt (fitness-style classes pay VAT); the owner ticks it for therapeutic classes.
  const isClass = f.kind === "visit" ? f.visit_kind === "class" : f.applies_to === "class";
  useEffect(() => { if (!product) setF((x) => ({ ...x, vat_exempt: !isClass })); }, [isClass]); // eslint-disable-line react-hooks/exhaustive-deps
  const price = parseEuros(f.price);
  const ready = !!f.name_pt.trim() && !!f.name_en.trim() && price !== null && (f.kind !== "pack" || +f.sessions > 0);
  const submit = async () => {
    const body: Record<string, unknown> = {
      name_pt: f.name_pt.trim(), name_en: f.name_en.trim(), price_cents: price, active: f.active, vat_exempt: f.vat_exempt,
      ...(f.kind !== "visit" ? { applies_to: f.applies_to, sessions: f.sessions ? +f.sessions : null, valid_months: f.kind === "pack" && f.valid_months ? +f.valid_months : null, shareable: f.shareable, instalments: +f.instalments || 1 } : {}),
    };
    if (product) await api("PATCH", `/products/${product.id}`, body);
    else await api("POST", "/products", { ...body, kind: f.kind, ...(f.kind === "visit" ? { visit_kind: f.visit_kind } : {}) });
    toast(product ? L("Preço atualizado", "Price updated") : L("Produto criado", "Product added"));
  };
  return (
    <Sheet title={product ? L("Editar produto", "Edit Product") : L("Novo produto", "New Product")} action={L("Guardar", "Save")} onAction={submit} disabled={!ready}
      sub={product && product.kind !== "visit" ? L("Quem já comprou mantém o preço que pagou.", "People who already bought it keep the price they paid.") : undefined}>
      {!product && (
        <Field label={L("Tipo", "Type")}>
          <select className="input" value={f.kind} onChange={set("kind")}>
            <option value="pack">{L("Pacote de sessões", "Session package")}</option><option value="monthly">{L("Plano mensal", "Monthly plan")}</option>
            <option value="visit">{L("Preço de uma marcação", "Price of one appointment")}</option>
          </select>
        </Field>
      )}
      <div className="grid2">
        <Field label={L("Nome em português", "Name in Portuguese")}><input className="input" value={f.name_pt} onChange={set("name_pt")} /></Field>
        <Field label={L("Nome em inglês", "Name in English")}><input className="input" value={f.name_en} onChange={set("name_en")} /></Field>
      </div>
      <div className="grid3">
        <Field label={f.kind === "monthly" ? L("Preço por mês (€)", "Price a month (€)") : L("Preço (€)", "Price (€)")}><Money value={f.price} onChange={(v) => setF((x) => ({ ...x, price: v }))} label={L("Preço", "Price")} /></Field>
        {f.kind === "visit" && !product && (
          <Field label={L("Para", "For")}>
            <select className="input" value={f.visit_kind} onChange={set("visit_kind")}>
              <option value="session">{L("Sessão", "Session")}</option><option value="assessment">{L("Avaliação inicial", "First assessment")}</option>
              <option value="home">{L("Domicílio", "Home visit")}</option><option value="class">{L("Aula de grupo", "Group class")}</option>
            </select>
          </Field>
        )}
        {f.kind !== "visit" && <>
          <Field label={f.kind === "monthly" ? L("Sessões por mês", "Sessions a month") : L("Sessões", "Sessions")}><input className="input num" type="number" min={1} value={f.sessions} onChange={set("sessions")} placeholder={f.kind === "monthly" ? L("sem limite", "no limit") : ""} /></Field>
          {f.kind === "pack" && <Field label={L("Válido (meses)", "Valid (months)")}><input className="input num" type="number" min={1} value={f.valid_months} onChange={set("valid_months")} /></Field>}
        </>}
      </div>
      {f.kind !== "visit" && (
        <div className="grid2">
          <Field label={L("Para", "For")}>
            <select className="input" value={f.applies_to} onChange={set("applies_to")}><option value="session">{L("Sessões individuais", "Individual sessions")}</option><option value="class">{L("Aulas de grupo", "Group classes")}</option></select>
          </Field>
          {f.kind === "pack" && <Field label={L("Prestações", "Instalments")}><input className="input num" type="number" min={1} max={12} value={f.instalments} onChange={set("instalments")} /></Field>}
        </div>
      )}
      {f.kind === "pack" && <label className="check"><input type="checkbox" checked={f.shareable} onChange={(e) => setF((x) => ({ ...x, shareable: e.target.checked }))} />{L("Pode ser partilhado com um familiar", "Can be shared with a family member")}</label>}
      <label className="check" style={{ alignItems: "flex-start" }}><input type="checkbox" checked={f.vat_exempt} onChange={(e) => setF((x) => ({ ...x, vat_exempt: e.target.checked }))} style={{ marginTop: 3 }} />
        <span>{L("Isento de IVA", "VAT exempt")}<span className="muted small" style={{ display: "block" }}>{L("Fisioterapia com fim terapêutico, feita por fisioterapeuta (art. 9.º, n.º 1 do CIVA). Aulas de fitness pagam IVA a 23 %.", "Therapeutic physiotherapy by a physiotherapist (art. 9(1) CIVA). Fitness-style classes pay 23 % VAT.")}</span></span></label>
      {product && <label className="check"><input type="checkbox" checked={f.active} onChange={(e) => setF((x) => ({ ...x, active: e.target.checked }))} />{L("À venda", "On sale")}</label>}
    </Sheet>
  );
}

/** Owner: someone's fixed monthly pay, and record a salary paid for a month. */
export function SalarySheet({ row, month, clinicBp }: { row: SalaryRow; month: string; clinicBp: number }) {
  const { L, lang, toast } = useApp();
  const [fixed, setFixed] = useState(centsField(row.fixed_cents, lang));
  const [pay, setPay] = useState(row.paid.length === 0);
  const [gross, setGross] = useState(centsField(row.fixed_cents, lang));
  const [paidOn, setPaidOn] = useState(ymd(new Date()));
  const [method, setMethod] = useState<"transfer" | "cash">("transfer");
  // Employer Social Security (audit phase C): the clinic's rate, or this person's own (reduced, or 0 if not on payroll).
  const [own, setOwn] = useState(row.employer_ss_bp !== null);
  const [rate, setRate] = useState(row.employer_ss_bp !== null ? String(row.employer_ss_bp / 100).replace(".", lang === "pt" ? "," : ".") : "");
  const [ssText, setSsText] = useState<string | null>(null); // null = worked out from the rate
  const f = parseEuros(fixed), g = parseEuros(gross);
  const ownBp = own ? parseRate(rate) : null;
  const bp = own ? ownBp : clinicBp;
  const ssAuto = g && bp !== null ? employerSs(g, bp) : 0;
  const ss = ssText === null ? ssAuto : parseEuros(ssText);
  const rateBad = own && ownBp === null;
  const submit = async () => {
    const newBp = own ? ownBp : null;
    if ((f !== null && f !== row.fixed_cents) || newBp !== row.employer_ss_bp) await api("PUT", `/staff/${row.staff_id}/pay`, { model: "fixed", fixed_cents: f ?? row.fixed_cents, employer_ss_bp: newBp });
    if (pay) await api("POST", "/finance/salaries", { staff_id: row.staff_id, month, gross_cents: g, paid_on: paidOn, method, ...(ssText !== null ? { employer_ss_cents: ss } : {}) });
    toast(pay ? L("Salário registado", "Salary recorded") : L("Remuneração atualizada", "Pay updated"));
  };
  return (
    <Sheet title={L("Salário", "Salary")} sub={row.name} action={L("Guardar", "Save")} onAction={submit} disabled={f === null || rateBad || (pay && (!(g && g > 0) || ss === null || ss < 0))}>
      <Field label={L("Remuneração fixa mensal, bruta (€)", "Fixed monthly pay, gross (€)")}><Money value={fixed} onChange={setFixed} label={L("Remuneração", "Pay")} /></Field>
      <p className="small muted" style={{ marginTop: -6, marginBottom: 12 }}>{L("Comissão e aluguer de sala chegam mais tarde.", "Commission and room rental arrive later.")}</p>
      <Field label={L("Segurança Social, entidade patronal", "Employer Social Security")} error={rateBad ? L("Escreva uma percentagem entre 0 e 100, como 23,75.", "Type a percentage between 0 and 100, like 23.75.") : undefined}>
        <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <select className="input" value={own ? "own" : "clinic"} onChange={(e) => { setOwn(e.target.value === "own"); setSsText(null); }} aria-label={L("Taxa", "Rate")}>
            <option value="clinic">{L(`Taxa da clínica (${pctBp(clinicBp, lang)})`, `Clinic rate (${pctBp(clinicBp, lang)})`)}</option>
            <option value="own">{L("Outra taxa para esta pessoa", "Another rate for this person")}</option>
          </select>
          {own && <><input className="input num" style={{ width: 80 }} inputMode="decimal" value={rate} onChange={(e) => { setRate(e.target.value); setSsText(null); }} aria-label={L("Percentagem", "Percentage")} placeholder="23,75" /><span className="muted">%</span></>}
        </span>
      </Field>
      <p className="small muted" style={{ marginTop: -6, marginBottom: 12 }}>{L("Use 0 para quem não está nos salários (por exemplo, recibos verdes).", "Use 0 for someone not on payroll (for example, self-employed).")}</p>
      <label className="check"><input type="checkbox" checked={pay} onChange={(e) => setPay(e.target.checked)} />{L("Registar um pagamento deste mês", "Record a payment for this month")}</label>
      {pay && (
        <div className="grid3">
          <Field label={L("Valor bruto (€)", "Gross amount (€)")}><Money value={gross} onChange={(v) => { setGross(v); setSsText(null); }} label={L("Valor", "Amount")} /></Field>
          <Field label={L("Segurança Social (€)", "Social Security (€)")} error={ss === null ? L("Escreva o valor como 237,50.", "Type the amount like 237.50.") : undefined}>
            <Money value={ssText ?? centsField(ssAuto, lang)} onChange={setSsText} label={L("Segurança Social", "Social Security")} /></Field>
          <Field label={L("Pago a", "Paid on")}><DateInput className="input" value={paidOn} onChange={(e) => setPaidOn(e.target.value)} /></Field>
          <Field label={L("Método", "Method")}>
            <select className="input" value={method} onChange={(e) => setMethod(e.target.value as "transfer" | "cash")}><option value="transfer">{L("Transferência", "Transfer")}</option><option value="cash">{L("Numerário", "Cash")}</option></select>
          </Field>
        </div>
      )}
    </Sheet>
  );
}

/** "23,75" or "23.75" → 2375 basis points; null if it isn't a number from 0 to 100 with up to two decimals. */
export const parseRate = (t: string): number | null => {
  const x = t.trim().replace(",", ".").replace(/\s*%$/, "");
  if (!/^\d{1,3}(\.\d{1,2})?$/.test(x)) return null;
  const bp = Math.round(Number(x) * 100);
  return bp >= 0 && bp <= 10000 ? bp : null;
};

/** Owner: the clinic's employer Social Security rate (audit phase C). New salaries use it; recorded ones keep theirs. */
export function SsRateSheet({ bp }: { bp: number }) {
  const { L, lang, toast } = useApp();
  const [t, setT] = useState(String(bp / 100).replace(".", lang === "pt" ? "," : "."));
  const v = parseRate(t);
  return (
    <Sheet title={L("Segurança Social, entidade patronal", "Employer Social Security")} sub={L("A taxa geral em 2026 é 23,75 %. Se a lei mudar, altere aqui: só conta para os salários registados a partir de agora.", "The general rate in 2026 is 23.75 %. If the law changes, change it here: it only applies to salaries recorded from now on.")}
      action={L("Guardar", "Save")} disabled={v === null || v === bp}
      onAction={async () => { await api("PUT", "/finance/salaries/rate", { employer_ss_bp: v }); toast(L(`Taxa alterada para ${pctBp(v!, "pt")}`, `Rate changed to ${pctBp(v!, "en")}`)); }}>
      <Field label={L("Taxa (%)", "Rate (%)")} error={v === null && t.trim() ? L("Escreva uma percentagem entre 0 e 100, como 23,75.", "Type a percentage between 0 and 100, like 23.75.") : undefined}>
        <input className="input num" inputMode="decimal" value={t} onChange={(e) => setT(e.target.value)} autoFocus style={{ maxWidth: 120 }} /></Field>
    </Sheet>
  );
}
