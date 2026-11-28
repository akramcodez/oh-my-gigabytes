import type { Metadata } from "next";
import Link from "next/link";
import CopyCommand from "@/components/CopyCommand";
import DocShell from "@/components/DocShell";
import { GUIDES } from "@/lib/guides";
import { INSTALL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Guides: free up disk space as a developer",
  description:
    "Short guides to freeing disk space on a developer machine: Xcode DerivedData, node_modules, Docker, package manager caches and finding what uses your disk.",
  alternates: { canonical: "/guides" },
  openGraph: { url: "/guides", title: "OMG guides: free up disk space as a developer" },
};

export default function Guides() {
  return (
    <DocShell current="guides">
      <section className="hero">
        <p className="eyebrow">Guides &middot; {GUIDES.length} short reads</p>
        <h1>
          Free up disk space
          <br />
          <em>as a developer.</em>
        </h1>
        <p className="lede">
          Each guide shows the manual way with the tools you already have, then how omg does it. Pick the one that matches
          what is filling your disk.
        </p>
      </section>

      <section className="section" aria-label="All guides">
        <ol className="grid index">
          {GUIDES.map((g, i) => (
            <li key={g.slug}>
              <Link href={`/guides/${g.slug}`}>
                <span className="num">{String(i + 1).padStart(2, "0")}</span>
                <h3>{g.title}</h3>
                <p>{g.summary}</p>
              </Link>
            </li>
          ))}
          <li>
            <Link href="/docs">
              <span className="num">Docs</span>
              <h3>Every omg command, in plain words</h3>
              <p>Each command, option and key, with an example.</p>
            </Link>
          </li>
        </ol>
      </section>

      <section className="cta" aria-label="Install">
        <h2>
          Your gigabytes, <em>accounted for.</em>
        </h2>
        <CopyCommand command={INSTALL} />
      </section>
    </DocShell>
  );
}
