import { SPONSORS, type SponsorForm } from "@/lib/site";

export type Pending = SponsorForm & { logo?: string; paid: boolean };

const KEY = "omg-sponsor-pending";
const CHANGE = "omg:pending";
export const OPEN_SPONSOR = "omg:sponsor";

export function readPending(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function parsePending(raw: string | null): Pending | null {
  if (!raw) return null;
  try {
    const p = JSON.parse(raw) as Pending;
    if (typeof p.name !== "string" || typeof p.link !== "string") return null;
    if (SPONSORS.some((s) => s.url === p.link)) return null;
    return p;
  } catch {
    return null;
  }
}

export function writePending(p: Pending) {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    try {
      localStorage.setItem(KEY, JSON.stringify({ ...p, logo: undefined }));
    } catch {}
  }
  window.dispatchEvent(new Event(CHANGE));
}

export function subscribePending(cb: () => void) {
  window.addEventListener("storage", cb);
  window.addEventListener(CHANGE, cb);
  return () => {
    window.removeEventListener("storage", cb);
    window.removeEventListener(CHANGE, cb);
  };
}
