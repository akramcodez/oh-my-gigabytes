import CopyCommand from "@/components/CopyCommand";
import Downloads from "@/components/Downloads";
import Sponsors from "@/components/Sponsors";
import NavSponsors from "@/components/NavSponsors";
import SponsorDialog from "@/components/SponsorDialog";
import Stars from "@/components/Stars";
import { starCount } from "@/lib/github";
import { totalDownloads } from "@/lib/npm";
import KeyNav from "@/components/KeyNav";
import Terminal from "@/components/Terminal";
import ThemeToggle from "@/components/ThemeToggle";
import { banner } from "@/lib/banner";
import { LuBookOpen, LuGithub } from "react-icons/lu";
import { SiNpm } from "react-icons/si";
import { CREATOR, CREATOR_URL, DESCRIPTION, INSTALL, NAME, NPM, REPO, SHOW_SPONSORS, SITE, VERSION, X_URL, doc } from "@/lib/site";

const SCHEMA = {
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: NAME,
  alternateName: "OMG",
  description: DESCRIPTION,
  url: SITE,
  downloadUrl: NPM,
  codeRepository: REPO,
  sameAs: [REPO, NPM, X_URL],
  softwareVersion: VERSION,
  applicationCategory: "DeveloperApplication",
  operatingSystem: "macOS, Linux, Windows",
  license: ["https://spdx.org/licenses/MIT.html", "https://spdx.org/licenses/Apache-2.0.html"],
  isAccessibleForFree: true,
  offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
};

