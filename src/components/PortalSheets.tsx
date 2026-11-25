// Phase 4b (23 Nov 2026): the patient area on the website, from the patient's file.
// Invite codes (everyone), password-reset codes and taking access away (reception, the patient's therapist or the
// owner; the server decides). Codes are shown once; the message goes from the staff member's own WhatsApp or SMS,
// like reminders (no automatic email).
import { useEffect, useState } from "react";
import { Copy, KeyRound, MessageCircle, Send, UserPlus, X } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr, openMessage } from "../lib/api";
import { fmtDate, useApi } from "../lib/data";
import { Badge, Seg, Sheet } from "./ui";
import { intlPhone } from "./Reminders";
import { isMac } from "../lib/platform";

export interface PortalInfo {
  accounts: { id: string; email: string; relation: "self" | "guardian"; two_step: boolean; last_login_at: string | null; created_at: string }[];
  invites: { id: string; relation: "self" | "guardian"; created_at: string; expires_at: string; created_by_name: string | null }[];
  open: boolean; minor: boolean; can_manage: boolean; site_url: string; clinic: string;
  patient: { name: string; phone: string | null; email: string | null; lang: "pt" | "en" };
}

/** A code just made: shown once, with ready-made messages. */
function CodeView({ info, code, kind, expires }: { info: PortalInfo; code: string; kind: "invite" | "reset"; expires: string }) {
  const { L, lang, toast } = useApp();
  const pl = info.patient.lang ?? "pt";
  const T = (pt: string, en: string) => (pl === "en" ? en : pt);
  const first = info.patient.name.split(" ")[0] ?? "";
  const base = info.site_url || "";
  const link = base ? `${base}${pl === "en" ? (kind === "invite" ? "/en/account/invite" : "/en/account/reset") : kind === "invite" ? "/conta/convite" : "/conta/repor"}?${pl === "en" ? "code" : "codigo"}=${code}` : "";
  const clinic = info.clinic || "PhysioPulse";
  const text = kind === "invite"
    ? T(`Olá ${first}. O seu código para a área de utente da ${clinic} é ${code} (válido 14 dias).${link ? ` Ative aqui: ${link}` : ""}`,
        `Hello ${first}. Your code for the ${clinic} patient area is ${code} (valid for 14 days).${link ? ` Set it up here: ${link}` : ""}`)
    : T(`Olá ${first}. O código para escolher uma nova palavra-passe na área de utente da ${clinic} é ${code} (válido 24 horas).${link ? ` ${link}` : ""}`,
        `Hello ${first}. The code to choose a new password in the ${clinic} patient area is ${code} (valid for 24 hours).${link ? ` ${link}` : ""}`);
  const phone = intlPhone(info.patient.phone);
  const send = (ch: "whatsapp" | "sms") => {
    if (!phone) return;
    const body = encodeURIComponent(text);
    const url = ch === "sms" ? `sms:+${phone}${isMac ? "&" : "?"}body=${body}` : `https://wa.me/${phone}?text=${body}`;
    openMessage(url).catch((e) => toast(isErr(e) ? e.message : String(e)));
  };
  return (
    <div className="portal-code">
      <p className="small muted">{kind === "invite" ? L("Código de convite (só é mostrado agora)", "Invite code (shown only now)") : L("Código para nova palavra-passe (só é mostrado agora)", "New-password code (shown only now)")}</p>
      <p className="code num" aria-live="polite">{code}</p>
      <p className="small muted">{L(`Válido até ${fmtDate(expires, lang, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })}.`, `Valid until ${fmtDate(expires, lang, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })}.`)}</p>
      <div className="portal-msg small">{text}</div>
      <div className="btnrow">
        <button className="btn" onClick={() => { navigator.clipboard?.writeText(text).then(() => toast(L("Mensagem copiada", "Message copied"))); }}><Copy />{L("Copiar mensagem", "Copy Message")}</button>
        {phone && <button className="btn" onClick={() => send("whatsapp")}><MessageCircle />WhatsApp</button>}
        {phone && <button className="btn" onClick={() => send("sms")}><Send />SMS</button>}
      </div>
      {!info.site_url && <p className="small muted">{L("Falta o endereço do site (SITE_URL) no servidor, por isso a mensagem não leva o link.", "The website address (SITE_URL) isn't set on the server, so the message has no link.")}</p>}
    </div>
  );
}

