import { useEffect, useState } from "react";
import { Check, ChevronLeft, ChevronRight, Copy, KeyRound, LogOut, Minus, MoreHorizontal, ShieldCheck, Smartphone, UserCog, UserMinus, UserPlus, UserCheck, X } from "lucide-react";
import { useApp, eur } from "../lib/ctx";
import { api, isErr, type Role, type StaffRow } from "../lib/api";
import { useApi } from "../lib/data";
import { c2e } from "../lib/money";
import { Layout } from "../components/Layout";
import { Avatar, Badge, Empty, Field, Head, Sheet, Switch } from "../components/ui";
import { openMenu } from "../components/menus";
import { FIN_SECTION } from "./Finance";

const colorFor = (role: Role) => (role === "owner" ? "var(--t1)" : role === "therapist" ? "var(--t2)" : "var(--t3)");
interface Invite { id: string; role: Role; email: string | null; created_at: string; expires_at: string; created_by_name: string | null }
interface PayRow { staff_id: string; model: string; fixed_cents: number }

export default function Team() {
  const app = useApp();
  const { L, lang, role, me, toast, openSheet, confirm, go } = app;
  const owner = role === "owner";
  const [rows, setRows] = useState<StaffRow[] | null>(null);
  const [err, setErr] = useState("");
  const [sel, setSel] = useState<string | null>(null);
  const roleName = (r: string) => r === "owner" ? L("Proprietário", "Owner") : r === "therapist" ? L("Fisioterapeuta", "Therapist") : L("Receção", "Reception");

  const load = () => {
    if (!owner) { setRows(null); return; }
    api<StaffRow[]>("GET", "/staff").then((r) => { setRows(r); setErr(""); })
      .catch((e) => setErr(isErr(e) ? e.message : String(e)));
  };
  useEffect(load, [owner]); // eslint-disable-line react-hooks/exhaustive-deps
  const inv = useApi<Invite[]>(owner ? "/invites" : null);
  const pay = useApi<{ staff: PayRow[] }>(owner ? "/finance/salaries" : null);
  const reload = () => { load(); inv.reload(); };

  const s = rows?.find((r) => r.id === sel);
  const lastSeen = (iso: string | null) => {
    if (!iso) return L("Nunca", "Never");
    const d = new Date(iso);
    return d.toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  };

  const actions = (r: StaffRow) => r.id === me.id || r.email === me.email ? [] : [
    { label: L("Mudar função…", "Change Role…"), icon: <UserCog />, disabled: !r.active, fn: () => openSheet(<RoleSheet person={r} onDone={load} />) },
    { label: L("Repor palavra-passe…", "Reset Password…"), icon: <KeyRound />, disabled: !r.active, fn: () => openSheet(<ResetCodeSheet person={r} />) },
    { label: L("Repor verificação em dois passos…", "Reset Two-Step Verification…"), icon: <Smartphone />, disabled: !r.active || !r.totp_enabled, fn: () => confirm({
        title: L(`Repor os dois passos de ${r.name}?`, `Reset two-step for ${r.name}?`),
        text: L("Use quando a pessoa perdeu o telemóvel. É desligada de todos os computadores e, ao entrar, configura a app de autenticação de novo.", "Use it when they lost their phone. They’re signed out of every computer and set up the authenticator app again when they next sign in."),
        action: L("Repor", "Reset"), destructive: true,
        onConfirm: () => api("POST", `/staff/${r.id}/reset-two-step`).then(() => { load(); toast(L("Dois passos repostos", "Two-step reset")); }).catch((e) => toast(isErr(e) ? e.message : String(e))) }) },
    "-" as const,
    { label: L("Terminar sessão em todo o lado", "Sign Out Everywhere"), icon: <LogOut />, fn: () =>
      api<{ ended: number }>("DELETE", `/staff/${r.id}/sessions`).then((x) => toast(L(`${x.ended} sessão(ões) terminada(s)`, `${x.ended} session(s) ended`))).catch((e) => toast(isErr(e) ? e.message : String(e))) },
    "-" as const,
    r.active
      ? { label: L("Desativar conta…", "Deactivate Account…"), icon: <UserMinus />, danger: true, fn: () => confirm({
          title: L(`Desativar ${r.name}?`, `Deactivate ${r.name}?`),
          text: L("Deixa de poder entrar e é desligada de todos os computadores. Pode reativar depois.", "They can’t sign in any more and are signed out of every computer. You can reactivate them later."),
          action: L("Desativar", "Deactivate"), destructive: true,
          onConfirm: () => api("PATCH", `/staff/${r.id}`, { active: false }).then(() => { load(); toast(L("Conta desativada", "Account deactivated"), () => api("PATCH", `/staff/${r.id}`, { active: true }).then(load), L("desativar conta", "Deactivate Account")); }).catch((e) => toast(isErr(e) ? e.message : String(e))),
        }) }
      : { label: L("Reativar conta", "Reactivate Account"), icon: <UserCheck />, fn: () => api("PATCH", `/staff/${r.id}`, { active: true }).then(() => { load(); toast(L("Conta reativada", "Account reactivated")); }) },
  ];

  // "sw": only when the owner turns the person's AI switch on (PhysioPulse plan, phase 2).
  const matrix: [string, boolean | "sw", boolean | "sw", boolean | "sw"][] = [
    [L("Agenda", "Schedule"), true, true, true],
    [L("Contactos e planos dos utentes", "Patient contacts and plans"), true, true, true],
    [L("Informação clínica", "Clinical information"), true, true, false],
    [L("Registar pagamentos", "Record payments"), true, true, true],
    [L("Finanças", "Finance"), true, false, false],
    [L("IA local: gravar sessões, rascunhos de notas e cartas", "Local AI: session recording, note and letter drafts"), true, "sw", false],
    [L("Assistente: marcações e agenda", "Assistant: appointments and schedule"), true, "sw", "sw"],
    [L("IA local nas finanças; assistente para preços, horário e equipa", "Local AI for finance; assistant for prices, hours and staff"), true, false, false],
    [L("Equipa, convites e registo de acessos", "Staff, invites and access log"), true, false, false],
  ];

  const person = owner ? s : { id: me.id, email: me.email, name: me.name, role, active: true, totp_enabled: true, ai_enabled: me.ai_enabled, created_at: "", last_seen: null };
  /** Owner: one AI switch per person, for every AI feature their role can reach (owner, 11 Nov 2026; reception: the assistant's schedule jobs). */
  const setAi = (r: StaffRow, on: boolean) => api("PATCH", `/staff/${r.id}`, { ai_enabled: on })
    .then(() => { load(); toast(on ? L(`IA local ligada para ${r.name}`, `Local AI on for ${r.name}`) : L(`IA local desligada para ${r.name}`, `Local AI off for ${r.name}`), () => api("PATCH", `/staff/${r.id}`, { ai_enabled: !on }).then(load), on ? L("ligar IA", "Turn AI On") : L("desligar IA", "Turn AI Off")); })
    .catch((e) => toast(isErr(e) ? e.message : String(e)));
  const dm = (iso: string) => new Date(iso).toLocaleDateString(lang === "pt" ? "pt-PT" : "en-GB", { day: "numeric", month: "short" });
  const payOf = pay.data?.staff.find((x) => x.staff_id === person?.id);

  // Owner: the staff list; choosing someone opens their details (‹ Equipa goes back).
  if (owner && !person) {
    return (
      <Layout label={L("Equipa", "Team")}>
        <div className="wrap">
          <Head title={L("Equipa", "Team")} sub={rows ? L(`${rows.length} pessoa${rows.length === 1 ? "" : "s"}`, `${rows.length} ${rows.length === 1 ? "person" : "people"}`) : undefined}>
            <button className="btn pri" onClick={() => openSheet(<InviteSheet onDone={reload} />)}><UserPlus />{L("Convidar profissional…", "Invite Staff Member…")}</button>
          </Head>
          {err ? <Empty icon={<ShieldCheck />} title={L("Não foi possível carregar a equipa", "Couldn’t load the team")} text={err} action={<button className="btn" onClick={load}>{L("Tentar de novo", "Try Again")}</button>} />
            : !rows ? <div className="skel" style={{ height: 120 }} /> : (
              <>
                <div className="rows">
                  {rows.map((x) => (
                    <button key={x.id} className="row" onClick={() => setSel(x.id)} onContextMenu={(e) => { const a = actions(x); if (a.length) openMenu(app, e, a); }}>
                      <Avatar name={x.name} color={colorFor(x.role)} />
                      <span className="grow"><span className="t" style={{ display: "block" }}>{x.name}</span><span className="s" style={{ display: "block" }}>{roleName(x.role)} · {x.email}</span></span>
                      {x.active && x.role !== "owner" && x.ai_enabled && <span title={L("Pode usar o assistente e a IA local", "Can use the assistant and the local AI")}><Badge tone="ai">{L("Assistente", "Assistant")}</Badge></span>}
                      {!x.active ? <Badge tone="bad">{L("Desativada", "Deactivated")}</Badge> : !x.totp_enabled ? <Badge tone="warn">{L("Dois passos por configurar", "Two-step not set up")}</Badge> : null}
                      <span className="s muted" style={{ width: 200, textAlign: "right" }}>{x.last_seen ? L(`Ativo a ${lastSeen(x.last_seen)}`, `Active ${lastSeen(x.last_seen)}`) : L("Nunca entrou", "Never signed in")}</span>
                      <ChevronRight size={14} className="chev" />
                    </button>
                  ))}
                </div>
                {inv.data && inv.data.length > 0 && <>
                  <h2 style={{ margin: "26px 0 10px" }}>{L("Convites por usar", "Unused invites")}</h2>
                  <div className="rows">
                    {inv.data.map((i) => (
                      <div key={i.id} className="row">
                        <Avatar name="?" color={colorFor(i.role)} />
                        <span className="grow"><span className="t" style={{ display: "block" }}>{i.email ?? L("Qualquer email", "Any email")}</span>
                          <span className="s" style={{ display: "block" }}>{roleName(i.role)} · {L(`expira ${dm(i.expires_at)}`, `expires ${dm(i.expires_at)}`)}</span></span>
                        <button className="btn sm" onClick={() => confirm({ title: L("Retirar este convite?", "Withdraw this invite?"), text: L("O código deixa de funcionar.", "The code stops working."), action: L("Retirar", "Withdraw"), destructive: true,
                          onConfirm: () => api("DELETE", `/invites/${i.id}`).then(() => { inv.reload(); toast(L("Convite retirado", "Invite withdrawn")); }).catch((e) => toast(isErr(e) ? e.message : String(e))) })}><X />{L("Retirar…", "Withdraw…")}</button>
                      </div>
                    ))}
                  </div>
                </>}
            <h2 style={{ margin: "26px 0 10px" }}>{L("Permissões por função", "Permissions by role")}</h2>
            <div className="tbl-wrap"><table>
              <thead><tr><th className="plain">{L("Área", "Area")}</th>{["owner", "therapist", "reception"].map((r) => <th key={r} className="plain">{roleName(r)}</th>)}</tr></thead>
              <tbody>{matrix.map(([a, ...v]) => (
                <tr key={a}><td>{a}</td>{v.map((ok, i) => <td key={i} style={{ color: ok === "sw" ? "var(--text2)" : ok ? "var(--ok)" : "var(--text3)" }}>{ok === "sw" ? <span className="small">{L("Se ligada", "If switched on")}</span> : ok ? <Check aria-label={L("Sim", "Yes")} /> : <Minus aria-label={L("Não", "No")} />}</td>)}</tr>
              ))}</tbody>
            </table></div>
            <p className="small muted" style={{ marginTop: 10 }}>{L("Estas regras são verificadas no servidor, não só escondidas na aplicação.", "These rules are checked on the server, not just hidden in the app.")}</p>
              </>
            )}
        </div>
      </Layout>
    );
  }

  return (
    <Layout label={L("Equipa", "Team")}>
      <div className="wrap">
        {owner && <div className="crumb"><button className="backlink" onClick={() => setSel(null)}><ChevronLeft size={14} />{L("Equipa", "Team")}</button></div>}
        {!person ? null
          : <>
            <Head title={<span style={{ display: "flex", gap: 12, alignItems: "center" }}><Avatar name={person.name} color={colorFor(person.role as Role)} size="md" />{person.name}</span>}
              sub={<span style={{ marginLeft: 52, display: "block" }}>{roleName(person.role)} · {person.email}</span>}>
              {owner && actions(person as StaffRow).length > 0 && (
                <button className="btn icon round" aria-haspopup="menu" aria-label={L("Mais ações", "More actions")} title={L("Mais ações", "More actions")} onClick={(e) => openMenu(app, e, actions(person as StaffRow))}><MoreHorizontal /></button>
              )}
            </Head>
            <div className="group">
              <div className="setrow"><div className="grow">{L("Estado", "Status")}</div>{person.active ? <Badge tone="ok">{L("Ativa", "Active")}</Badge> : <Badge tone="bad">{L("Desativada", "Deactivated")}</Badge>}</div>
              <div className="setrow"><div className="grow">{L("Verificação em dois passos", "Two-step verification")}</div>{person.totp_enabled ? <Badge tone="ok"><Check />{L("Ligada", "On")}</Badge> : <Badge tone="warn">{L("Por configurar", "Not set up")}</Badge>}</div>
              <div className="setrow">
                <div className="grow">{L("IA local", "Local AI")}
                  <div className="d">{person.role === "owner" ? L("Sempre ligada para o proprietário.", "Always on for the owner.")
                    : person.role === "reception" ? L("O assistente para marcar, mudar e cancelar marcações e tratar dos pedidos. Nada clínico.", "The assistant, to book, move and cancel appointments and handle requests. Nothing clinical.")
                      : L("O assistente para a agenda, gravar sessões e rascunhos de notas e cartas com a IA da clínica. As finanças ficam só para o proprietário.", "The assistant for the schedule, session recording, and note and letter drafts with the clinic’s AI. Finance stays the owner’s.")}</div></div>
                {owner && person.role !== "owner" && person.id !== me.id
                  ? <Switch on={!!person.ai_enabled} onChange={(v) => setAi(person as StaffRow, v)} label={L(`IA local para ${person.name}`, `Local AI for ${person.name}`)} />
                  : (person.role === "owner" || person.ai_enabled) ? <Badge tone="ok"><Check />{L("Ligada", "On")}</Badge> : <Badge tone="n">{L("Desligada", "Off")}</Badge>}
              </div>
              {owner && <div className="setrow"><div className="grow">{L("Última atividade", "Last active")}</div><span className="muted">{lastSeen(person.last_seen)}</span></div>}
              {owner && (
                <div className="setrow">
                  <div className="grow">{L("Remuneração", "Pay")}<div className="d">{!payOf ? "…" : payOf.fixed_cents ? L(`Fixo: ${eur(c2e(payOf.fixed_cents), "pt")} por mês`, `Fixed: ${eur(c2e(payOf.fixed_cents), "en")} a month`) : L("Ainda sem valor", "Not set yet")}</div></div>
                  <button className="btn sm" onClick={() => { go("finance"); setTimeout(() => window.dispatchEvent(new CustomEvent(FIN_SECTION, { detail: "salaries" })), 0); }}>{L("Alterar em Salários…", "Change in Salaries…")}</button>
                </div>
              )}
            </div>
            <Phones person={person as StaffRow} />
            {!owner && <>
            <h2 style={{ margin: "26px 0 10px" }}>{L("Permissões por função", "Permissions by role")}</h2>
            <div className="tbl-wrap"><table>
              <thead><tr><th className="plain">{L("Área", "Area")}</th>{["owner", "therapist", "reception"].map((r) => <th key={r} className="plain">{roleName(r)}</th>)}</tr></thead>
              <tbody>{matrix.map(([a, ...v]) => (
                <tr key={a}><td>{a}</td>{v.map((ok, i) => <td key={i} style={{ color: ok === "sw" ? "var(--text2)" : ok ? "var(--ok)" : "var(--text3)" }}>{ok === "sw" ? <span className="small">{L("Se ligada", "If switched on")}</span> : ok ? <Check aria-label={L("Sim", "Yes")} /> : <Minus aria-label={L("Não", "No")} />}</td>)}</tr>
              ))}</tbody>
            </table></div>
            <p className="small muted" style={{ marginTop: 10 }}>{L("Estas regras são verificadas no servidor, não só escondidas na aplicação.", "These rules are checked on the server, not just hidden in the app.")}</p>
            </>}
          </>}
      </div>
    </Layout>
  );
}

