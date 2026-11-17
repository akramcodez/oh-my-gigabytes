import { createHmac, timingSafeEqual } from "node:crypto";
import { SPONSOR_EMAIL } from "@/lib/site";

export const runtime = "nodejs";

const MAX_BODY = 64 * 1024;
const TOLERANCE = 5 * 60;

function verify(secret: string, id: string, timestamp: string, signatures: string, body: string) {
  const ts = Number(timestamp);
  if (!Number.isFinite(ts) || Math.abs(Date.now() / 1000 - ts) > TOLERANCE) return false;
  const key = Buffer.from(secret.replace(/^whsec_/, ""), "base64");
  const expected = createHmac("sha256", key).update(`${id}.${timestamp}.${body}`).digest();
  return signatures.split(" ").some((part) => {
    const [version, sig] = part.split(",");
    if (version !== "v1" || !sig) return false;
    const got = Buffer.from(sig, "base64");
    return got.length === expected.length && timingSafeEqual(got, expected);
  });
}

const escape = (s: unknown) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

type Payment = {
  payment_id?: string;
  total_amount?: number;
  currency?: string;
  customer?: { email?: string; name?: string };
  metadata?: Record<string, string>;
};

export async function POST(req: Request) {
  const secret = process.env.DODO_WEBHOOK_SECRET;
  const resendKey = process.env.RESEND_API_KEY;
  if (!secret || !resendKey) return new Response("not configured", { status: 503 });

  const body = await req.text();
  if (body.length > MAX_BODY) return new Response("too large", { status: 413 });
  const h = req.headers;
  if (!verify(secret, h.get("webhook-id") ?? "", h.get("webhook-timestamp") ?? "", h.get("webhook-signature") ?? "", body)) {
    return new Response("bad signature", { status: 401 });
  }

  let event: { type?: string; data?: Payment };
  try {
    event = JSON.parse(body);
  } catch {
    return new Response("bad json", { status: 400 });
  }
  if (event.type !== "payment.succeeded") return new Response("ignored");

  const p = event.data ?? {};
  const m = p.metadata ?? {};
  const amount = typeof p.total_amount === "number" ? `${(p.total_amount / 100).toFixed(2)} ${p.currency ?? ""}`.trim() : "unknown";
  const rows: [string, unknown][] = [
    ["Product name", m.sponsor_name],
    ["Link", m.sponsor_link],
    ["Description", m.sponsor_description],
    ["Contact email", m.sponsor_email ?? p.customer?.email],
    ["Payer", `${p.customer?.name ?? ""} ${p.customer?.email ?? ""}`.trim()],
    ["Amount", amount],
    ["Payment ID", p.payment_id],
  ];
  const html =
    `<p>Payment confirmed: a new OMG sponsor slot was paid.</p><table cellpadding="6">` +
    rows.map(([k, v]) => `<tr><td><b>${k}</b></td><td>${escape(v || "not given")}</td></tr>`).join("") +
    `</table><p>Ask them for a logo, add them to SPONSORS in website/lib/site.ts and post the shoutout on X.</p>`;

  const sent = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      authorization: `Bearer ${resendKey}`,
      "content-type": "application/json",
      "idempotency-key": `dodo-${h.get("webhook-id")}`,
    },
    body: JSON.stringify({
      from: process.env.SPONSOR_MAIL_FROM ?? "OMG Sponsors <onboarding@resend.dev>",
      to: [process.env.SPONSOR_MAIL_TO ?? SPONSOR_EMAIL],
      reply_to: m.sponsor_email ?? p.customer?.email,
      subject: `New sponsor paid: ${String(m.sponsor_name ?? "unknown").slice(0, 60)}`,
      html,
    }),
  });
  if (!sent.ok) return new Response("mail failed", { status: 502 });
  return new Response("ok");
}
