// Finance (owner only; the server refuses everyone else): overview, payments in, what's owed,
// prices and packages, expenses, cash close, salaries, month close and the accountant export.
import { useEffect, useState } from "react";
import {
  Banknote, Calculator, Check, ChevronLeft, ChevronRight, Coins, FileArchive, FileText, Folder, HandCoins, Lock, LockOpen, MoreHorizontal,
  Package, Pencil, Plus, Receipt, Sparkles, TrendingUp, Download, Eye,
} from "lucide-react";
import { useApp, eur } from "../lib/ctx";
import { api, download, isErr } from "../lib/api";
import { useApi, fmtDate, ymd } from "../lib/data";
import {
  c2e, centsField, parseEuros, pctBp, monthLabel, shiftMonth, thisMonth, methodLabel, categoryLabel, owedLabel, productName, METHODS,
  type CashDay, type Expense, type Method, type MonthClose, type Overview as OverviewT, type Owed, type Payment, type Product, type Salaries as SalariesT,
} from "../lib/money";
import { Layout, SubItem } from "../components/Layout";
import { Badge, Bars, Empty, Head, SortTable, DateInput } from "../components/ui";
import { ExpenseSheet, FileViewer, InvoiceSheet, InvoiceViewer, ProductSheet, RecordPayment, SalarySheet, SsRateSheet, VoidPayment } from "../components/MoneySheets";
import { AiCard, aiOffText, useAiStatus, type AiState } from "../components/Ai";
import { openMenu } from "../components/menus";

type Sec = "overview" | "payments" | "owed" | "packages" | "expenses" | "cash" | "salaries" | "months" | "reports" | "export";
/** Other parts of the app can open a Finance section (File › Accountant Export…). */
export const FIN_SECTION = "physiopulse:finance-section";

export default function Finance() {
  const { L, lang, role, openSheet } = useApp();
  const [sec, setSec] = useState<Sec>("overview");
  const [month, setMonth] = useState(thisMonth());
  useEffect(() => {
    const f = (e: Event) => setSec((e as CustomEvent<Sec>).detail);
    window.addEventListener(FIN_SECTION, f);
    return () => window.removeEventListener(FIN_SECTION, f);
  }, []);
  const owner = role === "owner";
  const { data: ov } = useApi<OverviewT>(owner ? `/finance/overview?month=${month}` : null);
  const { data: owed } = useApi<Owed[]>(owner ? "/finance/owed" : null);
  if (!owner) {
    return (
      <Layout label={L("Finanças", "Finance")}>
        <Empty icon={<Lock />} title={L("Só o proprietário vê as finanças", "Only the owner can see finances")} text={L("Pode registar pagamentos na ficha de cada utente ou em Ficheiro › Registar pagamento.", "You can record payments in each patient’s file or with File › Record Payment.")} />
      </Layout>
    );
  }
  const owedPatients = new Set(owed?.map((o) => o.patient_id)).size;
  const secs: [Sec, JSX.Element, string, number?][] = [
    ["overview", <TrendingUp />, L("Resumo", "Overview")], ["payments", <Banknote />, L("Recebimentos", "Payments In")],
    ["owed", <HandCoins />, L("Por receber", "To Collect"), owedPatients], ["packages", <Package />, L("Preços e pacotes", "Prices and Packages")],
    ["expenses", <Receipt />, L("Despesas", "Expenses")], ["cash", <Calculator />, L("Fecho de caixa", "Cash Close")],
    ["salaries", <Coins />, L("Salários", "Salaries")], ["months", <Lock />, L("Fecho do mês", "Month Close")],
    ["export", <FileArchive />, L("Exportar", "Export")], ["reports", <Sparkles />, L("Relatórios IA", "AI Reports")],
  ];
  const sub = secs.map(([k, ic, t, n]) => <SubItem key={k} icon={ic} on={sec === k} onClick={() => setSec(k)} count={n}>{t}</SubItem>);
  const monthNav = (
    <div className="seg" role="group" aria-label={L("Mês", "Month")} style={{ display: "inline-flex", alignItems: "center" }}>
      <button className="btn icon sm more" aria-label={L("Mês anterior", "Previous month")} onClick={() => setMonth((m) => shiftMonth(m, -1))}><ChevronLeft /></button>
      <span style={{ minWidth: 130, textAlign: "center", fontWeight: 500 }}>{monthLabel(month, lang).replace(/^./, (c) => c.toUpperCase())}</span>
      <button className="btn icon sm more" aria-label={L("Mês seguinte", "Next month")} disabled={month >= thisMonth()} onClick={() => setMonth((m) => shiftMonth(m, 1))}><ChevronRight /></button>
    </div>
  );
  return (
    <Layout label={L("Finanças", "Finance")} sub={sub}>
      <div className="wrap">{
        sec === "overview" ? <Overview ov={ov} owed={owed} nav={monthNav} openOwed={() => setSec("owed")} /> : sec === "payments" ? <PaymentsIn month={month} nav={monthNav} /> : sec === "owed" ? <ToCollect owed={owed} /> :
          sec === "packages" ? <Packages /> : sec === "expenses" ? <Expenses month={month} nav={monthNav} /> : sec === "cash" ? <CashClose /> :
            sec === "salaries" ? <Salaries month={month} nav={monthNav} /> : sec === "months" ? <Months /> : sec === "export" ? <Export /> : <Reports month={month} nav={monthNav} />
      }</div>
    </Layout>
  );
}

/** A figure in the summary line under the title (no card). */
function Fact({ label, value, note, tone, onClick }: { label: string; value: string; note?: string; tone?: "down"; onClick?: () => void }) {
  const inner = <><span className="l">{label}</span><span className={`v ${tone ?? ""}`}>{value}</span>{note && <span className="d">{note}</span>}</>;
  return onClick ? <button className="fact" onClick={onClick}>{inner}</button> : <div className="fact">{inner}</div>;
}

function topOwing(list: Owed[]) {
  const by = new Map<string, { id: string; name: string; due: number }>();
  for (const o of list) { const x = by.get(o.patient_id) ?? { id: o.patient_id, name: o.patient_name, due: 0 }; x.due += o.due_cents; by.set(o.patient_id, x); }
  return [...by.values()].sort((a, b) => b.due - a.due);
}
const pct = (a: number, b: number) => (b ? Math.round(((a - b) / b) * 1000) / 10 : null);