const FOOTER: { title: string; links: [string, string][] }[] = [
  {
    title: "Product",
    links: [
      ["How it works", "#how"],
      ["What it finds", "#finds"],
      ["Keys", "#keys"],
      ["Safety", "#safety"],
      ["Platforms", "#platforms"],
      ["FAQ", "#faq"],
      ...(SHOW_SPONSORS ? [["Sponsors", "#sponsors"] as [string, string]] : []),
    ],
  },
  {
    title: "Docs",
    links: [
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

const NAV = [
  { id: "how", label: "how it works" },
  { id: "finds", label: "what it finds" },
  { id: "keys", label: "keys" },
  { id: "safety", label: "safety" },
  { id: "platforms", label: "platforms" },
  { id: "faq", label: "faq" },
];

const STEPS = [
  {
    title: "Scan",
    cmd: "omg scan",
    body: "Walks your home folder and stores names, sizes and times in a local SQLite index. File contents are not read. The one exception: to judge an old Git clone, it reads Git's own config, branch references and index. Nothing is uploaded.",
  },
  {
    title: "Explain",
    cmd: "omg findings",
    body: "Every candidate comes with the rule that matched, the evidence, what regenerates it and its warnings. Folders omg cannot read show as unknown, never as empty.",
  },
  {
    title: "Review",
    cmd: "4  OMG Trash",
    body: "Suggestions land in OMG Trash, a saved list. Nothing moves. Press u to keep something, or space in Browse and Findings to add more.",
  },
  {
    title: "Confirm",
    cmd: "d  /  D",
    body: "\"Are you sure?\" shows the exact paths and sizes, with Cancel as the default. Each item is re-checked just before it is removed, and skipped if it changed.",
  },
];

const FINDS: { title: string; tag: string; body: string; note?: string }[] = [
  {
    title: "Package caches",
    tag: "package_caches",
    body: "npm, npx, pnpm, Yarn, Bun, pip, uv, conda, Cargo, Go, Gradle and its old distributions, Maven, Ivy, Coursier, NuGet, pub, Homebrew, Deno, CocoaPods, Composer, Terraform, minikube, Julia, ccache, sccache, pre-commit, Electron, node-gyp, Cypress, Playwright and Puppeteer.",
  },
  {
    title: "Build output",
    tag: "build_artifacts",
    body: "Build output and dependencies beside the project file that made them: Rust, Node and Next.js, Nuxt, SvelteKit, Gatsby, Gradle and Kotlin, Maven, sbt, Clojure, .NET, PHP, Ruby, R, Elixir, OCaml, Haskell, Swift, CocoaPods, Flutter, Zig, CMake, vcpkg, Terraform, Unity, Unreal, Godot and Python virtualenvs of any name, plus Xcode DerivedData. Also test coverage, Vite and nyc caches, Visual Studio .vs, Carthage and SwiftPM builds, a custom CARGO_TARGET_DIR and dist or build folders of monorepo packages. Each says when the project was last worked on; one touched this week is never suggested.",
  },
  {
    title: "Unused Git clones",
    tag: "unused_clones",
    body: "Untouched for 30 days, every branch equal to the remote, no local commits or stashes, and every file matching Git's own index exactly: same files, sizes and times, nothing extra. Git itself is never run.",
    note: "never taken by omg clean",
  },
  {
    title: "Old tool versions",
    tag: "old_tool_versions",
    body: "Older nvm, pyenv and rustup versions of the same line, older JetBrains IDE and Android Studio versions, and older VS Code and Cursor extension versions, once the newer one has settled.",
  },
  {
    title: "Unused toolchains",
    tag: "unused_toolchains",
    body: "rustup, nvm, pyenv, SDKMAN! and the Android SDK, unchanged for 5 months with no build of that language in that time.",
  },
  {
    title: "Unused app data",
    tag: "unused_apps",
    body: "Application data folders of 50 MB or more with nothing changed for 5 months. Never one with a database or key inside.",
    note: "settings and sign-ins go with it",
  },
  {
    title: "App and IDE caches",
    tag: "app_caches",
    body: "App caches of 50 MB or more in ~/Library/Caches, JetBrains IDE caches, iOS device support files and Simulator caches.",
  },
  {
    title: "Logs and temp files",
    tag: "logs_and_temp",
    body: "On macOS and Windows: logs, crash dumps and temporary files of 1 MB or more, unchanged for 7 days.",
  },
  {
    title: "Coding agent sessions",
    tag: "session_history",
    body: "Claude Code, Codex, Pi, GitHub Copilot CLI, Cursor CLI and Gemini CLI sessions of 1 MB or more, unchanged for 7 days. New tools can be added by a signed catalog rule.",
    note: "your own history",
  },
  {
    title: "Older copies of builds",
    tag: "duplicates",
    body: "Release and build folders whose files a newer sibling also has, version numbers aside. The newest copy is never listed. For review only.",
  },
  {
    title: "Developer data",
    tag: "developer_data",
    body: "Android emulators, conda environments unused for 3 months and, on macOS, Xcode archives. For review only.",
    note: "your own data",
  },
  {
    title: "Data of removed apps",
    tag: "app_leftovers",
    body: "On macOS: Library folders unchanged for 3 months whose app is no longer in Applications, judged by folder names only. For review only.",
  },
  {
    title: "Fast growth",
    tag: "growth",
    body: "Folders that grew by 256 MB or more since the scan before and nothing else explains. omg unexplained lists the biggest space no finding covers.",
  },
  {
    title: "Old installers",
    tag: "old_downloads",
    body: "Disk images, installers and archives of 10 MB or more in Downloads, unchanged for 30 days. For review only.",
  },
  {
    title: "Stores no rule knows",
    tag: "unrecognized",
    body: "Big old folders in a tool's own area that look like a cache, log or history store. Listed for review only; omg catalog draft turns one into a rule.",
  },
  {
    title: "Docker data",
    tag: "omg docker clean",
    body: "Asks your local Docker engine for dangling images older than 7 days and build cache unused for 30. Docker removes exactly those, one by one, without force.",
    note: "volumes and running containers untouched",
  },
  {
    title: "Downloaded models",
    tag: "ml_models",
    body: "Hugging Face, Ollama and LM Studio folders are listed in Findings but never suggested: file times cannot tell when a model was last used.",
  },
  {
    title: "Large files",
    tag: "large_files",
    body: "Big files are listed for you to look at. Size alone is not evidence that a file is unwanted, so they are not suggested.",
  },
  {
    title: "What grew",
    tag: "omg grew",
    body: "Names the folders that grew between your last two scans, so you can see which cache or project keeps growing.",
  },
];

const KEYS: [string, string][] = [
  ["1", "Overview: disk use and the biggest folders"],
  ["2", "Browse: folders by size"],
  ["3", "Findings: what can be cleaned and why"],
  ["4", "OMG Trash: what you or the scan picked"],
  ["↑ ↓  j k", "move"],
  ["⏎  ⌫", "open, go back"],
  ["␣", "add to or remove from OMG Trash"],
  ["u", "remove from the list"],
  ["d", "delete the selected item"],
  ["D", "delete everything listed"],
  ["r", "scan again"],
  ["m  ?  q", "options, all keys, quit"],
];

const COMMANDS: [string, string][] = [
  ['omg scan --root "$HOME" --accept-gaps', "index your home folder without opening the interface"],
  ["omg", "the interface"],
  ["omg status", "totals, freed so far, and how much space omg itself uses"],
  ["omg tree ~/code", "folder sizes under a path"],
  ["omg findings", "what can be cleaned, with explanations, as JSON"],
  ["omg grew", "which folders grew since the scan before"],
  ["omg unexplained", "the biggest folders no finding explains"],
  ["omg clean --category logs_and_temp --older-than 14d", "list a selection and its fingerprint; add --confirm <fingerprint> to delete exactly that"],
  ["omg docker clean", "unused Docker data, through Docker itself"],
  ["omg schedule enable --hour 12", "macOS and Windows: a daily scan that never deletes (Linux: omg start)"],
  ["omg doctor", "read-only health check"],
];

const NEVER = [
  "Anything outside your home folder",
  "Credentials, keys and .ssh",
  "Databases and disk images",
  ".git folders and active Git repositories",
  "Cloud-synced folders",
  "Container and VM disks (.colima, Docker Desktop)",
  "Docker volumes and running containers",
  "On macOS: iCloud placeholders, locked files, app bundles and ~/Library itself",
];

const RULES = [
  "Links are never followed, and scans stop at mount boundaries.",
  "Protection rules run separately from the finding catalog. A catalog rule can only explain, never grant a delete.",
  "Every delete is a plan you confirm. Before removing each item omg checks it is still the same file, and skips it if not.",
  "Whole projects and an app's data are deleted only after you confirm them in the interface, never by omg clean.",
  "No telemetry. Notifications and default diagnostics never contain paths or file names.",
  "The npm package runs no install scripts and downloads nothing. Every launch re-verifies the binary against an Ed25519-signed manifest.",
];

const PLATFORMS = [
  { os: "Linux", detail: "x86_64, ARM64 · glibc 2.28+, kernel 5.6+", scans: "ext4, XFS, Btrfs, tmpfs", deletes: "ext4, Btrfs, tmpfs", note: "Not musl. Optional background scan service." },
  { os: "macOS", detail: "15 or newer · Apple silicon", scans: "APFS, HFS+, exFAT, FAT", deletes: "APFS", note: "No Intel Macs. Allow your terminal in Privacy & Security to scan Desktop, Documents and Downloads." },
  { os: "Windows", detail: "Windows 11 x64", scans: "NTFS, ReFS", deletes: "NTFS", note: "FAT32 and exFAT are refused." },
];

const FAQ: { q: string; a: React.ReactNode }[] = [
  {
    q: "Is everything it suggests safe to delete?",
    a: "No tool can promise that. Suggestions are rules applied to file metadata: a cache can be downloaded again, but rebuilding takes time, and the app data and toolchain rules judge by the newest change time, not by last use. Read the list in OMG Trash before you confirm. Delete is permanent.",
  },
  {
    q: "Does OMG Trash move my files?",
    a: "No. It is a saved list of paths. Nothing moves until you press d or D and choose \"Yes, delete permanently\". OMG does not use the system trash in this release, so there is no undo.",
  },
  {
    q: "Does anything leave my machine?",
    a: "No. The index is a SQLite file in a private state folder on your machine. There is no telemetry, and the Docker feature talks only to a local engine.",
  },
  {
    q: "Can I script it?",
    a: (
      <>
        Yes. Commands print JSON that follows a stable output schema: fields are added, never removed or renamed, and an unknown size is <code>null</code>, never <code>0</code>. <code>omg clean</code> deletes only with <code>--confirm</code> and the exact fingerprint of the selection it printed.
      </>
    ),
  },
  {
    q: "Can it scan in the background?",
    a: (
      <>
        On Linux, <code>omg start</code> runs an optional per-user service. On macOS and Windows, <code>omg schedule enable</code> registers a daily scan. Both only scan; neither deletes anything.
      </>
    ),
  },
  {
    q: "Why install it from npm?",
    a: "npm is how the signed native binary is shipped to all three systems. The package needs Node 22 or newer, runs no install scripts, downloads nothing and verifies the binary each time it starts.",
  },
  {
    q: "What if the index gets damaged?",
    a: "omg refuses to use an index it cannot read and changes nothing. Move its state folder aside and scan again; you lose only the deletion history and trend samples.",
  },
];

export default async function Home() {
  const [stars, downloads] = await Promise.all([starCount(), totalDownloads()]);
  const [mark1, mark2] = banner(["OMG"]);

  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(SCHEMA) }} />
      <KeyNav targets={NAV.map((n) => n.id)} />

      <div className="frame">
        <header className="topbar">
          <a href="#top" className="brand" aria-label="OMG home">
            <pre className="mark" aria-hidden="true">{`${mark1}\n${mark2}`}</pre>
          </a>
          <nav aria-label="Project" className="topbar-actions">
            {SHOW_SPONSORS && <NavSponsors />}
            <a href={doc("README.md")} className="text-link" aria-label="Docs">
              <LuBookOpen aria-hidden="true" />
              <span className="nav-label">Docs</span>
            </a>
            <a href={NPM} className="text-link" aria-label="npm package">
              <SiNpm aria-hidden="true" />
              <span className="nav-label">npm</span>
            </a>
            <Stars initial={stars} />
          </nav>
        </header>

        <main id="main" tabIndex={-1}>
          <section id="top" className="hero">
            <p className="eyebrow">
              v{VERSION} &middot; macOS, Linux and Windows <Downloads initial={downloads} />
            </p>
            <h1>
              See where your disk went.
              <br />
              <em>Take it back on your terms.</em>
            </h1>
            <p className="lede">
              A keyboard-first disk analyzer for developers. It finds the caches, build folders and old toolchains filling
              your disk, and deletes only what you confirm.
            </p>
            <div className="hero-actions">
              <CopyCommand command={INSTALL} />
              <a href={REPO} className="button">
                <LuGithub aria-hidden="true" />
                Source code
              </a>
            </div>
            <p className="fine">
              Node 22 or newer, then run <code>omg</code>. Linux, macOS 15+ on Apple silicon, Windows 11 x64.
            </p>
          </section>

          <div className="stage">
            <Terminal />
          </div>

          <ul className="facts" aria-label="At a glance">
            <li>
              <strong>Local only</strong>
              <span>A SQLite index on your machine. No telemetry, nothing uploaded.</span>
            </li>
            <li>
              <strong>Metadata, not contents</strong>
              <span>Names, sizes and times. Only Git&apos;s own config, refs and index are read, to judge old clones.</span>
            </li>
            <li>
              <strong>You confirm every delete</strong>
              <span>Suggestions go to a list. Nothing moves until you say yes.</span>
            </li>
          </ul>

          <section id="how" className="section" tabIndex={-1} aria-labelledby="how-title">
            <SectionHead id="how-title" title="How it works" lead="Four steps. The last one is always yours." />
            <ol className="steps">
              {STEPS.map((step, i) => (
                <li key={step.title}>
                  <span className="num">{String(i + 1).padStart(2, "0")}</span>
                  <h3>{step.title}</h3>
                  <code>{step.cmd}</code>
                  <p>{step.body}</p>
                </li>
              ))}
            </ol>
          </section>

          <section id="finds" className="section" tabIndex={-1} aria-labelledby="finds-title">
            <SectionHead
              id="finds-title"
              title="What it finds"
              lead="Each finding is a fixed, reviewed rule with the evidence attached. Most are suggested after a scan; you can turn that off."
            />
            <ul className="grid">
              {FINDS.map((f) => (
                <li key={f.title}>
                  <code className="tag">{f.tag}</code>
                  <h3>{f.title}</h3>
                  <p>{f.body}</p>
                  {f.note && <p className="note">{f.note}</p>}
                </li>
              ))}
            </ul>
          </section>

          <section id="keys" className="section" tabIndex={-1} aria-labelledby="keys-title">
            <SectionHead
              id="keys-title"
              title="Keyboard first"
              lead="Four views and a handful of keys. Everything also works as a command that prints JSON."
            />
            <div className="split">
              <dl className="keys">
                {KEYS.map(([k, label]) => (
                  <div key={k}>
                    <dt>
                      {k.split("  ").map((part) => (
                        <kbd key={part}>{part}</kbd>
                      ))}
                    </dt>
                    <dd>{label}</dd>
                  </div>
                ))}
              </dl>
              <ul className="commands">
                {COMMANDS.map(([cmd, what]) => (
                  <li key={cmd}>
                    <code>
                      <span className="prompt" aria-hidden="true">
                        ${" "}
                      </span>
                      {cmd}
                    </code>
                    <span>{what}</span>
                  </li>
                ))}
              </ul>
            </div>
          </section>

          <section id="safety" className="section" tabIndex={-1} aria-labelledby="safety-title">
            <SectionHead
              id="safety-title"
              title="What it never deletes"
              lead="These refusals live in the deletion code itself. No flag, setting or catalog rule turns them off."
            />
            <div className="split">
              <ul className="never">
                {NEVER.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
              <div>
                <div className="warning" role="note">
                  <p className="warning-title">Delete is permanent</p>
                  <p>
                    OMG does not move files to the system trash and there is no undo. Freed space can be less than shown, because hard links,
                    APFS clones and snapshots share data.
                  </p>
                </div>
                <ul className="rules">
                  {RULES.map((rule) => (
                    <li key={rule}>{rule}</li>
                  ))}
                </ul>
              </div>
            </div>
          </section>

          <section id="platforms" className="section" tabIndex={-1} aria-labelledby="platforms-title">
            <SectionHead
              id="platforms-title"
              title="Platforms"
              lead="Native scanning on all three. No platform is certified; the test records are in the repository."
            />
            <div className="table-wrap">
              <table className="platforms">
                <thead>
                  <tr>
                    <th scope="col">System</th>
                    <th scope="col">Scans</th>
                    <th scope="col">Deletes on</th>
                    <th scope="col">Notes</th>
                  </tr>
                </thead>
                <tbody>
                  {PLATFORMS.map((p) => (
                    <tr key={p.os}>
                      <th scope="row">
                        {p.os}
                        <span>{p.detail}</span>
                      </th>
                      <td data-label="Scans">{p.scans}</td>
                      <td data-label="Deletes on">{p.deletes}</td>
                      <td data-label="Notes">{p.note}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="fine pad">
              Not in this release: moving to the system trash, a background service on macOS or Windows, Authenticode
              signing, Apple notarization, Intel Macs, Windows on Arm and musl. Test records:{" "}
              <a href={doc("docs/linux-qualification.md")}>Linux</a>, <a href={doc("docs/macos-evidence.md")}>macOS</a>,{" "}
              <a href={doc("docs/windows-evidence.md")}>Windows</a>.
            </p>
          </section>

          <section id="faq" className="section" tabIndex={-1} aria-labelledby="faq-title">
            <SectionHead id="faq-title" title="Questions" />
            <div className="faq">
              {FAQ.map((item) => (
                <details key={item.q}>
                  <summary>{item.q}</summary>
                  <div className="faq-answer">{item.a}</div>
                </details>
              ))}
            </div>
          </section>

          {SHOW_SPONSORS && (
            <>
              <section id="sponsors" className="section" tabIndex={-1} aria-labelledby="sponsors-title">
                <Sponsors />
              </section>
              <SponsorDialog />
            </>
          )}

          <section className="cta" aria-label="Install">
            <h2>
              Your gigabytes, <em>accounted for.</em>
            </h2>
            <CopyCommand command={INSTALL} />
          </section>
        </main>

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
              &copy; {new Date().getFullYear()} OMG contributors &middot; Built by{" "}
              <a href={CREATOR_URL}>{CREATOR}</a> &middot; Licensed under{" "}
              <a href={doc("LICENSE-MIT")}>MIT</a> or <a href={doc("LICENSE-APACHE")}>Apache-2.0</a>
            </p>
            <div className="footer-end">
              <p>
                v{VERSION} &middot; Runs locally. No account, no telemetry.
              </p>
              <ThemeToggle />
            </div>
          </div>
        </footer>
      </div>
    </>
  );
}

function SectionHead({ id, title, lead, kicker }: { id: string; title: string; lead?: string; kicker?: string }) {
  return (
    <div className="section-head">
      {kicker && <p className="kicker">{kicker}</p>}
      <h2 id={id}>{title}</h2>
      {lead && <p className="section-lead">{lead}</p>}
    </div>
  );
}
