"use client";

import { OPEN_SPONSOR } from "@/lib/pending";
import { SPONSOR_CONTACT, SPONSOR_PAYMENTS } from "@/lib/site";

type Props = { className: string; label: string; title?: string; children: React.ReactNode };

export default function SponsorButton({ className, label, title, children }: Props) {
  if (!SPONSOR_PAYMENTS) {
    return (
      <a className={className} href={SPONSOR_CONTACT} target="_blank" rel="noopener" aria-label={label} title={title}>
        {children}
      </a>
    );
  }
  return (
    <button type="button" className={className} aria-label={label} title={title} onClick={() => window.dispatchEvent(new Event(OPEN_SPONSOR))}>
      {children}
    </button>
  );
}
