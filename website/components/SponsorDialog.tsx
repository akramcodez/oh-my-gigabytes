"use client";

import { useEffect, useRef, useState } from "react";
import { OPEN_SPONSOR, writePending } from "@/lib/pending";
import { SPONSOR_PRICE, sponsorCheckout } from "@/lib/site";

const MAX_LOGO = 200 * 1024;

function readLogo(file: File | null): Promise<string | undefined> {
  if (!file || file.size === 0 || file.size > MAX_LOGO || !file.type.startsWith("image/")) return Promise.resolve(undefined);
  return new Promise((resolve) => {
    const r = new FileReader();
    r.onload = () => resolve(typeof r.result === "string" ? r.result : undefined);
    r.onerror = () => resolve(undefined);
    r.readAsDataURL(file);
  });
}

export default function SponsorDialog() {
  const ref = useRef<HTMLDialogElement>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const open = () => ref.current?.showModal();
    window.addEventListener(OPEN_SPONSOR, open);
    return () => window.removeEventListener(OPEN_SPONSOR, open);
  }, []);

  async function submit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const field = (k: string) => String(data.get(k) ?? "").trim();
    const form = { name: field("name"), link: field("link"), description: field("description"), email: field("email") };
    let url: URL;
    try {
      url = new URL(/^https?:\/\//i.test(form.link) ? form.link : `https://${form.link}`);
    } catch {
      return setError("Enter a valid link, like https://example.com");
    }
    if (url.protocol !== "https:" && url.protocol !== "http:") return setError("The link must start with https://");
    form.link = url.toString();
    setBusy(true);
    const logo = await readLogo(data.get("logo") as File | null);
    writePending({ ...form, logo, paid: false });
    window.location.href = sponsorCheckout(form);
  }

  return (
    <dialog ref={ref} className="sponsor-dialog" aria-labelledby="sponsor-dialog-title" onClose={() => setError("")}>
      <form onSubmit={submit}>
        <h2 id="sponsor-dialog-title">
          Sponsor <em>OMG</em>
        </h2>
        <p className="dialog-lead">Tell us what to show. You pay {SPONSOR_PRICE} once on the next page.</p>
        <label>
          Product name
          <input name="name" required maxLength={40} autoComplete="organization" />
        </label>
        <label>
          Link
          <input name="link" required maxLength={200} inputMode="url" placeholder="https://" />
        </label>
        <label>
          Description
          <textarea name="description" required maxLength={140} rows={2} placeholder="One line about your product" />
        </label>
        <label>
          Email
          <input name="email" type="email" required maxLength={120} autoComplete="email" />
        </label>
        <label>
          Logo <span className="optional">(optional, only for your preview)</span>
          <input name="logo" type="file" accept="image/png,image/svg+xml,image/jpeg,image/webp" />
        </label>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="dialog-cancel" onClick={() => ref.current?.close()}>
            Cancel
          </button>
          <button type="submit" className="dialog-pay" disabled={busy}>
            {busy ? "Opening checkout..." : `Continue to payment`}
          </button>
        </div>
      </form>
    </dialog>
  );
}
