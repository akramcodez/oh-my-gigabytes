"use client";

import { useEffect } from "react";
import { toggleTheme } from "./ThemeToggle";

export default function KeyNav({ targets }: { targets: string[] }) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey || event.isComposing) return;
      const el = event.target as HTMLElement | null;
      if (el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName))) return;
      if (event.key === "t") {
        toggleTheme();
        return;
      }
      const index = Number(event.key) - 1;
      const id = targets[index];
      if (!Number.isInteger(index) || !id) return;
      const section = document.getElementById(id);
      if (!section) return;
      event.preventDefault();
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      section.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
      section.focus({ preventScroll: true });
      history.replaceState(null, "", `#${id}`);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [targets]);
  return null;
}
