import { CalendarClock, Check, FileText, PenLine, Receipt, X, UserX, Users, RotateCcw } from "lucide-react";
import type { MouseEvent } from "react";
import type { AppCtx, CtxItem } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import type { Appointment } from "../lib/data";
import { AddAttendee, CancelAppointment, EditAppointment, RecordPayment } from "./Sheets";

/** Set the status of one appointment, with Undo. */
export function setStatus(c: AppCtx, a: Appointment, status: Appointment["status"]) {
  const { L, toast } = c;
  const label = status === "done" ? L("Marcar como realizada", "Mark as Done") : status === "no_show" ? L("Marcar falta", "Mark as No-Show") : L("Repor marcação", "Restore Appointment");
  api("PATCH", `/appointments/${a.id}`, { scope: "one", status })
    .then(() => toast(status === "done" ? L("Marcada como realizada", "Marked as done") : status === "no_show" ? L("Marcada como falta", "Marked as no-show") : L("Marcação reposta", "Appointment restored"),
      () => { api("PATCH", `/appointments/${a.id}`, { scope: "one", status: a.status, allow_overlap: true }).catch((e) => toast(isErr(e) ? e.message : String(e))); }, label))
    .catch((e) => toast(isErr(e) ? e.message : String(e)));
}

/** Same actions for an appointment wherever it appears: right-click, or the ⋮ button. */
export function apptItems(c: AppCtx, a: Appointment): CtxItem[] {
  const { L, role, openPatient, openSheet } = c;
  const items: CtxItem[] = [];
  if (a.status === "cancelled") return [{ label: L("Repor marcação", "Restore Appointment"), icon: <RotateCcw />, fn: () => setStatus(c, a, "booked") }];
  if (a.kind !== "block") {
    if (a.patient_id) {
      items.push({ label: L("Abrir ficha", "Open File"), icon: <FileText />, fn: () => openPatient(a.patient_id!) });
      if (role !== "reception") items.push({ label: L("Nota de evolução…", "Progress Note…"), icon: <PenLine />, fn: () => openPatient(a.patient_id!) });
      items.push({ label: L("Registar pagamento…", "Record Payment…"), icon: <Receipt />, fn: () => openSheet(<RecordPayment patientId={a.patient_id!} appointmentId={a.id} />) });
    } else if (a.kind === "class") {
      items.push({ label: L("Inscrever utente…", "Add Attendee…"), icon: <Users />, fn: () => openSheet(<AddAttendee appt={a} />) });
    }
    items.push("-");
    if (a.status === "booked") {
      items.push({ label: L("Marcar como realizada", "Mark as Done"), icon: <Check />, fn: () => setStatus(c, a, "done") });
      if (a.patient_id) items.push({ label: L("Marcar falta", "Mark as No-Show"), icon: <UserX />, fn: () => setStatus(c, a, "no_show") });
    } else {
      items.push({ label: L("Voltar a marcada", "Mark as Booked"), icon: <RotateCcw />, fn: () => setStatus(c, a, "booked") });
    }
    items.push("-");
  }
  items.push({ label: L("Remarcar…", "Reschedule…"), icon: <CalendarClock />, fn: () => openSheet(<EditAppointment appt={a} />) });
  items.push({ label: a.kind === "block" ? L("Remover bloqueio…", "Remove Blocked Time…") : L("Cancelar marcação…", "Cancel Appointment…"), icon: <X />, danger: true, fn: () => openSheet(<CancelAppointment appt={a} />) });
  return items;
}

/** Open a context menu from a right-click or from a button (placed under the button). */
export function openMenu(c: AppCtx, e: MouseEvent, items: CtxItem[]) {
  e.preventDefault();
  e.stopPropagation();
  if (e.type === "contextmenu") c.menuAt(e.clientX, e.clientY, items);
  else { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); c.menuAt(r.right - 220, r.bottom + 4, items); }
}
