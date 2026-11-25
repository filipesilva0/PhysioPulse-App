// Checks shared by forms (same rules as the server).

/** Portuguese NIF/NIPC: 9 digits and a mod-11 check digit (server: routes/clinic.rs `nif_ok`). */
export function nifOk(nif: string) {
  if (!/^\d{9}$/.test(nif) || nif[0] === "0" || nif[0] === "4") return false;
  const d = nif.split("").map(Number);
  const sum = d.slice(0, 8).reduce((a, x, i) => a + x * (9 - i), 0);
  const c = 11 - (sum % 11);
  return (c >= 10 ? 0 : c) === d[8];
}
export const emailOk = (e: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e.trim());
/** At least 9 digits (Portuguese numbers), spaces and a leading + allowed. */
export const phoneOk = (p: string) => /^\+?[\d\s()-]+$/.test(p.trim()) && p.replace(/\D/g, "").length >= 9;

/** A patient's tax number as the server accepts it (server: routes/patients.rs `tidy_nif`): a Portuguese NIF with a valid
 *  check digit (spaces, dots and a "PT" prefix ignored), or a foreign number starting with its two-letter country code. */
export function patientNifOk(raw: string) {
  let n = raw.replace(/[\s.-]/g, "").toUpperCase();
  if (!n) return true;
  if (n.startsWith("PT") && n.length === 11) n = n.slice(2);
  if (/^[A-Z]{2}[A-Z0-9]{1,18}$/.test(n)) return true;
  return nifOk(n);
}
