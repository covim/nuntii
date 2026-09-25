import { format, isSameYear, isToday, isYesterday } from "date-fns";
import { de } from "date-fns/locale";
import type { EmailAddress } from "./api";

export function formatListDate(ts: number): string {
  const d = new Date(ts * 1000);
  if (isToday(d)) return format(d, "HH:mm");
  if (isYesterday(d)) return "Gestern";
  if (isSameYear(d, new Date())) return format(d, "d. MMM", { locale: de });
  return format(d, "dd.MM.yy");
}

export function formatFullDate(ts: number): string {
  return format(new Date(ts * 1000), "EEEE, d. MMMM yyyy, HH:mm", { locale: de });
}

export function addressName(a: EmailAddress | undefined): string {
  if (!a) return "(unbekannt)";
  return a.name?.trim() || a.address;
}

export function addressList(list: EmailAddress[]): string {
  return list.map(addressName).join(", ");
}

/** "Name <addr>" form for compose fields. */
export function addressFull(a: EmailAddress): string {
  return a.name ? `${a.name} <${a.address}>` : a.address;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function initials(a: EmailAddress | undefined): string {
  const name = addressName(a);
  const parts = name.replace(/[<>"]/g, "").split(/[\s@.]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? "?") + (parts[1]?.[0] ?? "")).toUpperCase();
}

/** Splits a free-text recipient field ("a@b.de, Name <c@d.de>; e@f.de") into entries. */
export function splitRecipients(value: string): string[] {
  const out: string[] = [];
  let current = "";
  let inQuotes = false;
  let inAngle = false;
  for (const ch of value) {
    if (ch === '"') inQuotes = !inQuotes;
    if (ch === "<") inAngle = true;
    if (ch === ">") inAngle = false;
    if ((ch === "," || ch === ";") && !inQuotes && !inAngle) {
      if (current.trim()) out.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim()) out.push(current.trim());
  return out;
}

export function normalizeSubject(s: string): string {
  return s.replace(/^\s*((re|aw|fw|fwd|wg|antw)\s*:\s*)+/i, "").trim();
}
