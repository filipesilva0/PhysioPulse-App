// Account on the iPhone (phase 5; reference: grouped profile list). Who you are, this iPhone's session (until when;
// Face ID on every reopen), language, and Sign Out. Everything else about the account is changed on the computer.
import { useEffect, useState } from "react";
import { Check } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, getServer } from "../lib/api";
import { useApi, fmtDate } from "../lib/data";
import { version as APP_VERSION } from "../../package.json";
import { Avatar, Group, Row, Screen, SheetHead } from "./ui";

export interface PhoneSession { id: string; name: string; code: string; signed_in: string; last_seen: string; expires_at: string; this_device: boolean }

const ROLE: Record<string, [string, string]> = { owner: ["Proprietário", "Owner"], therapist: ["Fisioterapeuta", "Therapist"], reception: ["Receção", "Reception"] };

export function AccountScreen({ signOut }: { signOut: () => void }) {
  const { L, lang, me, openSheet } = useApp();
  const { data: phones } = useApi<PhoneSession[]>(me.id ? `/staff/${me.id}/phones` : null);
  const mine = phones?.find((p) => p.this_device);
  const [server, setServer] = useState<string | null>(null);
  const [srv, setSrv] = useState<string>("—");
  useEffect(() => {
    getServer().then(setServer).catch(() => undefined);
    api<{ version?: string }>("GET", "/status").then((r) => setSrv(r.version ?? "—")).catch(() => undefined);
  }, []);
  const host = server?.replace(/^https?:\/\//, "") ?? "—";
  return (
    <Screen title={L("Conta", "Account")}>
      <div className="p-profile">
        <Avatar name={me.name} size={56} />
        <span className="p-pinfo"><b>{me.name}</b><span>{me.email}</span><span className="p-role">{L(...(ROLE[me.role] ?? ["", ""]))}</span></span>
      </div>

      <Group title={L("Este iPhone", "This iPhone")}>
        <Row label={L("Nome", "Name")} value={mine?.name ?? "iPhone"} />
        <Row label={L("Bloqueio", "Lock")} value="Face ID" />
        <Row label={L("Sessão válida até", "Signed in until")} value={mine ? fmtDate(mine.expires_at, lang, { day: "numeric", month: "short", year: "numeric" }) : "—"} />
        <Row label={L("Servidor", "Server")} value={<span className="p-select p-ell">{host}</span>} />
      </Group>

      <Group title={L("Preferências", "Preferences")}>
        <Row label={L("Idioma", "Language")} value={lang === "pt" ? "Português" : "English"} chevron onClick={() => openSheet(<LanguageSheet />)} />
      </Group>

      <Group>
        <Row label={L("Terminar sessão…", "Sign Out…")} tone="bad" onClick={signOut} />
      </Group>

      <p className="p-foot">PhysioPulse {APP_VERSION} · {L("servidor", "server")} {srv}</p>
    </Screen>
  );
}

function LanguageSheet() {
  const { L, lang, setLang, closeSheet } = useApp();
  const pick = (l: "pt" | "en") => { setLang(l); closeSheet(); };
  return (
    <>
      <SheetHead title={L("Idioma", "Language")} onClose={closeSheet} />
      <div className="p-sheet-body">
        <Group>
          <Row label="Português" selected={lang === "pt"} value={lang === "pt" ? <Check className="p-check" /> : undefined} onClick={() => pick("pt")} />
          <Row label="English" selected={lang === "en"} value={lang === "en" ? <Check className="p-check" /> : undefined} onClick={() => pick("en")} />
        </Group>
      </div>
    </>
  );
}
