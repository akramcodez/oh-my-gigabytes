export async function starCount(): Promise<number | null> {
  try {
    const res = await fetch("https://api.github.com/repos/akramcodez/oh-my-gigabytes", {
      headers: { accept: "application/vnd.github+json" },
    });
    if (!res.ok) return null;
    const count = (await res.json()).stargazers_count;
    return typeof count === "number" ? count : null;
  } catch {
    return null;
  }
}
