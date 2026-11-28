import Link from "next/link";
import { LuGithub } from "react-icons/lu";
import { SiNpm } from "react-icons/si";
import ThemeToggle from "@/components/ThemeToggle";
import { banner } from "@/lib/banner";
import { CREATOR, CREATOR_URL, NPM, REPO, VERSION } from "@/lib/site";

export default function DocShell({ children }: { children: React.ReactNode }) {
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
            <Link href="/docs" className="text-link">
              Docs
            </Link>
            <Link href="/guides" className="text-link">
              Guides
            </Link>
            <a href={NPM} className="text-link" aria-label="npm package">
              <SiNpm aria-hidden="true" />
            </a>
            <a href={REPO} className="text-link" aria-label="GitHub repository">
              <LuGithub aria-hidden="true" />
            </a>
          </nav>
        </header>
        <main id="main" tabIndex={-1}>
          {children}
        </main>
        <footer className="footer doc-footer">
          <div className="footer-bottom">
            <p>
              <Link href="/">Oh My Gigabytes</Link> &middot; <Link href="/docs">Docs</Link> &middot;{" "}
              <Link href="/guides">Guides</Link> &middot; Built by <a href={CREATOR_URL}>{CREATOR}</a>
            </p>
            <div className="footer-end">
              <p>v{VERSION} &middot; Runs locally. No account, no telemetry.</p>
              <ThemeToggle />
            </div>
          </div>
        </footer>
      </div>
    </>
  );
}

export function Cmd({ children }: { children: string }) {
  return (
    <pre className="doc-cmd">
      <code>{children}</code>
    </pre>
  );
}
