import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import CopyCommand from "@/components/CopyCommand";
import DocShell, { SectionHead, Shell } from "@/components/DocShell";
import { GUIDES } from "@/lib/guides";
import { INSTALL, SITE } from "@/lib/site";

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

  return (
    <DocShell current="guides">
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(schema) }} />
      <section className="hero compact">
        <p className="eyebrow">
          <Link href="/guides">Guides</Link> &middot; {String(index).padStart(2, "0")}
        </p>
        <h1>{guide.title}</h1>
        {intro?.p?.map((t) => (
          <p key={t} className="lede">
            {t}
          </p>
        ))}
      </section>

      <div className="section rows">
        {blocks.map((b, i) => (
          <section key={i} className={b.h === "With omg" ? "row is-omg" : "row"} aria-label={b.h}>
            <div className="row-head">{b.h && <h2>{b.h}</h2>}</div>
            <div className="row-body prose">
              {b.p?.map((t) => <p key={t}>{t}</p>)}
              {b.list && (
                <ul className="rules">
                  {b.list.map((t) => (
                    <li key={t}>{t}</li>
                  ))}
                </ul>
              )}
              {b.code && <Shell>{b.code}</Shell>}
            </div>
          </section>
        ))}
      </div>

      <nav className="section" aria-labelledby="more-title">
        <SectionHead id="more-title" title="More guides" lead="Every command is also explained in the docs." />
        <ol className="grid four index">
          {others.map((g) => (
            <li key={g.slug}>
              <Link href={`/guides/${g.slug}`}>
                <span className="num">{String(GUIDES.indexOf(g) + 1).padStart(2, "0")}</span>
                <h3>{g.title}</h3>
                <p>{g.summary}</p>
              </Link>
            </li>
          ))}
        </ol>
      </nav>

      <section className="cta" aria-label="Install">
        <h2>
          Your gigabytes, <em>accounted for.</em>
        </h2>
        <div className="hero-actions">
          <CopyCommand command={INSTALL} />
          <Link href="/docs" className="button">
            Every command
          </Link>
        </div>
      </section>
    </DocShell>
  );
}