interface PhoneSession { id: string; name: string; code: string; signed_in: string; last_seen: string; expires_at: string; this_device: boolean }

/** The person's signed-in iPhones (PhysioPulse plan, phase 5). A phone stays signed in for 30 days with no idle lock
 *  (Face ID on every reopen), so the owner can end it here, e.g. when a phone is lost. The phone itself stays
 *  approved: the person signs in again with password and code. Removing the phone altogether is Settings › Devices. */
function Phones({ person }: { person: StaffRow }) {
  const { L, lang, role, toast, confirm } = useApp();
  const { data } = useApi<PhoneSession[]>(`/staff/${person.id}/phones`);
  if (!data?.length) return null;
  const d = (iso: string) => new Date(iso).toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  return (
    <>
      <h2 style={{ margin: "26px 0 10px" }}>iPhone</h2>
      <div className="group">
        {data.map((p) => (
          <div key={p.id} className="setrow">
            <Smartphone style={{ color: "var(--text2)" }} aria-hidden />
            <div className="grow">{p.name} <span className="muted small num">({p.code})</span>
              <div className="d">{L(`Entrou a ${d(p.signed_in)} · usado a ${d(p.last_seen)} · sessão até ${new Date(p.expires_at).toLocaleDateString("pt-PT", { day: "numeric", month: "short" })}`,
                `Signed in ${d(p.signed_in)} · last used ${d(p.last_seen)} · signed in until ${new Date(p.expires_at).toLocaleDateString("en-GB", { day: "numeric", month: "short" })}`)}</div></div>
            {role === "owner" && <button className="btn sm" onClick={() => confirm({
              title: L(`Terminar a sessão de ${person.name} neste iPhone?`, `Sign ${person.name} out on this iPhone?`),
              text: L("A aplicação do iPhone deixa de abrir as fichas de imediato. Para voltar a entrar é preciso a palavra-passe e o código de 6 dígitos. Para remover o iPhone de vez, use Definições › Dispositivos.",
                "The iPhone app stops showing patient files straight away. Signing back in needs the password and the 6-digit code. To remove the iPhone altogether, use Settings › Devices."),
              action: L("Terminar sessão", "Sign Out"), destructive: true,
              onConfirm: () => api("DELETE", `/staff/${person.id}/phones/${p.id}`).then(() => toast(L("Sessão do iPhone terminada", "iPhone signed out"))).catch((e) => toast(isErr(e) ? e.message : String(e))),
            })}>{L("Terminar sessão no iPhone…", "Sign Out This iPhone…")}</button>}
          </div>
        ))}
      </div>
    </>
  );
}

