import ThemeToggle from "@/components/ThemeToggle";
import { banner } from "@/lib/banner";
import { CREATOR, CREATOR_URL, NPM, REPO, SHOW_SPONSORS, VERSION, doc } from "@/lib/site";

const FOOTER: { title: string; links: [string, string][] }[] = [
  {
    title: "Product",
    links: [
      ["How it works", "/#how"],
      ["What it finds", "/#finds"],
      ["Keys", "/#keys"],
      ["Safety", "/#safety"],
      ["Platforms", "/#platforms"],
      ["FAQ", "/#faq"],
      ...(SHOW_SPONSORS ? [["Sponsors", "/#sponsors"] as [string, string]] : []),
    ],
  },
  {
    title: "Docs",
    links: [
      ["Every command", "/docs"],
      ["Guides", "/guides"],
      ["Readme", doc("README.md")],
      ["Safety rules", doc("docs/safety.md")],
      ["Compatibility", doc("docs/compatibility.md")],
      ["Security policy", doc("SECURITY.md")],
    ],
  },
  {
    title: "Community",
    links: [
      ["GitHub", REPO],
      ["npm package", NPM],
      ["Releases", `${REPO}/releases`],
      ["Report an issue", `${REPO}/issues/new`],
      ["Contribute", `${REPO}/pulls`],
    ],
  },
];

export default function SiteFooter() {
  const [mark1, mark2] = banner(["OMG"]);
  return (
    <footer className="footer">
      <div className="footer-top">
        <div className="footer-brand">
          <pre className="mark" aria-hidden="true">{`${mark1}\n${mark2}`}</pre>
          <p className="footer-line">Your disk, explained.</p>
        </div>
        {FOOTER.map((col) => (
          <nav key={col.title} aria-label={col.title} className="footer-col">
            <p className="footer-title">{col.title}</p>
            <ul>
              {col.links.map(([label, href]) => (
                <li key={label}>
                  <a href={href}>{label}</a>
                </li>
              ))}
            </ul>
          </nav>
        ))}
      </div>
      <div className="footer-bottom">
        <p>
          &copy; {new Date().getFullYear()} OMG contributors &middot; Built by <a href={CREATOR_URL}>{CREATOR}</a> &middot;
          Licensed under <a href={doc("LICENSE-MIT")}>MIT</a> or <a href={doc("LICENSE-APACHE")}>Apache-2.0</a>
        </p>
        <div className="footer-end">
          <p>v{VERSION} &middot; Runs locally. No account, no telemetry.</p>
          <ThemeToggle />
        </div>
      </div>
    </footer>
  );
}
