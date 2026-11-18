"use client";

export function toggleTheme() {
  const root = document.documentElement;
  const next = root.dataset.theme === "dark" ? "light" : "dark";
  root.dataset.theme = next;
  try {
    localStorage.setItem("omg-site-theme", next);
  } catch {}
}

export default function ThemeToggle() {
  return (
    <button type="button" className="theme-toggle" onClick={toggleTheme} aria-label="Switch between dark and light theme" title="Switch theme (t)">
      <span className="when-light">Dark theme</span>
      <span className="when-dark">Light theme</span>
    </button>
  );
}