/** Owner creates a one-time invite code. It's shown once: only its hash is kept on the server. */
function InviteSheet({ onDone }: { onDone: () => void }) {
  const { L, toast, closeSheet, cancelSheet } = useApp();
  const [inviteRole, setInviteRole] = useState<Role>("therapist");
  const [email, setEmail] = useState("");
  const [code, setCode] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const create = async () => {
    setBusy(true); setErr("");
    try { const r = await api<{ code: string }>("POST", "/invites", { role: inviteRole, email: email.trim() || null }); setCode(r.code); onDone(); }
    catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  if (code) {
    return (
      <Sheet title={L("Código de convite", "Invite Code")} sub={L("Dê este código à pessoa. Só é mostrado uma vez e expira em 7 dias.", "Give this code to the person. It’s shown only once and expires in 7 days.")}>
        <div className="card pad" style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, marginBottom: 12 }}>
          <span className="num" style={{ fontSize: "var(--fs-title2)", fontWeight: 700, letterSpacing: ".04em" }}>{code}</span>
          <button className="btn" onClick={() => { navigator.clipboard?.writeText(code); toast(L("Código copiado", "Code copied")); }}><Copy />{L("Copiar", "Copy")}</button>
        </div>
        <p className="small muted">{L("Na aplicação, a pessoa escolhe “Criar conta” e escreve este código.", "In the app, they choose “Create one” and enter this code.")}</p>
      </Sheet>
    );
  }
  return (
    <div className="sheet" role="dialog" aria-modal="true" aria-label={L("Convidar profissional", "Invite Staff Member")}>
      <div className="sheet-body">
        <h2>{L("Convidar profissional", "Invite Staff Member")}</h2>
        <p className="sub small" style={{ marginBottom: 14 }}>{L("Cria um código para a pessoa criar a sua conta.", "Creates a code they use to create their account.")}</p>
        <Field label={L("Função", "Role")}>
          <div className="radio-row">
            {(["therapist", "reception", "owner"] as Role[]).map((r) => (
              <label key={r} className="check"><input type="radio" name="role" checked={inviteRole === r} onChange={() => setInviteRole(r)} />{r === "owner" ? L("Proprietário", "Owner") : r === "therapist" ? L("Fisioterapeuta", "Therapist") : L("Receção", "Reception")}</label>
            ))}
          </div>
        </Field>
        <Field label={L("Email (opcional)", "Email (optional)")}><input className="input" type="email" placeholder="ana@clinica.pt" value={email} onChange={(e) => setEmail(e.target.value)} /></Field>
        <p className="small muted">{L("Com email, o código só serve para esse endereço.", "With an email, the code only works for that address.")}</p>
        <div className="err" role="alert">{err}</div>
      </div>
      <div className="sheet-foot">
        <button className="btn" onClick={cancelSheet}>{L("Cancelar", "Cancel")}</button>
        <button className="btn pri" disabled={busy} onClick={create}>{busy ? L("Um momento…", "One moment…") : L("Criar convite", "Create Invite")}</button>
      </div>
    </div>
  );
}