function Overview({ ov, owed, nav, openOwed }: { ov?: OverviewT; owed?: Owed[]; nav: JSX.Element; openOwed: () => void }) {
  const { L, lang, openSheet, openPatient } = useApp();
  if (!ov) return <><Head title={L("Resumo", "Overview")}>{nav}</Head><div className="skel" style={{ height: 120 }} /></>;
  // A month in progress isn't compared with a whole month (that reads like a crash on the 4th).
  const inProgress = ov.month === thisMonth() && !ov.closed;
  const sameDayDelta = inProgress ? null : pct(ov.revenue_cents, ov.previous.revenue_cents);
  const total = Object.values(ov.by_method).reduce((a, b) => a + (b ?? 0), 0);
  return (
    <>
      <Head title={L("Resumo", "Overview")} sub={ov.closed ? L("Mês fechado: os valores já não mudam.", "Month closed: these figures won’t change.") : L("Mês aberto", "Month open")}>
        {nav}
        <button className="btn" onClick={() => openSheet(<RecordPayment />)}><Receipt />{L("Registar pagamento…", "Record Payment…")}</button>
      </Head>
      <div className="facts">
        <Fact label={L("Receita", "Revenue")} value={eur(c2e(ov.revenue_cents), lang)}
          note={sameDayDelta === null ? L(`${ov.payments} pagamento${ov.payments === 1 ? "" : "s"}${inProgress ? " até hoje" : ""}`, `${ov.payments} payment${ov.payments === 1 ? "" : "s"}${inProgress ? " so far" : ""}`) : L(`${sameDayDelta > 0 ? "+" : ""}${String(sameDayDelta).replace(".", ",")}% vs. mês anterior`, `${sameDayDelta > 0 ? "+" : ""}${sameDayDelta}% vs previous month`)} />
        <Fact label={L("Despesas e salários", "Expenses and salaries")} value={eur(c2e(ov.expenses_cents + ov.salaries_cents + (ov.employer_ss_cents ?? 0)), lang)} note={L(`salários ${eur(c2e(ov.salaries_cents + (ov.employer_ss_cents ?? 0)), lang)}${ov.employer_ss_cents ? " com Seg. Social" : ""}`, `salaries ${eur(c2e(ov.salaries_cents + (ov.employer_ss_cents ?? 0)), lang)}${ov.employer_ss_cents ? " incl. Social Security" : ""}`)} />
        <Fact label={L("Resultado", "Profit")} value={eur(c2e(ov.result_cents), lang)} tone={ov.result_cents < 0 ? "down" : undefined}
          note={ov.revenue_cents > 0 && ov.result_cents >= 0 ? L(`${Math.round((ov.result_cents / ov.revenue_cents) * 100)}% da receita`, `${Math.round((ov.result_cents / ov.revenue_cents) * 100)}% of revenue`) : undefined} />
        <Fact label={L("Por receber", "To collect")} value={eur(c2e(ov.owed_cents), lang)} note={L(`${ov.owed_patients} utente${ov.owed_patients === 1 ? "" : "s"}`, `${ov.owed_patients} patient${ov.owed_patients === 1 ? "" : "s"}`)} onClick={openOwed} />
      </div>
      <div className="grid2" style={{ gridTemplateColumns: "minmax(0,1.6fr) minmax(0,1fr)" }}>
        <div className="card pad"><h3 style={{ marginBottom: 10 }}>{L("Últimos 6 meses", "Last 6 months")}</h3>
          <Bars data={ov.series.map((m) => ({ m: monthLabel(m.month, lang, true), a: c2e(m.revenue_cents), b: c2e(m.out_cents) }))} labelA={L("Receita", "Revenue")} labelB={L("Despesas e salários", "Expenses and salaries")} partial={inProgress ? L(`${monthLabel(ov.month, "pt", true)} ainda em curso`, `${monthLabel(ov.month, "en", true)} still in progress`) : undefined} /></div>
        <div className="card pad"><h3 style={{ marginBottom: 12 }}>{L("Por método de pagamento", "By payment method")}</h3>
          {total === 0 && <p className="small muted">{L("Ainda sem pagamentos este mês.", "No payments this month yet.")}</p>}
          {METHODS.filter(([m]) => ov.by_method[m]).map(([m]) => {
            const v = ov.by_method[m] ?? 0, p = Math.round((v / total) * 100);
            return (
              <div key={m} style={{ marginBottom: 12 }}>
                <div style={{ display: "flex", justifyContent: "space-between", marginBottom: 5 }}><span>{methodLabel(m, lang)}</span><span className="num muted">{eur(c2e(v), lang)} · {p}%</span></div>
                <div className="bar" role="img" aria-label={`${methodLabel(m, lang)} ${p}%`}><i style={{ width: `${p}%` }} /></div>
              </div>
            );
          })}
          {ov.uninvoiced > 0 && <p className="small" style={{ marginTop: 6 }}><Badge tone="warn">{L(`${ov.uninvoiced} sem fatura`, `${ov.uninvoiced} not invoiced`)}</Badge> <span className="muted">{eur(c2e(ov.uninvoiced_cents), lang)}</span></p>}
        </div>
      </div>
      {owed && owed.length > 0 && (
        <>
          <div className="section-title"><h2>{L("Mais em dívida", "Owing most")}</h2><button className="linkbtn" onClick={openOwed}>{L("Ver tudo", "See All")}</button></div>
          <div className="rows">
            {topOwing(owed).slice(0, 5).map((o) => (
              <button key={o.id} className="row" onClick={() => openPatient(o.id)}><span className="grow">{o.name}</span><span className="amt">{eur(c2e(o.due), lang)}</span><ChevronRight size={14} className="chev" /></button>
            ))}
          </div>
        </>
      )}
    </>
  );
}

/** The ⋯ menu on a payment: invoice, void. */
function paymentMenu(e: React.MouseEvent, p: Payment, ctx: ReturnType<typeof useApp>) {
  const { L, openSheet } = ctx;
  if (p.voided_at) return;
  openMenu(ctx, e, [
    { label: p.invoice_number ? L("Editar fatura…", "Edit Invoice…") : L("Registar fatura…", "Add Invoice…"), icon: <FileText />, fn: () => openSheet(<InvoiceSheet payment={p} />) },
    ...(p.invoice_file_id ? [{ label: L("Ver fatura", "View Invoice"), icon: <Eye />, fn: () => openSheet(<InvoiceViewer payment={p} />) }] : []),
    ...(p.invoice_file_id ? [{ label: L("Descarregar PDF da fatura", "Download Invoice PDF"), icon: <Download />, fn: () => { download(p.invoice_file_id!, `${p.invoice_number.replace(/\//g, "-") || "fatura"}.pdf`).then((where) => ctx.toast(L(`Guardado em ${where}`, `Saved to ${where}`))).catch((x) => ctx.toast(isErr(x) ? x.message : String(x))); } }] : []),
    "-",
    { label: L("Anular pagamento…", "Void Payment…"), danger: true, fn: () => openSheet(<VoidPayment payment={p} />) },
  ]);
}

