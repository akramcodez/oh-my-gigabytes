import Link from "next/link";
import { LuBookOpen, LuCompass } from "react-icons/lu";
import { SiNpm } from "react-icons/si";
import SiteFooter from "@/components/SiteFooter";
import Stars from "@/components/Stars";
import { banner } from "@/lib/banner";
import { starCount } from "@/lib/github";
import { NPM } from "@/lib/site";

export default async function DocShell({ current, children }: { current?: "docs" | "guides"; children: React.ReactNode }) {
  const stars = await starCount();
  const [mark1, mark2] = banner(["OMG"]);
  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <div className="frame">
        <header className="topbar">
          <Link href="/" className="brand" aria-label="OMG home">
            <pre className="mark" aria-hidden="true">{`${mark1}\n${mark2}`}</pre>
          </Link>
          <nav aria-label="Site" className="topbar-actions">
            <Link href="/docs" className="text-link" aria-label="Docs" aria-current={current === "docs" ? "page" : undefined}>
              <LuBookOpen aria-hidden="true" />
              <span className="nav-label">Docs</span>
            </Link>
            <Link
              href="/guides"
              className="text-link"
              aria-label="Guides"
              aria-current={current === "guides" ? "page" : undefined}
            >
              <LuCompass aria-hidden="true" />
              <span className="nav-label">Guides</span>
            </Link>
            <a href={NPM} className="text-link" aria-label="npm package">
              <SiNpm aria-hidden="true" />
              <span className="nav-label">npm</span>
            </a>
            <Stars initial={stars} />
          </nav>
        </header>
        <main id="main" tabIndex={-1}>
          {children}
        </main>
        <SiteFooter />
      </div>
    </>
  );
}

export function SectionHead({ id, num, title, lead }: { id: string; num?: string; title: string; lead?: string }) {
  return (
    <div className="section-head">
      {num && <p className="kicker">{num}</p>}
      <h2 id={id}>{title}</h2>
      {lead && <p className="section-lead">{lead}</p>}
    </div>
  );
}

export function Shell({ children }: { children: string }) {
  return (
    <pre className="shell">
      <code>
        {children.split("\n").map((line, i) => {
          if (line.startsWith("#")) {
            return (
              <span key={i} className="shell-line shell-note">
                {line}
              </span>
            );
          }
          const at = line.search(/\s{2,}#/);
          const cmd = at < 0 ? line : line.slice(0, at);
          const note = at < 0 ? "" : line.slice(at);
          return (
            <span key={i} className="shell-line">
              <span className="prompt" aria-hidden="true">
                ${" "}
              </span>
              {cmd}
              {note && <span className="shell-note">{note}</span>}
            </span>
          );
        })}
      </code>
    </pre>
  );
}
