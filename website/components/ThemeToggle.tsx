"use client";

import { LuMoon, LuSun } from "react-icons/lu";

export function toggleTheme() {
  const root = document.documentElement;
  const current = root.dataset.theme ?? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  const next = current === "light" ? "dark" : "light";
  root.dataset.theme = next;
  try {
    localStorage.setItem("omg-theme", next);
  } catch {}
}

export default function ThemeToggle() {
  return (
    <button type="button" className="theme-toggle" onClick={toggleTheme} aria-label="Switch between dark and light theme" title="Switch theme (t)">
      <LuSun className="logo-dark" aria-hidden="true" />
      <LuMoon className="logo-light" aria-hidden="true" />
    </button>
  );
}
