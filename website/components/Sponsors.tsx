import { LuMail, LuPlus } from "react-icons/lu";
import { SiX } from "react-icons/si";
import SponsorLogo from "@/components/SponsorLogo";
import {
  SPONSORS,
  SPONSOR_CHECKOUT,
  SPONSOR_EMAIL,
  SPONSOR_PRICE,
  X_URL,
} from "@/lib/site";

export default function Sponsors() {
  return (
    <>
      <div className="section-head">
        <h2 id="sponsors-title">
          Lifetime brand exposure, <em>one payment.</em>
        </h2>
        <p className="section-lead">
          Your logo on OMG plus a shoutout on X, for{" "}
          <strong className="sponsor-amount">{SPONSOR_PRICE}</strong> once.
        </p>
      </div>
      <div className="sponsor-shelf">
        <p className="sponsor-hint">Click an empty slot to add your logo</p>
        <ul className="sponsor-icons">
          {SPONSORS.map((s) => (
            <li key={s.name}>
              <a
                className="icon-slot"
                href={s.url}
                target="_blank"
                rel={s.house ? "noopener" : "sponsored noopener"}
              >
                <span className="icon-box">
                  <SponsorLogo sponsor={s} size={96} alt="" />
                </span>
                <span className="icon-name">{s.name}</span>
              </a>
            </li>
          ))}
          <li>
            <a
              className="icon-slot is-open"
              href={SPONSOR_CHECKOUT}
              target="_blank"
              rel="noopener"
              aria-label={`Add your brand for ${SPONSOR_PRICE}`}
            >
              <span className="icon-box" aria-hidden="true">
                <LuPlus />
              </span>
              <span className="icon-name">Add yours</span>
            </a>
          </li>
        </ul>
        <p className="sponsor-ask">
          Have questions?
          <a href={X_URL} target="_blank" rel="noopener">
            <SiX className="x-logo" aria-hidden="true" />X / Twitter
          </a>
          <a href={`mailto:${SPONSOR_EMAIL}`}>
            <LuMail aria-hidden="true" />
            Email
          </a>
        </p>
      </div>
    </>
  );
}
