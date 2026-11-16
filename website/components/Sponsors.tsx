import { LuPlus } from "react-icons/lu";
import { SiX } from "react-icons/si";
import { REPO, SPONSORS, SPONSOR_CONTACT, SPONSOR_PRICE, SPONSOR_SLOTS, X_URL } from "@/lib/site";

export default function Sponsors() {
  const empty = Math.max(SPONSOR_SLOTS - SPONSORS.length, 1);
  return (
    <>
      <p className="sponsor-hint">{SPONSORS.length ? "Click an empty slot to add your logo" : "Slots are empty, click any slot to add your logo"}</p>
      <ul className="sponsors">
        {SPONSORS.map((s) => (
          <li key={s.name}>
            <a className="sponsor" href={s.url} target="_blank" rel="sponsored noopener">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              {s.logo && <img src={s.logo} alt="" width={32} height={32} />}
              <span className="sponsor-name">{s.name}</span>
              <span className="sponsor-tagline">{s.tagline}</span>
            </a>
          </li>
        ))}
        {Array.from({ length: empty }, (_, i) => (
          <li key={`open-${i}`}>
            <a className="sponsor is-open" href={SPONSOR_CONTACT} target="_blank" rel="noopener">
              <LuPlus aria-hidden="true" />
              <span className="sponsor-name">Your brand here</span>
              <span className="sponsor-tagline">Add yours &middot; {SPONSOR_PRICE}</span>
            </a>
          </li>
        ))}
      </ul>
      <p className="sponsor-foot">
        Have questions?
        <a href={X_URL} target="_blank" rel="noopener">
          <SiX aria-hidden="true" /> X / Twitter
        </a>
        <a href={`${REPO}/issues/new?title=${encodeURIComponent("Sponsor slot")}`} target="_blank" rel="noopener">
          GitHub
        </a>
      </p>
    </>
  );
}
