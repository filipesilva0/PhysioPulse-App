// Shared pieces for the local-AI features (owner, and therapists with AI on): whether the AI computer answers, and the labelled
// card every AI result appears in (specific progress text, then Use / Retry / Discard — DESIGN.md › Feedback).
import type { ReactNode } from "react";
import { Sparkles } from "lucide-react";
import { useApp } from "../lib/ctx";
import { useApi } from "../lib/data";

export interface AiStatus { available: boolean; model: string; url?: string; reason?: "offline" | "no_model" | "not_local" | "error"; models?: string[];
  /** The speech-to-text program for session recordings (whisper.cpp on the AI PC). */
  speech?: { available: boolean; url: string; reason?: "not_set" | "not_local" | "offline" };
  /** The assistant's model: its own (Settings › Local AI, 13 Nov 2026) or the main one. */
  assistant?: { model: string; available: boolean; reason?: "offline" | "no_model" | "not_local" | "error"; own: boolean } }

/** Is the local AI reachable? Asked only for people with AI on (the owner, and therapists switched on in Team). */
export function useAiStatus() {
  const { role, me } = useApp();
  return useApi<AiStatus>(role === "owner" || me.ai_enabled ? "/ai/status" : null);
}

/** Why the AI can't be used right now, in words that say what to do. */
export function aiOffText(s: AiStatus, L: (pt: string, en: string) => string) {
  const where = s.url ? ` (${s.url.replace(/^http:\/\//, "")})` : "";
  return s.reason === "no_model"
    ? L(`O modelo ${s.model} não está instalado no computador da IA. Escolha outro em Definições › IA local.`, `The model ${s.model} isn’t installed on the AI computer. Choose another in Settings › Local AI.`)
    : L(`Sem resposta da IA local${where}. Verifique se o computador está ligado, ou o endereço em Definições › IA local.`, `No answer from the local AI${where}. Check the computer is on, or the address in Settings › Local AI.`);
}

export type AiState<T> = { state: "idle" } | { state: "working" } | { state: "error"; message: string } | { state: "done"; result: T };

/** The violet, labelled card an AI result lives in. `working` = the specific progress sentence. */
export function AiCard({ title, working, error, children, actions, lines = 3 }: {
  title: string; working?: string; error?: string; children?: ReactNode; actions?: ReactNode; lines?: number;
}) {
  return (
    <div className="ai-card" aria-live="polite" aria-busy={!!working}>
      <div className="ai-head"><Sparkles size={14} aria-hidden />{working ?? title}</div>
      {working && Array.from({ length: lines }, (_, i) => <div key={i} className="skel" style={{ width: `${[95, 86, 62, 78][i % 4]}%` }} />)}
      {!working && error && <p className="small" role="alert">{error}</p>}
      {!working && children}
      {!working && actions && <div className="ai-actions">{actions}</div>}
    </div>
  );
}