function PaymentsIn({ month, nav }: { month: string; nav: JSX.Element }) {
  const ctx = useApp();
  const { L, lang, openSheet } = ctx;
  const [only, setOnly] = useState(false);
  const last = new Date(+month.slice(0, 4), +month.slice(5, 7), 0);
  const { data } = useApi<Payment[]>(`/payments?from=${month}-01&to=${ymd(last)}${only ? "&uninvoiced=true" : ""}`);
  return (
    <>
      <Head title={L("Recebimentos", "Payments In")} sub={L("Clique num cabeçalho para ordenar. Clique numa linha para a fatura.", "Click a heading to sort. Click a row for its invoice.")}>
        {nav}
        <label className="check" style={{ margin: 0 }}><input type="checkbox" checked={only} onChange={(e) => setOnly(e.target.checked)} />{L("Só sem fatura", "Not invoiced only")}</label>
      </Head>
      {!data ? <div className="skel" style={{ height: 120 }} /> : data.length === 0 ? (
        <Empty icon={<Banknote />} title={only ? L("Tudo faturado", "Everything is invoiced") : L("Sem pagamentos neste mês", "No payments this month")}
          action={<button className="btn" onClick={() => openSheet(<RecordPayment />)}><Receipt />{L("Registar pagamento…", "Record Payment…")}</button>} />
      ) : (
        <SortTable initial="d" rows={data} cols={[
          { key: "d", label: L("Data", "Date"), sort: (r) => r.received_on + String(r.number).padStart(8, "0"), cell: (r) => <span className="num">{fmtDate(r.received_on, lang, { day: "2-digit", month: "2-digit" })}</span> },
          { key: "p", label: L("Utente", "Patient"), sort: (r) => r.patient_name ?? "", cell: (r) => r.patient_name ? <span style={{ whiteSpace: "nowrap" }}>{r.patient_name}</span> : <span className="muted">{L("Avulso", "Walk-in")}</span> },
          { key: "desc", label: L("Descrição", "Description"), cell: (r) => <span style={r.voided_at ? { textDecoration: "line-through", color: "var(--text3)" } : undefined}>{r.description}</span> },
          { key: "m", label: L("Método", "Method"), sort: (r) => r.method, cell: (r) => methodLabel(r.method, lang) },
          { key: "inv", label: L("Fatura", "Invoice"), sort: (r) => (r.voided_at ? 2 : r.invoice_number ? 1 : 0), cell: (r) => r.voided_at ? <Badge tone="bad">{L("Anulado", "Voided")}</Badge>
            : r.invoice_number ? <button className="linkbtn" style={{ whiteSpace: "nowrap" }} onClick={() => openSheet(<InvoiceSheet payment={r} />)}>{r.invoice_number}</button>
              : <button className="btn sm" onClick={() => openSheet(<InvoiceSheet payment={r} />)}>{L("Registar fatura…", "Add Invoice…")}</button> },
          { key: "v", label: L("Valor", "Amount"), right: true, sort: (r) => r.amount_cents, cell: (r) => <span className="amt" style={r.voided_at ? { textDecoration: "line-through", color: "var(--text3)" } : undefined}>{eur(c2e(r.amount_cents), lang)}</span> },
          { key: "x", label: "", cell: (r) => r.voided_at ? null : <button className="btn icon sm more" aria-label={L("Mais ações", "More actions")} onClick={(e) => paymentMenu(e, r, ctx)}><MoreHorizontal /></button> },
        ]} />
      )}
      <p className="small muted" style={{ marginTop: 10 }}>{L("As faturas são emitidas no programa de faturação certificado; aqui fica o número e o PDF. Os pagamentos nunca são apagados: anulam-se com um motivo.", "Invoices are issued in your certified invoicing software; the number and PDF are kept here. Payments are never deleted: they’re voided with a reason.")}</p>
    </>
  );
}

function ToCollect({ owed }: { owed?: Owed[] }) {
  const { L, lang, openSheet, openPatient } = useApp();
  return (
    <>
      <Head title={L("Por receber", "To Collect")} sub={L("Sessões realizadas sem pacote, pacotes por pagar e mensalidades em falta.", "Sessions done without a package, unpaid packages and monthly plans not yet paid.")} />
      {!owed ? <div className="skel" style={{ height: 120 }} /> : owed.length === 0 ? (
        <Empty icon={<Check />} title={L("Nada por receber", "Nothing to collect")} text={L("Todos os utentes estão em dia.", "Every patient is up to date.")} />
      ) : (
        <SortTable initial="d" rows={owed} cols={[
          { key: "d", label: L("Data", "Date"), sort: (r) => r.day, cell: (r) => <span className="num">{fmtDate(r.day, lang, { day: "2-digit", month: "2-digit" })}</span> },
          { key: "p", label: L("Utente", "Patient"), sort: (r) => r.patient_name, cell: (r) => <button className="linkbtn" style={{ whiteSpace: "nowrap" }} onClick={() => openPatient(r.patient_id)}>{r.patient_name}</button> },
          { key: "w", label: L("O quê", "What"), cell: (r) => owedLabel(r, lang) },
          { key: "v", label: L("Em falta", "Due"), right: true, sort: (r) => r.due_cents, cell: (r) => <span className="amt">{eur(c2e(r.due_cents), lang)}</span> },
          { key: "x", label: "", cell: (r) => <button className="btn sm" onClick={() => openSheet(<RecordPayment patientId={r.patient_id} appointmentId={r.appointment_id ?? undefined} />)}>{L("Receber…", "Collect…")}</button> },
        ]} />
      )}
    </>
  );
}

