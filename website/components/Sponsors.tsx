import { LuPlus } from "react-icons/lu";
import SponsorLogo from "@/components/SponsorLogo";
import { REPO, SPONSORS, SPONSOR_CHECKOUT, SPONSOR_PRICE, SPONSOR_SLOTS, X_URL } from "@/lib/site";

export default function Sponsors() {
  const empty = Math.max(SPONSOR_SLOTS - SPONSORS.length, 1);
  return (
    <>
      <div className="section-head">
        <h2 id="sponsors-title">
          Lifetime brand exposure, <em>one payment.</em>
        </h2>
        <p className="section-lead">
          Showcase your product to developers on OMG and get a dedicated shoutout on X. Click an empty slot to add
          your logo.
        </p>
        <p className="sponsor-price">
          <span className="sponsor-amount">{SPONSOR_PRICE}</span>
          <span className="sponsor-terms">one-time, for a lifetime slot and a shoutout on X</span>
        </p>
        <p className="sponsor-ask">
          Have questions?
          <a href={X_URL} target="_blank" rel="noopener">
            X / Twitter
          </a>
          <a href={`${REPO}/issues/new?title=${encodeURIComponent("Sponsor slot")}`} target="_blank" rel="noopener">
            GitHub
          </a>
        </p>
      </div>
      <ul className="grid sponsor-grid">
        {SPONSORS.map((s) => (
          <li key={s.name}>
            <a className="slot" href={s.url} target="_blank" rel={s.house ? "noopener" : "sponsored noopener"}>
              <SponsorLogo sponsor={s} className="slot-logo" size={48} alt="" />
              <h3>{s.name}</h3>
              <p>{s.tagline}</p>
            </a>
          </li>
        ))}
        {Array.from({ length: empty }, (_, i) => (
          <li key={`open-${i}`}>
            <a className="slot is-open" href={SPONSOR_CHECKOUT} target="_blank" rel="noopener" aria-label={`Add your brand for ${SPONSOR_PRICE}`}>
              <span className="slot-plus" aria-hidden="true">
                <LuPlus />
              </span>
              <h3>Your brand here</h3>
              <p>Add yours &middot; {SPONSOR_PRICE}</p>
            </a>
          </li>
        ))}
      </ul>
    </>
  );
}
