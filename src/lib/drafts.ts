// Typed clinical work that hasn't been saved yet, kept in memory per patient so it survives switching
// tabs and pages (audit C2, 10 Nov 2026). Never written to disk. Cleared when someone else signs in
// on this computer and when the app closes; a lock by the same person keeps it.
import type { Block } from "./data";

export interface NoteDraft {
  tpl: string; blocks: Block[]; text: string; aiUsed: boolean;
  /** The server draft this is saved to (autosave creates it). */
  draftId: string | null;
  /** JSON of what was last saved, to know if there's anything new. */
  saved: string;
  savedAt: number | null;
  saving?: Promise<string>;
  /** The recording whose AI draft this note uses (PhysioPulse plan, phase 2): signing the note deletes it. */
  recordingId?: string | null;
}
export type AnamnesisForm = Record<string, unknown>;

export const noteDrafts = new Map<string, NoteDraft>();
export const anamnesisDrafts = new Map<string, AnamnesisForm>();
let who = "";

/** Call when someone signs in: another person's unsaved work is dropped. */
export function draftsFor(staffId: string) {
  if (who !== staffId) { noteDrafts.clear(); anamnesisDrafts.clear(); who = staffId; }
}