function Packages() {
  const { L, lang, openSheet } = useApp();
  const { data } = useApi<Product[]>("/products?all=true");
  const single = data?.find((p) => p.kind === "visit" && p.visit_kind === "session" && p.active)?.price_cents ?? 2000;
  const classOne = data?.find((p) => p.kind === "visit" && p.visit_kind === "class" && p.active)?.price_cents ?? 1000;
  const per = (r: Product) => (r.sessions && r.kind !== "visit" ? r.price_cents / r.sessions : null);
  return (
    <>
      <Head title={L("Preços e pacotes", "Prices and Packages")} sub={L("Quem já comprou um pacote mantém o preço que pagou.", "People who already bought a package keep the price they paid.")}>
        <button className="btn" onClick={() => openSheet(<ProductSheet />)}><Plus />{L("Novo produto…", "New Product…")}</button>
      </Head>
      {!data ? <div className="skel" style={{ height: 120 }} /> : (
        <SortTable rows={data} cols={[
          { key: "n", label: L("Produto", "Product"), sort: (r) => productName(r, lang), cell: (r) => <span style={r.active ? undefined : { color: "var(--text3)" }}>{productName(r, lang)}{!r.active && <> <Badge>{L("Retirado", "Retired")}</Badge></>}</span> },
          { key: "p", label: L("Preço", "Price"), right: true, sort: (r) => r.price_cents, cell: (r) => <span className="amt">{eur(c2e(r.price_cents), lang)}{r.kind === "monthly" ? L("/mês", "/month") : ""}</span> },
          { key: "ps", label: L("Por sessão", "Per session"), right: true, cell: (r) => per(r) !== null ? <span className="num">{eur(Math.round(per(r)!) / 100, lang)}</span> : <span className="muted">—</span> },
          { key: "s", label: L("Poupança", "Saving"), right: true, cell: (r) => {
            const p = per(r); if (p === null) return <span className="muted">—</span>;
            const saving = 1 - p / (r.applies_to === "class" ? classOne : single);
            return saving > 0.005 ? <Badge tone="ok">{Math.round(saving * 100)}%</Badge> : <Badge tone="warn">{L("Nenhuma", "None")}</Badge>;
          } },
          { key: "iva", label: L("IVA", "VAT"), sort: (r) => Number(r.vat_exempt), cell: (r) => r.vat_exempt ? <span className="muted">{L("Isento", "Exempt")}</span> : <Badge tone="warn">23 %</Badge> },
          { key: "v", label: L("Validade", "Valid for"), cell: (r) => r.kind === "monthly" ? L("Mensal", "Monthly") : r.valid_months ? L(`${r.valid_months} ${r.valid_months === 1 ? "mês" : "meses"}`, `${r.valid_months} month${r.valid_months === 1 ? "" : "s"}`) : <span className="muted">—</span> },
          { key: "x", label: "", cell: (r) => <button className="btn icon sm more" aria-label={L(`Editar ${productName(r, lang)}`, `Edit ${productName(r, lang)}`)} onClick={() => openSheet(<ProductSheet product={r} />)}><Pencil /></button> },
        ]} />
      )}
      <p className="small muted" style={{ marginTop: 10 }}>{L(`Poupança calculada contra a sessão a ${eur(c2e(single), lang)} e a aula a ${eur(c2e(classOne), lang)}.`, `Savings are against the ${eur(c2e(single), lang)} single session and the ${eur(c2e(classOne), lang)} drop-in class.`)}</p>
      <p className="small muted" style={{ marginTop: 4 }}>{L("Isento: fisioterapia com fim terapêutico (art. 9.º, n.º 1 do CIVA). As aulas começam com IVA a 23 %; marque como isenta uma aula terapêutica dada por fisioterapeuta, como o Pilates clínico. Confirme com o contabilista.",
        "Exempt: therapeutic physiotherapy (art. 9(1) CIVA). Classes start at 23 % VAT; mark a class exempt when it's therapeutic and given by a physiotherapist, such as clinical Pilates. Check with your accountant.")}</p>
    </>
  );
}

function Expenses({ month, nav }: { month: string; nav: JSX.Element }) {
  const ctx = useApp();
  const { L, lang, openSheet, confirm, toast } = ctx;
  const last = new Date(+month.slice(0, 4), +month.slice(5, 7), 0);
  const { data } = useApi<Expense[]>(`/expenses?from=${month}-01&to=${ymd(last)}`);
  const total = data?.reduce((t, e) => t + e.amount_cents, 0) ?? 0;
  const more = (e: React.MouseEvent, r: Expense) => openMenu(ctx, e, [
    { label: L("Editar…", "Edit…"), icon: <Pencil />, fn: () => openSheet(<ExpenseSheet expense={r} />) },
    ...(r.file_id ? [{ label: L("Descarregar talão", "Download Receipt"), icon: <Download />, fn: () => { download(r.file_id!, r.filename ?? "talao").then((w) => toast(L(`Guardado em ${w}`, `Saved to ${w}`))).catch((x) => toast(isErr(x) ? x.message : String(x))); } }] : []),
    "-",
    { label: L("Apagar…", "Delete…"), danger: true, fn: () => confirm({ title: L("Apagar esta despesa?", "Delete this expense?"), text: `${r.supplier || categoryLabel(r.category, lang)} · ${eur(c2e(r.amount_cents), lang)}`, action: L("Apagar", "Delete"), destructive: true, hold: true,
      onConfirm: () => { api("DELETE", `/expenses/${r.id}`).then(() => toast(L("Despesa apagada", "Expense deleted"))).catch((x) => toast(isErr(x) ? x.message : String(x))); } }) },
  ]);
  return (
    <>
      <Head title={L("Despesas", "Expenses")} sub={data?.length ? L(`${data.length} despesas · ${eur(c2e(total), lang)}`, `${data.length} expenses · ${eur(c2e(total), lang)}`) : undefined}>
        {nav}
        <button className="btn" onClick={() => openSheet(<ExpenseSheet />)}><Plus />{L("Nova despesa…", "New Expense…")}</button>
      </Head>
      {!data ? <div className="skel" style={{ height: 120 }} /> : data.length === 0 ? (
        <Empty icon={<Receipt />} title={L("Sem despesas neste mês", "No expenses this month")} text={L("Registe compras, renda e contas, com a foto ou o PDF do talão.", "Record purchases, rent and bills, with a photo or PDF of the receipt.")}
          action={<button className="btn" onClick={() => openSheet(<ExpenseSheet />)}><Plus />{L("Nova despesa…", "New Expense…")}</button>} />
      ) : (
        <SortTable initial="d" rows={data} cols={[
          { key: "d", label: L("Data", "Date"), sort: (r) => r.spent_on, cell: (r) => <span className="num">{fmtDate(r.spent_on, lang, { day: "2-digit", month: "2-digit" })}</span> },
          { key: "s", label: L("Fornecedor", "Supplier"), sort: (r) => r.supplier, cell: (r) => <>{r.supplier || <span className="muted">{L("Sem nome", "No name")}</span>}{r.ai_read && <> <span title={L("Lido do talão pela IA local", "Read from the receipt by the local AI")}><Badge tone="ai"><Sparkles size={10} style={{ verticalAlign: -1 }} aria-label={L("Lido pela IA", "Read by AI")} /></Badge></span></>}</> },
          { key: "c", label: L("Categoria", "Category"), sort: (r) => categoryLabel(r.category, lang), cell: (r) => categoryLabel(r.category, lang) },
          { key: "f", label: L("Talão", "Receipt"), sort: (r) => (r.file_id ? 1 : 0), cell: (r) => r.file_id ? <Badge tone="ok"><FileText size={11} />{L("Anexado", "Attached")}</Badge> : <span className="muted">—</span> },
          { key: "v", label: L("Valor", "Amount"), right: true, sort: (r) => r.amount_cents, cell: (r) => <span className="amt">{eur(c2e(r.amount_cents), lang)}</span> },
          { key: "x", label: "", cell: (r) => r.closed ? <Lock size={14} aria-label={L("Mês fechado", "Month closed")} className="muted" /> : <button className="btn icon sm more" aria-label={L("Mais ações", "More actions")} onClick={(e) => more(e, r)}><MoreHorizontal /></button> },
        ]} />
      )}
    </>
  );
}

