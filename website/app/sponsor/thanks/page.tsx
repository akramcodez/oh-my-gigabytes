import type { Metadata } from "next";
import Link from "next/link";
import { Suspense } from "react";
import PaymentId from "@/components/PaymentId";
import { REPO, X_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Thanks for sponsoring",
  robots: { index: false, follow: false },
};

export default function SponsorThanks() {
  return (
    <main className="thanks">
      <p className="kicker">Sponsors</p>
      <h1>
        Thank you for <em>sponsoring OMG.</em>
      </h1>
      <Suspense fallback={null}>
        <PaymentId />
      </Suspense>
      <p>To put your slot live, send these:</p>
      <ol>
        <li>Your logo (SVG, or a square PNG)</li>
        <li>Your brand name and a one-line description</li>
        <li>The link it should open</li>
        <li>Your payment ID</li>
      </ol>
      <p>
        Send them by DM on <a href={X_URL}>X</a> or in a{" "}
        <a href={`${REPO}/issues/new?title=${encodeURIComponent("Sponsor slot")}`}>GitHub issue</a>. Your slot and the
        shoutout on X follow shortly after.
      </p>
      <p>
        <Link href="/#sponsors">Back to the site</Link>
      </p>
    </main>
  );
}
