import { LuArrowUpRight, LuPlus } from "react-icons/lu";
import { REPO, SPONSORS, SPONSOR_CONTACT, SPONSOR_PRICE, SPONSOR_SLOTS, X_URL } from "@/lib/site";

export default function Sponsors() {
  const empty = Math.max(SPONSOR_SLOTS - SPONSORS.length, 1);
  return (
    <div className="sponsor-wrap">
      <div className="sponsor-pitch">
        <p className="kicker">Sponsors</p>
        <h2 id="sponsors-title">
          Lifetime brand exposure,
          <br />
          <em>one payment.</em>
        </h2>
        <p className="section-lead">
          Showcase your product to developers on OMG and get a dedicated shoutout on X.
        </p>
        <div className="sponsor-price">
          <span className="sponsor-amount">{SPONSOR_PRICE}</span>
          <span className="sponsor-terms">one time &middot; your slot stays for life</span>
        </div>
        <a className="button sponsor-cta" href={SPONSOR_CONTACT} target="_blank" rel="noopener">
          Claim a slot <LuArrowUpRight aria-hidden="true" />
        </a>
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

      <div>
        <p className="sponsor-hint">
          {SPONSORS.length ? "Click an empty slot to add your logo" : "Slots are empty, click any slot to add your logo"}
        </p>
        <ul className="sponsors">
          {SPONSORS.map((s) => (
            <li key={s.name}>
              <a className="tile" href={s.url} target="_blank" rel="sponsored noopener">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                {s.logo && <img className="tile-logo" src={s.logo} alt="" width={28} height={28} />}
                <span className="tile-name">{s.name}</span>
                <span className="tile-sub">{s.tagline}</span>
              </a>
            </li>
          ))}
          {Array.from({ length: empty }, (_, i) => (
            <li key={`open-${i}`}>
              <a className="tile is-open" href={SPONSOR_CONTACT} target="_blank" rel="noopener" aria-label={`Add your brand for ${SPONSOR_PRICE}`}>
                <span className="tile-plus" aria-hidden="true">
                  <LuPlus />
                </span>
                <span className="tile-name">Your brand here</span>
                <span className="tile-sub">Add yours &middot; {SPONSOR_PRICE}</span>
              </a>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