function CashClose() {
  const { L, lang, toast } = useApp();
  const [day, setDay] = useState(ymd(new Date()));
  const { data } = useApi<CashDay>(`/finance/cash?day=${day}`);
  const [counted, setCounted] = useState<Record<string, string>>({});
  const [note, setNote] = useState("");
  const methods = METHODS.filter(([m]) => m !== "stripe" || data?.expected.stripe).map(([m]) => m);
  useEffect(() => {
    if (!data) return;
    const src = data.close?.counted ?? data.expected;
    setCounted(Object.fromEntries(methods.map((m) => [m, src[m] ? centsField(src[m]!, lang) : ""])));
    setNote(data.close?.note ?? "");
  }, [data]); // eslint-disable-line react-hooks/exhaustive-deps
  const exp = (m: Method) => data?.expected[m] ?? 0;
  const cnt = (m: Method) => parseEuros(counted[m] ?? "") ?? 0;
  const diff = methods.reduce((t, m) => t + cnt(m) - exp(m), 0);
  const valid = methods.every((m) => !counted[m] || parseEuros(counted[m]) !== null);
  const close = async () => {
    try {
      const r = await api<{ difference_cents: number }>("POST", "/finance/cash", { day, note, counted: Object.fromEntries(methods.map((m) => [m, cnt(m)])) });
      toast(r.difference_cents === 0 ? L("Caixa fechada, sem diferenças", "Day closed, no difference") : L(`Caixa fechada com diferença de ${eur(c2e(r.difference_cents), lang)}`, `Day closed with a ${eur(c2e(r.difference_cents), lang)} difference`));
    } catch (e) { toast(isErr(e) ? e.message : String(e)); }
  };
  return (
    <>
      <Head title={L("Fecho de caixa", "Cash Close")} sub={data ? L(`${data.payments} pagamentos neste dia`, `${data.payments} payments that day`) : undefined}>
        <DateInput className="input" value={day} max={ymd(new Date())} onChange={(e) => setDay(e.target.value)} aria-label={L("Dia", "Day")} style={{ width: "auto" }} />
      </Head>
      {data?.close && <p className="small" style={{ marginBottom: 12 }}><Badge tone="ok"><Check size={11} />{L("Fechada", "Closed")}</Badge> <span className="muted">{L(`por ${data.close.closed_by_name ?? "—"}, ${fmtDate(data.close.closed_at, lang, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })}. Pode fechar de novo para corrigir.`, `by ${data.close.closed_by_name ?? "—"}, ${fmtDate(data.close.closed_at, lang, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })}. You can close it again to correct it.`)}</span></p>}
      <div className="tbl-wrap"><table>
        <thead><tr><th className="plain">{L("Método", "Method")}</th><th className="plain r">{L("Esperado", "Expected")}</th><th className="plain r">{L("Contado", "Counted")}</th><th className="plain r">{L("Diferença", "Difference")}</th></tr></thead>
        <tbody>{methods.map((m) => {
          const d = cnt(m) - exp(m);
          return (
            <tr key={m}><td>{methodLabel(m, lang)}</td><td className="r amt">{eur(c2e(exp(m)), lang)}</td>
              <td className="r"><input className="input num" style={{ width: 110, textAlign: "right" }} inputMode="decimal" value={counted[m] ?? ""} onChange={(e) => setCounted((c) => ({ ...c, [m]: e.target.value }))} aria-label={`${L("Contado", "Counted")} ${methodLabel(m, lang)}`} /></td>
              <td className="r amt" style={{ color: d ? "var(--bad)" : undefined }}>{eur(c2e(d), lang)}</td></tr>
          );
        })}
          <tr><td><b>{L("Total", "Total")}</b></td><td className="r amt"><b>{eur(c2e(methods.reduce((t, m) => t + exp(m), 0)), lang)}</b></td><td className="r amt"><b>{eur(c2e(methods.reduce((t, m) => t + cnt(m), 0)), lang)}</b></td>
            <td className="r amt" style={{ color: diff ? "var(--bad)" : undefined }}><b>{eur(c2e(diff), lang)}</b></td></tr>
        </tbody>
      </table></div>
      <label className="field" style={{ marginTop: 12, maxWidth: 520 }}><span>{L("Nota (opcional)", "Note (optional)")}</span><input className="input" value={note} onChange={(e) => setNote(e.target.value)} placeholder={L("Ex.: troco dado a mais", "E.g. too much change given")} /></label>
      <button className="btn pri" style={{ marginTop: 6 }} disabled={!data || !valid} onClick={close}><Check />{data?.close ? L("Fechar caixa de novo", "Close Day Again") : L("Fechar caixa", "Close Day")}</button>
      {!!data?.recent.length && (
        <div style={{ marginTop: 24 }}>
          <h3 style={{ marginBottom: 8 }}>{L("Últimos fechos", "Recent closes")}</h3>
          <div className="rows">{data.recent.map((r) => (
            <button key={r.day} className="row" onClick={() => setDay(r.day.slice(0, 10))}><span className="grow">{fmtDate(r.day, lang, { weekday: "short", day: "numeric", month: "short" })}</span>
              {r.difference_cents ? <Badge tone="bad">{eur(c2e(r.difference_cents), lang)}</Badge> : <Badge tone="ok">{L("Certo", "Balanced")}</Badge>}</button>
          ))}</div>
        </div>
      )}
    </>
  );
}

