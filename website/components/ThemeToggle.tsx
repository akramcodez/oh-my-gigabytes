"use client";

import { LuMoon, LuSun } from "react-icons/lu";

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
      <span className="when-light">
        <LuMoon aria-hidden="true" />
        Dark theme
      </span>
      <span className="when-dark">
        <LuSun aria-hidden="true" />
        Light theme
      </span>
    </button>
  );
}
