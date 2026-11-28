import type { Metadata } from "next";
import Link from "next/link";
import DocShell from "@/components/DocShell";
import { GUIDES } from "@/lib/guides";

export const metadata: Metadata = {
  title: "Guides: free up disk space as a developer",
  description:
    "Short guides to freeing disk space on a developer machine: Xcode DerivedData, node_modules, Docker, package manager caches and finding what uses your disk.",
  alternates: { canonical: "/guides" },
  openGraph: { url: "/guides", title: "OMG guides: free up disk space as a developer" },
};

export default function Guides() {
  return (
    <DocShell>
      <article className="doc-body doc-narrow">
        <p className="kicker">Guides</p>
        <h1>Free up disk space as a developer</h1>
        <p className="doc-lead">
          Each guide shows the manual way with the tools you already have, then how omg does it. Pick the one that matches
          what is filling your disk.
        </p>
        <ul className="guide-list">
          {GUIDES.map((g) => (
            <li key={g.slug}>
              <Link href={`/guides/${g.slug}`}>
                <h2>{g.title}</h2>
                <p>{g.summary}</p>
              </Link>
            </li>
          ))}
        </ul>
        <p className="doc-more">
          Looking for a command? See the <Link href="/docs">docs</Link>.
        </p>
      </article>
    </DocShell>
  );
}