function Salaries({ month, nav }: { month: string; nav: JSX.Element }) {
  const { L, lang, openSheet, confirm, toast } = useApp();
  const { data } = useApi<SalariesT>(`/finance/salaries?month=${month}`);
  const roleName = (r: string) => r === "owner" ? L("Proprietário", "Owner") : r === "therapist" ? L("Fisioterapeuta", "Therapist") : L("Receção", "Reception");
  return (
    <>
      <Head title={L("Salários", "Salaries")} sub={L("Valores brutos; a Segurança Social da entidade patronal soma-se ao custo. Comissão e aluguer de sala chegam mais tarde.", "Gross amounts; the employer’s Social Security is added to the cost. Commission and room rental arrive later.")}>{nav}</Head>
      {!data ? <div className="skel" style={{ height: 120 }} /> : (<>
        <div className="rows">
          {data.staff.map((s) => {
            const paid = s.paid.reduce((t, p) => t + p.gross_cents, 0);
            return (
              <div key={s.staff_id} className="row" style={{ flexWrap: "wrap" }}>
                <span className="grow"><span className="t" style={{ display: "block" }}>{s.name}</span><span className="s">{roleName(s.role)} · {s.fixed_cents ? L(`fixo ${eur(c2e(s.fixed_cents), lang)}/mês`, `fixed ${eur(c2e(s.fixed_cents), lang)}/month`) : L("sem remuneração definida", "no pay set")}
                  {s.employer_ss_bp !== null && ` · ${L("Seg. Social", "Social Security")} ${pctBp(s.employer_ss_bp, lang)}`}</span></span>
                {s.paid.map((p) => (
                  <Badge key={p.id} tone="ok">{L("Pago", "Paid")} {eur(c2e(p.gross_cents), lang)}{p.employer_ss_cents > 0 && ` + ${L("SS", "SS")} ${eur(c2e(p.employer_ss_cents), lang)}`} · {fmtDate(p.paid_on, lang, { day: "numeric", month: "short" })}
                    {!data.closed && <button className="linkbtn" style={{ marginLeft: 6 }} aria-label={L("Apagar este pagamento", "Delete this payment")}
                      onClick={() => confirm({ title: L("Apagar este pagamento de salário?", "Delete this salary payment?"), action: L("Apagar", "Delete"), destructive: true, hold: true,
                        onConfirm: () => { api("DELETE", `/finance/salaries/${p.id}`).then(() => toast(L("Pagamento apagado", "Payment deleted"))).catch((x) => toast(isErr(x) ? x.message : String(x))); } })}>×</button>}
                  </Badge>
                ))}
                {paid === 0 && s.role !== "owner" && <Badge tone="warn">{L("Por pagar", "Not paid")}</Badge>}
                <button className="btn sm" disabled={data.closed} onClick={() => openSheet(<SalarySheet row={s} month={month} clinicBp={data.employer_ss_bp} />)}>{L("Registar…", "Record…")}</button>
              </div>
            );
          })}
        </div>
        {(() => {
          const g = data.staff.reduce((t, s) => t + s.paid.reduce((u, p) => u + p.gross_cents, 0), 0);
          const ss = data.staff.reduce((t, s) => t + s.paid.reduce((u, p) => u + p.employer_ss_cents, 0), 0);
          return (
            <div className="group" style={{ marginTop: 16 }}>
              {g > 0 && <div className="lrow"><span className="grow">{L("Custo dos salários deste mês", "Salary cost this month")}<span className="small muted" style={{ display: "block" }}>{L(`${eur(c2e(g), lang)} brutos + ${eur(c2e(ss), lang)} de Segurança Social`, `${eur(c2e(g), lang)} gross + ${eur(c2e(ss), lang)} Social Security`)}</span></span><span className="num">{eur(c2e(g + ss), lang)}</span></div>}
              <div className="lrow"><span className="grow">{L("Segurança Social, entidade patronal", "Employer Social Security")}<span className="small muted" style={{ display: "block" }}>{L("Aplica-se aos salários registados a partir de agora; os já registados não mudam.", "Applies to salaries recorded from now on; those already recorded don’t change.")}</span></span>
                <span className="num">{pctBp(data.employer_ss_bp, lang)}</span>
                <button className="btn sm" onClick={() => openSheet(<SsRateSheet bp={data.employer_ss_bp} />)}>{L("Alterar…", "Change…")}</button></div>
            </div>
          );
        })()}
      </>)}
    </>
  );
}

function Months() {
  const { L, lang, confirm, toast } = useApp();
  const { data } = useApi<MonthClose[]>("/finance/months");
  const prev = shiftMonth(thisMonth(), -1);
  const { data: pov } = useApi<OverviewT>(`/finance/overview?month=${prev}`);
  const closed = (m: string) => data?.some((x) => x.month === m);
  const close = (m: string) => confirm({
    title: L(`Fechar ${monthLabel(m, lang)}?`, `Close ${monthLabel(m, lang)}?`),
    text: L("Os pagamentos, despesas e salários desse mês ficam bloqueados. Pode reabrir o mês se precisar de corrigir algo.", "That month’s payments, expenses and salaries will be locked. You can reopen the month if something needs fixing."),
    action: L("Fechar mês", "Close Month"),
    onConfirm: () => { api("POST", `/finance/months/${m}/close`).then(() => toast(L("Mês fechado", "Month closed"), () => { api("POST", `/finance/months/${m}/reopen`).catch(() => {}); }, L("Fechar mês", "Close Month"))).catch((x) => toast(isErr(x) ? x.message : String(x))); },
  });
  const reopen = (m: string) => confirm({
    title: L(`Reabrir ${monthLabel(m, lang)}?`, `Reopen ${monthLabel(m, lang)}?`),
    text: L("Se já enviou a exportação ao contabilista, envie-lhe uma nova depois de corrigir.", "If you already sent the export to your accountant, send a new one after fixing it."),
    action: L("Reabrir", "Reopen"),
    onConfirm: () => { api("POST", `/finance/months/${m}/reopen`).then(() => toast(L("Mês reaberto", "Month reopened"))).catch((x) => toast(isErr(x) ? x.message : String(x))); },
  });
  return (
    <>
      <Head title={L("Fecho do mês", "Month Close")} sub={L("Feche cada mês depois de rever tudo; depois exporte para o contabilista.", "Close each month after checking everything; then export it for your accountant.")} />
      {!closed(prev) && (
        <div className="card pad" style={{ marginBottom: 18 }}>
          <h3 style={{ marginBottom: 6 }}>{monthLabel(prev, lang).replace(/^./, (c) => c.toUpperCase())}</h3>
          {pov ? (
            <>
              {/* Plain figures, like Overview and Today (audit phase C: no stat cards). */}
              <div className="facts" style={{ marginBottom: 12 }}>
                <Fact label={L("Receita", "Revenue")} value={eur(c2e(pov.revenue_cents), lang)} />
                <Fact label={L("Despesas", "Expenses")} value={eur(c2e(pov.expenses_cents), lang)} />
                <Fact label={L("Salários", "Salaries")} value={eur(c2e(pov.salaries_cents + (pov.employer_ss_cents ?? 0)), lang)} note={pov.employer_ss_cents ? L(`com ${eur(c2e(pov.employer_ss_cents), lang)} de Seg. Social`, `incl. ${eur(c2e(pov.employer_ss_cents), lang)} Social Security`) : undefined} />
                <Fact label={L("Resultado", "Profit")} value={eur(c2e(pov.result_cents), lang)} tone={pov.result_cents < 0 ? "down" : undefined} />
              </div>
              {pov.uninvoiced > 0 && <p className="small" style={{ marginBottom: 10 }}><Badge tone="warn">{L(`${pov.uninvoiced} pagamentos sem fatura`, `${pov.uninvoiced} payments not invoiced`)}</Badge> <span className="muted">{L("Registe as faturas em Recebimentos antes de fechar.", "Add their invoices in Payments In before closing.")}</span></p>}
            </>
          ) : <div className="skel" />}
          <button className="btn pri" onClick={() => close(prev)}><Lock />{L("Fechar mês…", "Close Month…")}</button>
        </div>
      )}
      <h3 style={{ marginBottom: 8 }}>{L("Meses fechados", "Closed months")}</h3>
      {data && data.length === 0 ? <p className="small muted">{L("Ainda nenhum.", "None yet.")}</p> : (
        <div className="rows">{data?.map((m) => (
          <div key={m.month} className="row">
            <span className="grow"><span className="t" style={{ display: "block" }}>{monthLabel(m.month, lang).replace(/^./, (c) => c.toUpperCase())}</span>
              <span className="s">{L(`Receita ${eur(c2e(m.totals.revenue_cents), lang)} · resultado ${eur(c2e(m.totals.result_cents), lang)} · fechado por ${m.closed_by_name ?? "—"}`, `Revenue ${eur(c2e(m.totals.revenue_cents), lang)} · profit ${eur(c2e(m.totals.result_cents), lang)} · closed by ${m.closed_by_name ?? "—"}`)}</span></span>
            <button className="btn sm" onClick={() => reopen(m.month)}><LockOpen />{L("Reabrir…", "Reopen…")}</button>
          </div>
        ))}</div>
      )}
    </>
  );
}

