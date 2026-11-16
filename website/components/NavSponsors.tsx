import { LuPlus } from "react-icons/lu";
import SponsorLogo from "@/components/SponsorLogo";
import { SPONSORS, SPONSOR_CHECKOUT } from "@/lib/site";

export default function NavSponsors() {
  return (
    <ul className="nav-sponsors" aria-label="Sponsors">
      {SPONSORS.map((s) => (
        <li key={s.url}>
          <a className="nav-sponsor" href={s.url} target="_blank" rel={s.house ? "noopener" : "sponsored noopener"} title={s.name}>
            {s.logo ? (
              <SponsorLogo sponsor={s} size={32} alt={s.name} />
            ) : (
              <span aria-label={s.name}>{s.name.slice(0, 1)}</span>
            )}
          </a>
        </li>
      ))}
      <li>
        <a className="nav-sponsor is-open" href={SPONSOR_CHECKOUT} target="_blank" rel="noopener" title="Sponsor OMG" aria-label="Sponsor OMG">
          <LuPlus aria-hidden="true" />
        </a>
      </li>
    </ul>
  );
}
