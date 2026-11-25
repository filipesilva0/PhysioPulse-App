import { createContext, useContext } from "react";
import type { ReactNode } from "react";

export type Lang = "pt" | "en";
export type Role = "owner" | "therapist" | "reception";
export type Page = "today" | "schedule" | "patients" | "finance" | "team" | "settings";

export type CtxItem = { label: string; icon?: ReactNode; fn: () => void; danger?: boolean; disabled?: boolean } | "-";

export interface ConfirmOpts {
  title: string;
  text?: string;
  action: string;
  destructive?: boolean;
  /** A delete: the button has to be held for 5 seconds (implies destructive). */
  hold?: boolean;
  onConfirm: () => void;
}

export interface AppCtx {
  lang: Lang;
  setLang: (l: Lang) => void;
  /** Pick the string for the current language: L("Hoje", "Today"). */
  L: (pt: string, en: string) => string;
  role: Role;
  setRole: (r: Role) => void;
  /** The signed-in person. */
  me: { id: string; name: string; email: string; role: Role; ai_enabled?: boolean };
  /** After the person changes their own name (Settings › Account). */
  setMe: (m: { id: string; name: string; email: string; role: Role; ai_enabled?: boolean }) => void;
  /** Minutes without use before the app locks (set on the server). */
  idleMinutes: number;
  page: Page;
  go: (p: Page) => void;
  /** Short note at the top; pass undo (and what it undoes) to offer Undo, also in Edit › Undo. */
  toast: (msg?: string, undo?: () => void, undoLabel?: string) => void;
  /** Context menu at the pointer (right-click) or under a button. */
  menuAt: (x: number, y: number, items: CtxItem[]) => void;
  openSheet: (node: ReactNode) => void;
  /** Closes the sheet straight away (after its action succeeded). */
  closeSheet: () => void;
  /** Cancel or Esc: asks "Discard changes?" first if something was typed in the sheet. */
  cancelSheet: () => void;
  /** Called by Sheet when something inside it changes. */
  sheetEdited: () => void;
  confirm: (o: ConfirmOpts) => void;
  patientId: string;
  openPatient: (id: string) => void;
  /** Name of the patient open on the Patients page (for File-menu commands). */
  patientName: string;
  setPatientName: (n: string) => void;
}

export const Ctx = createContext<AppCtx>(null as unknown as AppCtx);
export const useApp = () => useContext(Ctx);

export const eur = (n: number, lang: Lang) =>
  new Intl.NumberFormat(lang === "pt" ? "pt-PT" : "en-IE", {
    style: "currency", currency: "EUR",
    // Always two decimals, so amounts line up in every list and table (audit phase C: "472 €" next to "936,40 €").
    minimumFractionDigits: 2, maximumFractionDigits: 2,
  }).format(n);