interface ReportRow { id: string; month: string; lang: "pt" | "en"; text: string; model: string; file_id: string | null; created_at: string; created_by_name: string | null }
type Fig = [string, string, string];

/** Owner: the month's figures (worked out by the server) and the local AI's commentary on them, kept as PDFs. */
function Reports({ month, nav }: { month: string; nav: JSX.Element }) {
  const { L, lang, toast, confirm, openSheet } = useApp();
  const figs = useApi<{ rows: Fig[] }>(`/finance/reports/figures?month=${month}&lang=${lang}`);
  const kept = useApi<ReportRow[]>("/finance/reports");
  const ai = useAiStatus();
  const [draft, setDraft] = useState<AiState<{ text: string; model: string }>>({ state: "idle" });
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  useEffect(() => { setDraft({ state: "idle" }); }, [month, lang]);
  const write = async () => {
    setDraft({ state: "working" });
    try { const r = await api<{ text: string; model: string }>("POST", "/ai/report", { month, lang }); setText(r.text); setDraft({ state: "done", result: r }); }
    catch (e) { setDraft({ state: "error", message: isErr(e) ? e.message : String(e) }); }
  };
  const keep = async () => {
    if (draft.state !== "done") return;
    setSaving(true);
    try {
      await api("POST", "/finance/reports", { month, lang, text, model: draft.result.model });
      setDraft({ state: "idle" });
      toast(L(`Relatório de ${monthLabel(month, "pt")} guardado em PDF no arquivo`, `${monthLabel(month, "en")} report saved as PDF in the archive`));
    } catch (e) { toast(isErr(e) ? e.message : String(e)); } finally { setSaving(false); }
  };
  const remove = (r: ReportRow) => confirm({
    title: L(`Apagar o relatório de ${monthLabel(r.month, "pt")}?`, `Delete the ${monthLabel(r.month, "en")} report?`),
    text: L("O texto e o PDF no arquivo são apagados. Os números do mês não mudam.", "The text and its PDF in the archive are deleted. The month’s figures don’t change."),
    action: L("Apagar", "Delete"), destructive: true, hold: true,
    onConfirm: () => api("DELETE", `/finance/reports/${r.id}`).then(() => toast(L("Relatório apagado", "Report deleted"))).catch((e) => toast(isErr(e) ? e.message : String(e))),
  });
  const off = ai.data && !ai.data.available;
  return (
    <>
      <Head title={L("Relatórios IA", "AI Reports")} sub={L("Os números vêm do servidor; a IA local só escreve o comentário. Nada sai da clínica.", "The figures come from the server; the local AI only writes the commentary. Nothing leaves the clinic.")}>{nav}</Head>
      <div className="card pad" style={{ marginBottom: 18 }}>
        <div className="section-title"><h2>{L(`Números de ${monthLabel(month, "pt")}`, `${monthLabel(month, "en")} figures`)}</h2></div>
        {figs.error ? <p className="err">{figs.error}</p> : !figs.data ? <div className="skel" style={{ height: 120 }} /> : (
          <div className="tbl-wrap"><table>
            <thead><tr><th className="plain"></th><th className="plain r">{monthLabel(month, lang, true)}</th><th className="plain r">{monthLabel(shiftMonth(month, -1), lang, true)}</th></tr></thead>
            <tbody>{figs.data.rows.map(([k, a, b]) => <tr key={k}><td>{k}</td><td className="num r">{a}</td><td className="num r muted">{b}</td></tr>)}</tbody>
          </table></div>
        )}
        <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 14, flexWrap: "wrap" }}>
          <button className="btn ai" onClick={write} disabled={draft.state === "working" || !!off}><Sparkles />{L("Escrever relatório com IA", "Write Report with AI")}</button>
          {off && <span className="small muted">{aiOffText(ai.data!, L)}</span>}
        </div>
        {draft.state !== "idle" && (
          <div style={{ marginTop: 14 }}>
            <AiCard lines={4}
              title={draft.state === "error" ? L("A IA não conseguiu escrever o relatório", "The AI couldn’t write the report") : L("Comentário da IA local — corrija o que for preciso antes de guardar", "Commentary from the local AI — correct anything before keeping it")}
              working={draft.state === "working" ? L(`A escrever sobre a receita, os custos e a atividade de ${monthLabel(month, "pt")}… pode demorar alguns minutos.`, `Writing about ${monthLabel(month, "en")}’s revenue, costs and activity… this can take a few minutes.`) : undefined}
              error={draft.state === "error" ? draft.message : undefined}
              actions={<>
                {draft.state === "done" && <button className="btn sm pri" onClick={keep} disabled={saving || !text.trim()} aria-busy={saving}>{L("Guardar relatório", "Keep Report")}</button>}
                <button className="btn sm" onClick={write}>{L("Tentar de novo", "Retry")}</button>
                <button className="btn sm" onClick={() => setDraft({ state: "idle" })}>{L("Descartar", "Discard")}</button>
              </>}>
              {draft.state === "done" && <textarea className="input" style={{ minHeight: 200, lineHeight: 1.55 }} value={text} onChange={(e) => setText(e.target.value)} aria-label={L("Texto do relatório", "Report text")} />}
            </AiCard>
          </div>
        )}
      </div>
      <h2 style={{ marginBottom: 10 }}>{L("Relatórios guardados", "Kept reports")}</h2>
      {!kept.data ? null : kept.data.length === 0 ? <p className="small muted">{L("Ainda não guardou nenhum relatório. Escreva um acima e guarde-o: fica em PDF em Finanças › <ano> › Relatórios.", "No reports kept yet. Write one above and keep it: it’s saved as a PDF in Finance › <year> › Reports.")}</p> : (
        <div className="rows">
          {kept.data.map((r) => (
            <div key={r.id} className="row" style={{ alignItems: "flex-start" }}>
              <span className="grow" style={{ minWidth: 0 }}>
                <span style={{ display: "block", fontWeight: 600 }}>{monthLabel(r.month, lang)} <Badge tone="ai"><Sparkles size={10} style={{ verticalAlign: -1 }} /> {L("IA", "AI")}</Badge></span>
                <span className="s ell" style={{ display: "block" }}>{r.text.split("\n")[0]}</span>
                <span className="s">{L(`Guardado por ${r.created_by_name ?? "—"}, ${fmtDate(r.created_at, "pt")}`, `Kept by ${r.created_by_name ?? "—"}, ${fmtDate(r.created_at, "en")}`)}{r.model ? ` · ${r.model}` : ""}</span>
              </span>
              <span style={{ display: "flex", gap: 6 }}>
                {r.file_id && <button className="btn sm" onClick={() => openSheet(<FileViewer fileId={r.file_id!} title={L(`Relatório de ${monthLabel(r.month, "pt")}`, `${monthLabel(r.month, "en")} report`)} filename={`${L("Relatorio", "Report")} ${r.month}.pdf`} />)}><FileText />PDF</button>}
                <button className="btn sm" onClick={() => remove(r)}>{L("Apagar…", "Delete…")}</button>
              </span>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

function Export() {
  const { L, lang, toast } = useApp();
  const { data: closes } = useApi<MonthClose[]>("/finance/months");
  const prev = shiftMonth(thisMonth(), -1);
  const [range, setRange] = useState<"month" | "quarter" | "year" | "custom">("month");
  const [from, setFrom] = useState(prev);
  const [to, setTo] = useState(prev);
  const [files, setFiles] = useState(true);
  const [pdf, setPdf] = useState(true);
  const [busy, setBusy] = useState(false);
  const [last, setLast] = useState<{ filename: string; path: string; saved?: string; provisional: boolean } | null>(null);
  useEffect(() => {
    const [y, m] = prev.split("-").map(Number);
    if (range === "month") { setFrom(prev); setTo(prev); }
    if (range === "quarter") { const q0 = Math.floor((m - 1) / 3) * 3 + 1; setFrom(`${y}-${String(q0).padStart(2, "0")}`); setTo(`${y}-${String(q0 + 2).padStart(2, "0")}`); }
    if (range === "year") { setFrom(`${y}-01`); setTo(`${y}-12`); }
  }, [range]); // eslint-disable-line react-hooks/exhaustive-deps
  const months: string[] = []; for (let m = from; m <= to && months.length < 25; m = shiftMonth(m, 1)) months.push(m);
  const open = months.filter((m) => !closes?.some((c) => c.month === m) && m <= thisMonth());
  const label = from === to ? from : `${from}_a_${to}`;
  const run = async () => {
    setBusy(true);
    try {
      const r = await api<{ id: string; filename: string; path: string; provisional: boolean }>("POST", "/finance/export", { from, to, include_files: files, include_pdf: pdf });
      setLast({ filename: r.filename, path: r.path, provisional: r.provisional });
      const saved = await download(r.id, r.filename);
      setLast({ filename: r.filename, path: r.path, saved, provisional: r.provisional });
      toast(L("Zip guardado em Transferências", "Zip saved to Downloads"));
    } catch (e) { toast(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const tree: [number, string][] = [
    [0, `Exportacao_${label}`], [1, "01 Entradas"], [2, `entradas.csv  por-metodo.csv${files ? "  Faturas/" : ""}`], [1, "02 Saídas"], [2, `saidas.csv${files ? L("  pastas por categoria/", "  folders by category/") : ""}`],
    [1, "03 Salários"], [2, "salarios.csv"], [1, "04 Resumo"], [2, `resumo.csv${pdf ? "  resumo.pdf" : ""}`], [1, "LEIA-ME.txt"],
  ];
  return (
    <>
      <Head title={L("Exportar para contabilista", "Accountant Export")} sub={L("Um zip com tudo o que entrou e saiu, arrumado em pastas.", "One zip with everything in and out, sorted into folders.")} />
      <div className="grid2">
        <div className="card pad">
          <label className="field"><span>{L("Período", "Period")}</span>
            <select className="input" value={range} onChange={(e) => setRange(e.target.value as typeof range)}>
              <option value="month">{monthLabel(prev, lang)}</option>
              <option value="quarter">{L("Trimestre do mês passado", "Last month’s quarter")}</option>
              <option value="year">{L(`Ano de ${prev.slice(0, 4)}`, `Year ${prev.slice(0, 4)}`)}</option>
              <option value="custom">{L("Escolher meses…", "Choose Months…")}</option>
            </select>
          </label>
          {range === "custom" && (
            <div className="grid2">
              <Field2 label={L("De", "From")}><input className="input" type="month" value={from} max={thisMonth()} onChange={(e) => setFrom(e.target.value)} /></Field2>
              <Field2 label={L("Até", "To")}><input className="input" type="month" value={to} min={from} max={thisMonth()} onChange={(e) => setTo(e.target.value)} /></Field2>
            </div>
          )}
          <label className="check"><input type="checkbox" checked={files} onChange={(e) => setFiles(e.target.checked)} />{L("Incluir PDF das faturas e talões", "Include invoice PDFs and receipts")}</label>
          <label className="check"><input type="checkbox" checked={pdf} onChange={(e) => setPdf(e.target.checked)} />{L("Incluir resumo em PDF", "Include PDF summary")}</label>
          {open.length > 0 && <p className="small" style={{ marginTop: 6 }}><Badge tone="warn">{L("Provisório", "Provisional")}</Badge> <span className="muted">{L(`${open.map((m) => monthLabel(m, lang)).join(", ")} ainda não ${open.length > 1 ? "estão fechados" : "está fechado"}.`, `${open.map((m) => monthLabel(m, lang)).join(", ")} ${open.length > 1 ? "aren’t" : "isn’t"} closed yet.`)}</span></p>}
          <button className="btn pri" style={{ marginTop: 14 }} disabled={busy || to < from} aria-busy={busy} onClick={run}><FileArchive />{busy ? L("A criar o zip…", "Creating the zip…") : L("Criar zip", "Create Zip")}</button>
          {last && (
            <p className="small" style={{ marginTop: 12 }}>
              <Check size={13} style={{ verticalAlign: "-2px", color: "var(--ok)" }} /> {last.saved ? L(`Guardado em ${last.saved}.`, `Saved to ${last.saved}.`) : last.filename}
              <br /><span className="muted">{L(`Também fica no servidor em ${last.path}.`, `Also kept on the server in ${last.path}.`)}</span>
            </p>
          )}
        </div>
        <div className="card pad tree" aria-label={L("Conteúdo do zip", "Zip contents")}>
          {tree.map(([lvl, name], i) => (
            <div key={i} style={{ paddingLeft: lvl * 18 }}>{lvl < 2 && !name.endsWith(".txt") ? <Folder /> : <FileText />}<span className={lvl === 2 ? "muted" : ""}>{name}</span></div>
          ))}
        </div>
      </div>
    </>
  );
}
const Field2 = ({ label, children }: { label: string; children: React.ReactNode }) => <label className="field"><span>{label}</span>{children}</label>;
