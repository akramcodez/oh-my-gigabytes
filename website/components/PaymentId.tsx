"use client";

import { useSearchParams } from "next/navigation";

export default function PaymentId() {
  const params = useSearchParams();
  const id = params.get("payment_id");
  const status = params.get("status");
  if (status === "failed" || status === "cancelled") {
    return <p>The payment did not go through ({status}). Nothing was charged; you can try again from the site.</p>;
  }
  return id ? (
    <>
      <p>
        Payment ID: <code>{id}</code>
      </p>
      {status === "processing" && <p>Your payment is still processing. Dodo Payments emails your receipt once it clears.</p>}
    </>
  ) : null;
}