/** Owner: change someone's role (checked on the server; takes effect on their next request). */
function RoleSheet({ person, onDone }: { person: StaffRow; onDone: () => void }) {
  const { L, toast } = useApp();
  const [r, setR] = useState<Role>(person.role);
  const name = (x: Role) => x === "owner" ? L("Proprietário", "Owner") : x === "therapist" ? L("Fisioterapeuta", "Therapist") : L("Receção", "Reception");
  return (
    <Sheet title={L(`Função de ${person.name}`, `${person.name}’s role`)} sub={L("Muda o que a pessoa vê e pode fazer, a partir do próximo clique.", "Changes what they see and can do, from their next click.")}
      action={L("Mudar função", "Change Role")} disabled={r === person.role}
      onAction={async () => { await api("PATCH", `/staff/${person.id}`, { role: r }); onDone(); toast(L(`${person.name}: ${name(r)}`, `${person.name}: ${name(r)}`)); }}>
      <div className="radio-row" role="radiogroup" aria-label={L("Função", "Role")}>
        {(["therapist", "reception", "owner"] as Role[]).map((x) => <label key={x} className="check"><input type="radio" name="role" checked={r === x} onChange={() => setR(x)} />{name(x)}</label>)}
      </div>
      {r === "owner" && r !== person.role && <p className="small" style={{ marginTop: 10, color: "var(--warn)" }}>{L("Um proprietário vê tudo: finanças, IA, equipa e registo de acessos.", "An owner sees everything: finance, AI, staff and the access log.")}</p>}
      {r === "reception" && person.role !== "reception" && <p className="small muted" style={{ marginTop: 10 }}>{L("A receção não vê informação clínica.", "Reception doesn’t see clinical information.")}</p>}
    </Sheet>
  );
}

