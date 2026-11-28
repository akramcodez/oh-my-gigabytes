import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import DocShell, { DocLayout, type NavLink, Note, Shell } from "@/components/DocShell";
import { GUIDES } from "@/lib/guides";
import { SITE } from "@/lib/site";

export const dynamicParams = false;

export function generateStaticParams() {
  return GUIDES.map((g) => ({ slug: g.slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const guide = GUIDES.find((g) => g.slug === slug);
  if (!guide) return {};
  return {
    title: guide.title,
    description: guide.description,
    alternates: { canonical: `/guides/${guide.slug}` },
    openGraph: { type: "article", url: `/guides/${guide.slug}`, title: guide.title, description: guide.description },
  };
}

export default async function GuidePage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const guide = GUIDES.find((g) => g.slug === slug);
  if (!guide) notFound();
  const others = GUIDES.filter((g) => g.slug !== guide.slug);
  const index = GUIDES.indexOf(guide) + 1;
  const intro = guide.blocks[0].h ? undefined : guide.blocks[0];
  const blocks = intro ? guide.blocks.slice(1) : guide.blocks;
  const schema = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "TechArticle",
        headline: guide.title,
        description: guide.description,
        url: `${SITE}/guides/${guide.slug}`,
        inLanguage: "en",
      },
      {
        "@type": "BreadcrumbList",
        itemListElement: [
          { "@type": "ListItem", position: 1, name: "OMG", item: `${SITE}/` },
          { "@type": "ListItem", position: 2, name: "Guides", item: `${SITE}/guides` },
          { "@type": "ListItem", position: 3, name: guide.title, item: `${SITE}/guides/${guide.slug}` },
        ],
      },
    ],
  };

  const anchor = (h: string) => h.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  const nav: NavLink[] = [
    { href: "#top", label: "Overview" },
    ...blocks.filter((b) => b.h).map((b) => ({ href: `#${anchor(b.h!)}`, label: b.h! })),
  ];

  return (
    <DocShell current="guides">
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(schema) }} />
      <DocLayout title="In this guide" nav={nav}>
        <header className="docs-head" id="top">
          <p className="eyebrow">
            <Link href="/guides">Guides</Link> &middot; {String(index).padStart(2, "0")}
          </p>
          <h1>{guide.title}</h1>
          {intro?.p?.map((t) => (
            <p key={t} className="docs-lede">
              {t}
            </p>
          ))}
        </header>

        {blocks.map((b, i) => {
          const body = (
            <>
              {b.p?.map((t) => <p key={t}>{t}</p>)}
              {b.list && (
                <ul className="rules">
                  {b.list.map((t) => (
                    <li key={t}>{t}</li>
                  ))}
                </ul>
              )}
              {b.code && <Shell>{b.code}</Shell>}
            </>
          );
          return (
            <section key={i} className="docs-section" aria-labelledby={b.h ? anchor(b.h) : undefined}>
              {b.h && (
                <h2 id={anchor(b.h)}>
                  <a href={`#${anchor(b.h)}`}>{b.h}</a>
                </h2>
              )}
              {b.h === "With omg" ? <Note>{body}</Note> : body}
            </section>
          );
        })}

        <section className="docs-section" aria-labelledby="more">
          <h2 id="more">More guides</h2>
          <ul className="doc-links">
            {others.map((g) => (
              <li key={g.slug}>
                <Link href={`/guides/${g.slug}`}>{g.title}</Link>
                <span>{g.summary}</span>
              </li>
            ))}
            <li>
              <Link href="/docs">Every omg command</Link>
              <span>Each command, option and key in plain words.</span>
            </li>
          </ul>
        </section>
      </DocLayout>
    </DocShell>
  );
}
