import { LuPlus } from "react-icons/lu";
import PendingSlot from "@/components/PendingSlot";
import SponsorButton from "@/components/SponsorButton";
import SponsorLogo from "@/components/SponsorLogo";
import { SPONSORS } from "@/lib/site";

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
      <PendingSlot variant="nav" />
      <li>
        <SponsorButton className="nav-sponsor is-open" label="Sponsor OMG" title="Sponsor OMG">
          <LuPlus aria-hidden="true" />
        </SponsorButton>
      </li>
    </ul>
  );
}
