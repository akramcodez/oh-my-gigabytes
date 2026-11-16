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
          Showcase your product to developers on OMG and get a dedicated shoutout on X.
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
      <div className="sponsor-shelf">
        <p className="sponsor-hint">Click an empty slot to add your logo</p>
        <ul className="sponsor-icons">
          {SPONSORS.map((s) => (
            <li key={s.name}>
              <a className="icon-slot" href={s.url} target="_blank" rel={s.house ? "noopener" : "sponsored noopener"}>
                <span className="icon-box">
                  <SponsorLogo sponsor={s} size={96} alt="" />
                </span>
                <span className="icon-name">{s.name}</span>
              </a>
            </li>
          ))}
          {Array.from({ length: empty }, (_, i) => (
            <li key={`open-${i}`}>
              <a className="icon-slot is-open" href={SPONSOR_CHECKOUT} target="_blank" rel="noopener" aria-label={`Add your brand for ${SPONSOR_PRICE}`}>
                <span className="icon-box" aria-hidden="true">
                  <LuPlus />
                </span>
                <span className="icon-name">Add yours</span>
              </a>
            </li>
          ))}
        </ul>
      </div>
    </>
  );
}