export function PortalAccess({ pid }: { pid: string }) {
  const { L, lang, toast, confirm } = useApp();
  const q = useApi<PortalInfo>(`/patients/${pid}/portal`);
  const info = q.data;
  const [relation, setRelation] = useState<"self" | "guardian">("self");
  const [made, setMade] = useState<{ code: string; kind: "invite" | "reset"; expires: string } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (info?.minor) setRelation("guardian"); }, [info?.minor]);
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try { await fn(); } catch (e) { toast(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const invite = () => run(async () => {
    const r = await api<{ code: string; expires_at: string }>("POST", `/patients/${pid}/portal/invite`, { relation });
    setMade({ code: r.code, kind: "invite", expires: r.expires_at }); q.reload();
  });
  const reset = (account: string) => run(async () => {
    const r = await api<{ code: string; expires_at: string }>("POST", `/patients/${pid}/portal/reset`, { account_id: account });
    setMade({ code: r.code, kind: "reset", expires: r.expires_at });
  });
  const unlink = (a: PortalInfo["accounts"][number]) => confirm({
    title: L(`Retirar o acesso de ${a.email}?`, `Remove access for ${a.email}?`),
    text: L("Essa conta deixa de ver este utente no site. Se não tiver mais ninguém, a conta é apagada.", "That account stops seeing this patient on the website. If it has nobody else, the account is deleted."),
    action: L("Retirar Acesso", "Remove Access"), destructive: true,
    onConfirm: () => run(async () => { await api("DELETE", `/patients/${pid}/portal/accounts/${a.id}`); toast(L("Acesso retirado", "Access removed")); q.reload(); }),
  });
  const dropInvite = (id: string) => run(async () => { await api("DELETE", `/patients/${pid}/portal/invites/${id}`); q.reload(); });
  const when = (iso: string | null) => iso ? fmtDate(iso, lang, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : L("nunca", "never");
  return (
    <Sheet title={L("Área de utente", "Patient Area")} sub={L("O que este utente vê no site: as sessões, os pedidos e as faturas. Nunca notas nem testes.", "What this patient sees on the website: sessions, requests and invoices. Never notes or tests.")}>
      {!info ? <div className="skel" style={{ height: 120 }} /> : (
        <>
          {!info.open && <p className="notice info" role="note">{L("A área de utente ainda está fechada no site (Definições › Site e portal). Pode criar convites para experimentar quando a abrir.", "The patient area is still closed on the website (Settings › Website and Portal). You can make invites to try it once it's open.")}</p>}
          {made && <CodeView info={info} code={made.code} kind={made.kind} expires={made.expires} />}
          <p className="list-head">{L("Contas com acesso", "Accounts with access")}</p>
          {info.accounts.length === 0 ? <p className="small muted" style={{ margin: "0 0 14px" }}>{L("Ninguém ainda.", "Nobody yet.")}</p> : (
            <div className="rows" style={{ marginBottom: 14 }}>
              {info.accounts.map((a) => (
                <div key={a.id} className="row">
                  <span className="grow"><span className="t" style={{ display: "block" }}>{a.email}</span>
                    <span className="s" style={{ display: "block" }}>{a.relation === "guardian" ? L("Encarregado de educação", "Parent or guardian") : L("O próprio", "The patient")} · {L(`Última entrada: ${when(a.last_login_at)}`, `Last sign-in: ${when(a.last_login_at)}`)}{a.two_step ? L(" · dois passos", " · two-step") : ""}</span>
                    {info.can_manage && <span className="btnrow" style={{ marginTop: 8 }}>
                      <button className="btn sm" disabled={busy} onClick={() => reset(a.id)}><KeyRound />{L("Código para nova palavra-passe", "New-Password Code")}</button>
                      <button className="btn sm" disabled={busy} onClick={() => unlink(a)}><X />{L("Retirar acesso…", "Remove Access…")}</button>
                    </span>}</span>
                </div>
              ))}
            </div>
          )}
          {info.invites.length > 0 && <>
            <p className="list-head">{L("Convites por usar", "Unused invites")}</p>
            <div className="rows" style={{ marginBottom: 14 }}>
              {info.invites.map((c) => (
                <div key={c.id} className="row">
                  <span className="grow"><span className="t" style={{ display: "block" }}>{c.relation === "guardian" ? L("Para o encarregado", "For the guardian") : L("Para o próprio", "For the patient")}</span>
                    <span className="s" style={{ display: "block" }}>{L(`Criado por ${c.created_by_name ?? "—"} · válido até ${fmtDate(c.expires_at, lang, { day: "numeric", month: "short" })}`, `Made by ${c.created_by_name ?? "—"} · valid until ${fmtDate(c.expires_at, lang, { day: "numeric", month: "short" })}`)}</span></span>
                  <button className="btn sm" disabled={busy} onClick={() => dropInvite(c.id)}>{L("Retirar", "Withdraw")}</button>
                </div>
              ))}
            </div>
          </>}
          <p className="list-head">{L("Novo convite", "New invite")}</p>
          <div className="btnrow" style={{ alignItems: "center" }}>
            {info.minor ? <span className="small muted">{L("Menor de 16 anos: o convite é para o encarregado de educação.", "Under 16: the invite is for a parent or guardian.")}</span>
              : <Seg label={L("Para quem", "For whom")} value={relation} onChange={setRelation} options={[["self", L("O próprio", "The Patient")], ["guardian", L("Encarregado", "Guardian")]]} />}
            <button className="btn pri" disabled={busy} onClick={invite}><UserPlus />{L("Criar código de convite", "Make Invite Code")}</button>
          </div>
          {info.accounts.some((a) => a.two_step) && <p className="small muted" style={{ marginTop: 10 }}><Badge>{L("Nota", "Note")}</Badge> {L("O código para nova palavra-passe também desliga os códigos de dois passos dessa conta.", "A new-password code also turns that account's two-step codes off.")}</p>}
        </>
      )}
    </Sheet>
  );
}
