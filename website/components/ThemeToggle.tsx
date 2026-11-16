"use client";

import { LuSunMoon } from "react-icons/lu";

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
    <button type="button" className="theme-toggle" onClick={toggleTheme} aria-label="Switch between dark and light theme">
      <LuSunMoon className="theme-icon" aria-hidden="true" />
      <span className="nav-label">Theme</span> <kbd aria-hidden="true">t</kbd>
    </button>
  );
}
