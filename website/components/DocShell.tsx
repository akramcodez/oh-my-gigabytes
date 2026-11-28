import Link from "next/link";
import { LuBookOpen, LuCompass } from "react-icons/lu";
import { SiNpm } from "react-icons/si";
import SiteFooter from "@/components/SiteFooter";
import Stars from "@/components/Stars";
import TocSpy from "@/components/TocSpy";
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

export type NavLink = { href: string; label: string; code?: boolean; items?: NavLink[] };

function NavList({ links }: { links: NavLink[] }) {
  return (
    <ul>
      {links.map((l) => (
        <li key={l.href}>
          <a href={l.href}>{l.code ? <code>{l.label}</code> : l.label}</a>
          {l.items && <NavList links={l.items} />}
        </li>
      ))}
    </ul>
  );
}

export function DocLayout({ title, nav, children }: { title: string; nav: NavLink[]; children: React.ReactNode }) {
  return (
    <div className="docs">
      <aside className="docs-nav" aria-label={title}>
        <p className="docs-nav-title">{title}</p>
        <NavList links={nav} />
      </aside>
      <div className="docs-body">
        <details className="docs-toc">
          <summary>{title}</summary>
          <NavList links={nav} />
        </details>
        {children}
      </div>
      <TocSpy />
    </div>
  );
}

export function Note({ children, tone }: { children: React.ReactNode; tone?: "warn" }) {
  return (
    <div className={tone ? `note is-${tone}` : "note"} role="note">
      {children}
    </div>
  );
}
