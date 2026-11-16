const FIRST_PUBLISH = "2026-11-08";
const DAY = 86_400_000;
const WINDOW = 540;

const iso = (t: number) => new Date(t).toISOString().slice(0, 10);

export async function totalDownloads(): Promise<number | null> {
  try {
    const end = Date.now();
    let from = Date.parse(FIRST_PUBLISH);
    let total = 0;
    while (from <= end) {
      const to = Math.min(end, from + (WINDOW - 1) * DAY);
      const res = await fetch(`https://api.npmjs.org/downloads/point/${iso(from)}:${iso(to)}/oh-my-gigabytes`);
      if (!res.ok) return null;
      const count = (await res.json()).downloads;
      if (typeof count !== "number") return null;
      total += count;
      from = to + DAY;
    }
    return total;
  } catch {
    return null;
  }
}

export const formatCount = (n: number) =>
  n >= 1_000_000
    ? `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`
    : n >= 1000
      ? `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1).replace(/\.0$/, "")}k`
      : String(n);
