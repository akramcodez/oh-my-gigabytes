/* eslint-disable @next/next/no-img-element */
import type { Sponsor } from "@/lib/site";

type Props = { sponsor: Sponsor; size: number; alt: string; className?: string };

export default function SponsorLogo({ sponsor, size, alt, className = "" }: Props) {
  if (!sponsor.logo) return null;
  if (!sponsor.logoDark) return <img className={className} src={sponsor.logo} alt={alt} width={size} height={size} />;
  return (
    <>
      <img className={`${className} logo-light`} src={sponsor.logo} alt={alt} width={size} height={size} />
      <img className={`${className} logo-dark`} src={sponsor.logoDark} alt={alt} width={size} height={size} />
    </>
  );
}
