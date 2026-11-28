"use client";

import { useEffect } from "react";

export default function TocSpy() {
  useEffect(() => {
    const links = Array.from(document.querySelectorAll<HTMLAnchorElement>(".docs-nav a[href^='#']"));
    const targets = links
      .map((a) => document.getElementById(decodeURIComponent(a.hash.slice(1))))
      .filter((el): el is HTMLElement => el !== null);
    if (targets.length === 0) return;

    let frame = 0;
    const update = () => {
      frame = 0;
      const line = 120;
      let current = targets[0];
      for (const t of targets) {
        if (t.getBoundingClientRect().top <= line) current = t;
        else break;
      }
      for (const a of links) {
        if (a.hash !== `#${current.id}`) a.removeAttribute("aria-current");
        else if (!a.hasAttribute("aria-current")) {
          a.setAttribute("aria-current", "location");
          a.scrollIntoView({ block: "nearest" });
        }
      }
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      window.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);
  return null;
}
