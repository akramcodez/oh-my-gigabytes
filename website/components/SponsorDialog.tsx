/* eslint-disable @next/next/no-img-element */
"use client";

import { useEffect, useRef, useState } from "react";
import { LuArrowRight, LuImagePlus, LuLock, LuX } from "react-icons/lu";
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
  const [logo, setLogo] = useState<{ src?: string; name: string } | null>(null);

  useEffect(() => {
    const open = () => ref.current?.showModal();
    window.addEventListener(OPEN_SPONSOR, open);
    return () => window.removeEventListener(OPEN_SPONSOR, open);
  }, []);

  async function pick(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0] ?? null;
    if (!file) return setLogo(null);
    const src = await readLogo(file);
    setLogo({ src, name: file.name });
    setError(src ? "" : "Logo must be an image under 200 KB.");
  }

  async function submit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const field = (k: string) => String(data.get(k) ?? "").trim();
    const form = { name: field("name"), link: field("link"), description: field("description"), email: field("email") };
    let url: URL;
    try {
      url = new URL(/^https?:\/\//i.test(form.link) ? form.link : `https://${form.link}`);
    } catch {
      return setError("Enter a valid link, like acme.dev");
    }
    form.link = url.toString();
    setBusy(true);
    writePending({ ...form, logo: logo?.src, paid: false });
    window.location.href = sponsorCheckout(form);
  }

  return (
    <dialog
      ref={ref}
      className="sponsor-dialog"
      aria-labelledby="sponsor-dialog-title"
      onClose={() => setError("")}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <form onSubmit={submit}>
        <div className="dialog-head">
          <div>
            <h2 id="sponsor-dialog-title">Claim a sponsor slot</h2>
            <p className="dialog-lead">
              <strong>{SPONSOR_PRICE}</strong> once. Logo on OMG and a shoutout on X.
            </p>
          </div>
          <button type="button" className="dialog-close" aria-label="Close" onClick={() => ref.current?.close()}>
            <LuX aria-hidden="true" />
          </button>
        </div>
        <div className="dialog-row">
          <label>
            Product
            <input name="name" required maxLength={40} placeholder="Acme" autoComplete="organization" />
          </label>
          <label>
            Website
            <input name="link" required maxLength={200} inputMode="url" placeholder="acme.dev" />
          </label>
        </div>
        <label>
          Tagline
          <input name="description" required maxLength={100} placeholder="What it does, in one line" />
        </label>
        <label>
          Email
          <input name="email" type="email" required maxLength={120} placeholder="you@acme.dev" autoComplete="email" />
        </label>
        <label className="logo-pick">
          <span className="logo-thumb">{logo?.src ? <img src={logo.src} alt="" /> : <LuImagePlus aria-hidden="true" />}</span>
          <span className="logo-text">
            {logo ? logo.name : "Add logo"} <small>{logo ? "" : "optional, PNG or SVG"}</small>
          </span>
          <input name="logo" type="file" accept="image/png,image/svg+xml,image/jpeg,image/webp" onChange={pick} />
        </label>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <button type="submit" className="dialog-pay" disabled={busy}>
          {busy ? "Opening checkout..." : "Continue to payment"}
          {!busy && <LuArrowRight aria-hidden="true" />}
        </button>
        <p className="dialog-note">
          <LuLock aria-hidden="true" /> Secure checkout by Dodo Payments
        </p>
      </form>
    </dialog>
  );
}
