import type { Metadata } from "next";
import Link from "next/link";
import { Suspense } from "react";
import PaymentId from "@/components/PaymentId";
import { SPONSOR_EMAIL, X_URL } from "@/lib/site";

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
      <p>
        We got your product details with the payment and are reviewing your slot. It shows blurred on the site for you
        until it goes live, and the shoutout on X follows.
      </p>
      <p>
        Send your logo (SVG, or a square PNG) with your payment ID by <a href={`mailto:${SPONSOR_EMAIL}`}>email</a> or
        DM on <a href={X_URL}>X</a>.
      </p>
      <p>
        <Link href="/#sponsors">Back to the site</Link>
      </p>
    </main>
  );
}
