// Working days in Portugal (same rules as server/src/workdays.rs): Monday to Friday, without national holidays.
import { addDays, fromYmd, ymd } from "./data";

function easter(y: number) {
  const a = y % 19, b = Math.floor(y / 100), c = y % 100, d = Math.floor(b / 4), e = b % 4, f = Math.floor((b + 8) / 25), g = Math.floor((b - f + 1) / 3);
  const h = (19 * a + b - d - g + 15) % 30, i = Math.floor(c / 4), k = c % 4, l = (32 + 2 * e + 2 * i - h - k) % 7, m = Math.floor((a + 11 * h + 22 * l) / 451);
  return new Date(y, Math.floor((h + l - 7 * m + 114) / 31) - 1, ((h + l - 7 * m + 114) % 31) + 1);
}
/** National public holidays in Portugal (Carnival and the municipal holiday are optional, so they count as working days). */
function isHoliday(d: Date) {
  const md = `${d.getMonth() + 1}-${d.getDate()}`;
  if (["1-1", "4-25", "5-1", "6-10", "8-15", "10-5", "11-1", "12-1", "12-8", "12-25"].includes(md)) return true;
  const e = easter(d.getFullYear()), s = ymd(d);
  return [addDays(e, -2), e, addDays(e, 60)].some((x) => ymd(x) === s);
}
const isWorking = (d: Date) => d.getDay() !== 0 && d.getDay() !== 6 && !isHoliday(d);
export function addWorkingDays(from: string, n: number) {
  let d = fromYmd(from);
  while (n > 0) { d = addDays(d, 1); if (isWorking(d)) n--; }
  return ymd(d);
}
/** Working days after `from` up to `to`; negative when `to` has passed. */
export function workingDaysBetween(from: string, to: string) {
  const [a, b, sign] = to >= from ? [from, to, 1] : [to, from, -1];
  let d = fromYmd(a), n = 0;
  while (ymd(d) < b) { d = addDays(d, 1); if (isWorking(d)) n++; }
  return n * sign;
}
