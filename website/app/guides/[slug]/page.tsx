import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import DocShell, { Cmd } from "@/components/DocShell";
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

  return (
    <DocShell>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(schema) }} />
      <article className="doc-body doc-narrow">
        <p className="kicker">
          <Link href="/guides">Guides</Link>
        </p>
        <h1>{guide.title}</h1>
        {guide.blocks.map((b, i) => (
          <section key={i}>
            {b.h && <h2>{b.h}</h2>}
            {b.p?.map((t) => <p key={t}>{t}</p>)}
            {b.list && (
              <ul className="doc-list">
                {b.list.map((t) => (
                  <li key={t}>{t}</li>
                ))}
              </ul>
            )}
            {b.code && <Cmd>{b.code}</Cmd>}
          </section>
        ))}
        <p className="doc-more">
          Every command is explained in the <Link href="/docs">docs</Link>.
        </p>
        <nav aria-label="More guides" className="guide-more">
          <p className="kicker">More guides</p>
          <ul>
            {others.map((g) => (
              <li key={g.slug}>
                <Link href={`/guides/${g.slug}`}>{g.title}</Link>
              </li>
            ))}
          </ul>
        </nav>
      </article>
    </DocShell>
  );
}
