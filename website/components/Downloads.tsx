"use client";

import { useEffect, useState } from "react";
import { formatCount, totalDownloads } from "@/lib/npm";

export default function Downloads({ initial }: { initial: number | null }) {
  const [count, setCount] = useState(initial);

  useEffect(() => {
    let live = true;
    totalDownloads().then((n) => {
      if (live && n !== null) setCount((c) => Math.max(c ?? 0, n));
    });
    return () => {
      live = false;
    };
  }, []);

  if (count === null) return null;
  return (
    <>
      <span aria-hidden="true">&middot;</span>
      <span className="downloads">{formatCount(count)} downloads</span>
    </>
  );
}
