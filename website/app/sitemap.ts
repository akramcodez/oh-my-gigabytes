import type { MetadataRoute } from "next";
import { GUIDES } from "@/lib/guides";
import { SITE } from "@/lib/site";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const lastModified = new Date();
  return [
    { url: SITE, lastModified, changeFrequency: "weekly", priority: 1 },
    { url: `${SITE}/docs`, lastModified, changeFrequency: "weekly", priority: 0.8 },
    { url: `${SITE}/guides`, lastModified, changeFrequency: "monthly", priority: 0.6 },
    ...GUIDES.map((g) => ({ url: `${SITE}/guides/${g.slug}`, lastModified, changeFrequency: "monthly" as const, priority: 0.6 })),
  ];
}