/** Owner: a one-time code someone uses to choose a new password ("Forgot your password?" on the sign-in screen). */
function ResetCodeSheet({ person }: { person: StaffRow }) {
  const { L, toast } = useApp();
  const [code, setCode] = useState<string | null>(null);
  if (code) return (
    <Sheet title={L("Código para nova palavra-passe", "Password Reset Code")} sub={L(`Dê este código a ${person.name}. Só é mostrado agora e expira em 24 horas.`, `Give this code to ${person.name}. It’s shown only now and expires in 24 hours.`)}>
      <div className="card pad" style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, marginBottom: 12 }}>
        <span className="num" style={{ fontSize: "var(--fs-title2)", fontWeight: 700, letterSpacing: ".04em" }}>{code}</span>
        <button className="btn" onClick={() => { navigator.clipboard?.writeText(code); toast(L("Código copiado", "Code copied")); }}><Copy />{L("Copiar", "Copy")}</button>
      </div>
      <p className="small muted">{L(`No ecrã de entrada, a pessoa escolhe “Esqueceu-se da palavra-passe?”, escreve ${person.email}, este código e a nova palavra-passe. Depois entra com o código de 6 dígitos, como sempre.`, `On the sign-in screen, they choose “Forgot Your Password?”, enter ${person.email}, this code and a new password. Then they sign in with the 6-digit code as usual.`)}</p>
    </Sheet>
  );
  return (
    <Sheet title={L(`Repor a palavra-passe de ${person.name}?`, `Reset ${person.name}’s password?`)} sub={L("Cria um código de uso único (24 horas). A palavra-passe atual continua a funcionar até ser usado.", "Makes a one-time code (24 hours). Their current password keeps working until it’s used.")}
      action={L("Criar código", "Make Code")} onAction={async () => { const r = await api<{ code: string }>("POST", `/staff/${person.id}/reset-code`); setCode(r.code); return false; }}>
      <p className="small muted">{L("Um código mais recente substitui o anterior.", "A newer code replaces the older one.")}</p>
    </Sheet>
  );
}
