/* eslint-disable @next/next/no-img-element */
"use client";

import { useSyncExternalStore } from "react";
import { parsePending, readPending, subscribePending } from "@/lib/pending";

const TIP = "We're reviewing your slot. Your product will show here soon.";

export default function PendingSlot({ variant }: { variant: "grid" | "nav" }) {
  const raw = useSyncExternalStore(subscribePending, readPending, () => null);
  const p = parsePending(raw);
  if (!p?.paid) return null;
  const mark = p.logo ? <img src={p.logo} alt="" /> : <span className="pending-letter">{p.name.slice(0, 1).toUpperCase()}</span>;
  if (variant === "nav") {
    return (
      <li>
        <span className="nav-sponsor is-pending" tabIndex={0} aria-label={`${p.name}: ${TIP}`} data-tip={TIP}>
          <span className="pending-blur">{mark}</span>
        </span>
      </li>
    );
  }
  return (
    <li>
      <span className="icon-slot is-pending" tabIndex={0} aria-label={`${p.name}: ${TIP}`} data-tip={TIP}>
        <span className="icon-box">
          <span className="pending-blur">{mark}</span>
        </span>
        <span className="icon-name">{p.name}</span>
      </span>
    </li>
  );
}
