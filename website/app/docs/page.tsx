import type { Metadata } from "next";
import Link from "next/link";
import CopyCommand from "@/components/CopyCommand";
import DocShell, { DocLayout, type NavLink, Note, Shell } from "@/components/DocShell";
import { INSTALL, NAME, SITE, VERSION, doc } from "@/lib/site";

export const metadata: Metadata = {
  title: "Docs: every omg command explained",
  description:
    "Plain-language guide to the omg CLI: installing, scanning, the interface keys, findings, OMG Trash, deleting safely, auto delete, Docker cleanup, background scans and every command and option.",
  alternates: { canonical: "/docs" },
  openGraph: { url: "/docs", title: "OMG docs: every command explained" },
};

type Option = [string, string];
type Command = { name: string; what: string; example: string; options?: Option[]; note?: string };
type Group = { id: string; title: string; lead: string; commands: Command[] };

const WORDS: [string, string][] = [
  ["Scan", "omg walks a folder and writes down each item's name, size and dates. It does not open your files."],
  ["Index", "The list a scan produces, kept in a small SQLite file on your machine. Every command reads it."],
  ["Scan root", "The folder that was scanned. By default your home folder."],
  ["Finding", "Something omg recognises, such as an npm cache or an old build folder, with the reason it was picked."],
  ["Category", "The kind of finding, such as package_caches or build_artifacts. Used to choose what to clean."],
  ["OMG Trash", "A saved list of paths you might delete. Adding something to it does not move or delete anything."],
  ["Fingerprint", "A short code that stands for one exact selection. Deleting from a script needs it, so you delete only what you saw."],
  ["State folder", "Where omg keeps its index and settings. It never leaves your computer."],
];

const KEYS: [string[], string][] = [
  [["1", "2", "3", "4", "5"], "Switch tabs: Overview, Browse, Findings, OMG Trash, Deleted. Tab works too"],
  [["↑", "↓", "j", "k"], "Move up and down. PgUp and PgDn jump further"],
  [["⏎", "→"], "Open a folder or show details"],
  [["⌫", "←"], "Go up one folder"],
  [["␣"], "Add to OMG Trash, or take it out again. Nothing changes on disk"],
  [["d"], "Delete the selected item permanently, after asking"],
  [["D"], "In OMG Trash: delete everything listed, after asking"],
  [["u", "U"], "Take one item, or everything, out of OMG Trash. In Deleted: one line, or the whole log"],
  [["K"], "In OMG Trash: keep this item away from auto delete"],
  [["p"], "In Findings: record a dry-run preview of what deleting would do"],
  [["o"], "In Findings: show the item in your file manager"],
  [["t"], "In Findings: filter by tier"],
  [["r", "R"], "Scan again, or do a full rescan. Esc stops a running scan"],
  [["m"], "Options: auto delete, Docker cleanup, suggestions, theme"],
  [["?"], "Show every shortcut"],
  [["q"], "Quit"],
];

const TABS: [string, string][] = [
  ["Overview", "How full your disk is, how much the scan covered, and the biggest folders."],
  ["Browse", "Your folders sorted by size. Open one to see what is inside it."],
  ["Findings", "Everything omg recognises, with why it was picked and what happens if you delete it."],
  ["OMG Trash", "Your list of things to delete. Review it, then press d or D."],
  ["Deleted", "A log of what was deleted, when, how much space it freed and whether you or auto delete did it."],
];

const GROUPS: Group[] = [
  {
    id: "look",
    title: "Look at your disk",
    lead: "These commands only read. None of them changes a file.",
    commands: [
      {
        name: "omg",
        what: "Opens the interface. On the first run it scans your home folder for you. Use the keys listed above to move around.",
        example: "omg",
        note: "Needs a real terminal. On Windows, Git Bash's default window is not one: run winpty omg, or use Windows Terminal or PowerShell.",
      },
      {
        name: "omg scan",
        what: "Scans a folder and saves the result. Without --root it scans the folder you scanned last time, or your home folder. If some folders could not be read, omg keeps your previous result unless you pass --accept-gaps.",
        example: 'omg scan --root ~/code\nomg scan --root "$HOME" --accept-gaps',
        options: [
          ["--root PATH", "The folder to scan. Give its real path (on macOS /private/tmp, not /tmp)."],
          ["--accept-gaps", "Save the result even if some folders could not be read. It is marked partial."],
          ["--no-suggest", "Do not add regenerable caches to OMG Trash after this scan."],
          ["--exclude PATH", "Skip a folder. Repeat it for more than one."],
          ["--workers N", "How many threads scan at once. The default is up to 8."],
          ["--full", "Mark this scan as a full rescan instead of an update of the last one."],
        ],
        note: "omg scan prints a JSON report. When it adds suggestions to OMG Trash it says so on stderr; scripts that read the JSON should not merge stderr into it, or should pass --no-suggest.",
      },
      {
        name: "omg status",
        what: "A short summary: how much was scanned, when, how much omg can explain, how much you have freed so far and how big omg's own index is.",
        example: "omg status",
      },
      {
        name: "omg tree",
        what: "The biggest folders and files inside a path, largest first. Without a path it starts at the scan root.",
        example: "omg tree ~/Library\nomg tree ~/code --limit 10",
        options: [
          ["PATH", "A folder inside the scan root."],
          ["--limit N", "How many rows to show (default 25)."],
          ["--offset N", "Skip this many rows, to page through a long list."],
        ],
      },
      {
        name: "omg findings",
        what: "Everything omg recognises as possibly removable, biggest first. Each one says which rule matched, the evidence, what would recreate it and any warnings.",
        example: "omg findings",
        options: [["--offset N", "Skip this many findings, to see the next page."]],
      },
      {
        name: "omg grew",
        what: "Folders that got bigger between your last two complete scans, so you can see what keeps growing.",
        example: "omg grew --limit 10",
        options: [["--limit N", "How many folders to show (default 20)."]],
      },
      {
        name: "omg unexplained",
        what: "The biggest folders that no finding explains, with how much is left after the findings inside them. Good for spotting space omg does not know about yet.",
        example: "omg unexplained",
        options: [["--limit N", "How many folders to show (default 20)."]],
      },
      {
        name: "omg trends",
        what: "How your free space and the scanned size changed over time, from the samples each scan records.",
        example: "omg trends --days 90",
        options: [["--days N", "How far back to look (default 30)."]],
      },
    ],
  },
  {
    id: "trash",
    title: "Choose what to delete",
    lead: "OMG Trash is only a list. Nothing moves until you delete and confirm.",
    commands: [
      {
        name: "omg trash",
        what: "Shows what is in OMG Trash. After each scan omg adds safe-to-recreate caches here for you; you can turn that off in the options menu.",
        example: "omg trash\nomg trash --add ~/old-project\nomg trash --remove ~/old-project",
        options: [
          ["--add PATH", "Add a scanned path to the list."],
          ["--remove PATH", "Take a path off the list."],
          ["--refresh", "Work out the suggestions again from the current scan, without scanning."],
        ],
        note: "If you take a suggestion off the list, omg leaves it off. It comes back only after it has doubled or grown by 1 GiB, or after 30 days.",
      },
      {
        name: "omg clean",
        what: "Deletes whole categories in one go, for scripts. Run it once without --confirm: it lists exactly what would go and prints a fingerprint. Run it again with that fingerprint to delete exactly that list. If anything changed in between, it refuses.",
        example: "omg clean --category logs_and_temp --older-than 14d\nomg clean --category logs_and_temp --older-than 14d --confirm <fingerprint>",
        options: [
          ["--category NAME", "A category to include, such as package_caches. Repeat it for more. The list is below."],
          ["--older-than DAYS", "Only items unchanged for this long, written as 30 or 30d."],
          ["--confirm FINGERPRINT", "Delete exactly the list the first run printed."],
        ],
        note: "omg clean never takes a whole Git repository or an app's data. Those can only be deleted from the interface, after you confirm them there.",
      },
      {
        name: "omg cleanup --dry-run",
        what: "Records a preview of what deleting some findings would do, without deleting anything. The interface's p key does the same.",
        example: "omg cleanup --dry-run --candidate <finding-id>",
        options: [["--candidate ID", "One or more finding ids, from omg findings."]],
      },
      {
        name: "omg history",
        what: "Lists the dry-run previews you recorded.",
        example: "omg history",
      },
      {
        name: "omg ignores",
        what: "Lists findings you told omg to ignore. --remove brings one back.",
        example: "omg ignores\nomg ignores --remove 3",
        options: [["--remove ID", "Stop ignoring this one."]],
      },
    ],
  },
  {
    id: "docker",
    title: "Docker",
    lead: "omg asks your own Docker engine what is unused and lets Docker remove it. It never touches volumes or running containers.",
    commands: [
      {
        name: "omg docker clean",
        what: "Shows the unused Docker data that would go: dangling images older than 7 days and build cache unused for 30 days. Run it again with --confirm and the fingerprint it printed to remove exactly that.",
        example: "omg docker clean\nomg docker clean --confirm <fingerprint>",
        options: [
          ["--include-unused-images", "Also tagged images no container uses (they can be pulled or built again)."],
          ["--include-stopped-containers", "Also stopped containers. Changes inside them that are not in a volume are lost."],
          ["--confirm FINGERPRINT", "Remove exactly what the first run listed."],
        ],
        note: "Works only with a local engine. A remote DOCKER_HOST is refused.",
      },
    ],
  },
  {
    id: "auto",
    title: "Automatic scans and auto delete",
    lead: "Scanning on a schedule never deletes. Deleting in the background is a separate switch that is off until you turn it on.",
    commands: [
      {
        name: "omg auto-delete on",
        what: "Turns on auto delete after you type yes. A job then runs every few hours, scans, and deletes items that have sat in OMG Trash, unchanged, for the whole period you chose. You get a notification with the count.",
        example: "omg auto-delete on --every 7",
        options: [
          ["--every DAYS", "How long an item must wait in OMG Trash: 1, 7, 30 or 180 days (default 7)."],
          ["--include-risky", "Also allow whole repositories, application data and toolchains. Off by default."],
          ["--allow-battery", "Run on battery power too. By default it waits for mains power."],
        ],
      },
      {
        name: "omg auto-delete status",
        what: "Shows the settings, whether the job is registered and what the next run would delete.",
        example: "omg auto-delete status --items",
        options: [["--items", "List every item in OMG Trash and whether it is due, waiting, kept, changed or left for you."]],
      },
      {
        name: "omg auto-delete off",
        what: "Turns auto delete off at once and removes the job. A run in progress stops before its next item.",
        example: "omg auto-delete off",
      },
      {
        name: "omg auto-delete run",
        what: "One run by hand: scan, then delete what is due. This is what the scheduled job runs.",
        example: "omg auto-delete run",
      },
      {
        name: "omg schedule",
        what: "macOS and Windows: a daily scan in the background, so your numbers stay fresh. It only scans.",
        example: "omg schedule enable --hour 9\nomg schedule status\nomg schedule disable",
        options: [["enable --hour H", "Scan every day at this local hour, 0 to 23 (default 12)."]],
      },
      {
        name: "omg start / omg stop",
        what: "Linux: start or stop a background service that keeps the index fresh. It only scans. omg start asks before it registers anything.",
        example: "omg start\nomg stop",
      },
      {
        name: "omg service",
        what: "Linux: finer control over that service, a per-user systemd unit.",
        example: "omg service install --enable\nomg service status\nomg service scan\nomg service uninstall",
        options: [
          ["install", "Register the service. --enable also starts it at login; --yes skips the question."],
          ["start / stop / status", "Control it and see whether it is running."],
          ["scan", "Ask the running service for a low-priority scan."],
          ["uninstall", "Stop it and remove the registration. Your index is kept."],
          ["run", "Run the service in the foreground. The registered unit uses this."],
        ],
      },
    ],
  },
  {
    id: "settings",
    title: "Settings and upkeep",
    lead: "Small commands for the look, health and housekeeping of omg itself.",
    commands: [
      {
        name: "omg theme",
        what: "Shows or sets the interface colours. auto follows your terminal.",
        example: "omg theme dark",
        options: [["auto | dark | light", "The theme to use."]],
      },
      {
        name: "omg doctor",
        what: "A read-only health check: this build, what your system supports, the state folder and any scheduled jobs. It never changes or repairs anything.",
        example: "omg doctor\nomg doctor --json",
        options: [["--json", "Print only JSON, for scripts."]],
      },
      {
        name: "omg prune",
        what: "Finishes clearing out old scans from the index and gives the space back. omg does this a little after every scan, so you rarely need it.",
        example: "omg prune",
      },
      {
        name: "omg catalog",
        what: "The catalog is the list of rules that recognise caches and tools. These commands show it, load a signed update, go back to the previous one, or draft a new rule for a folder omg does not know.",
        example: "omg catalog status\nomg catalog draft ~/.some-tool/cache\nomg catalog import rules.toml rules.sig\nomg catalog rollback",
        options: [
          ["status", "Which catalog is in use."],
          ["draft PATH", "Print a draft rule for a tool's folder, built from folder listings only."],
          ["import FILE SIGNATURE", "Load a catalog update. It must carry a valid signature."],
          ["rollback", "Go back to the previous catalog."],
        ],
        note: "A catalog rule can only explain a folder. It can never make something deletable that the safety rules protect.",
      },
    ],
  },
];

const GLOBAL: Option[] = [
  ["--state-dir PATH", "Use a different state folder. Handy for trying things out without touching your real index."],
  ["--config PATH", "Use a different settings file for the background service."],
  ["--version, -V", "Print the version."],
  ["--help, -h", "Print help. Works after any command too, such as omg clean --help."],
  ["OMG_STATE_DIR", "Environment variable that sets the state folder, like --state-dir."],
];

const CATEGORIES: [string, string, boolean][] = [
  ["package_caches", "Download caches of package managers: npm, pnpm, Yarn, pip, uv, Cargo, Go, Gradle, Maven, Homebrew and many more.", true],
  ["build_artifacts", "Build output and dependencies next to a project: target, node_modules, .next, dist, virtualenvs, Xcode DerivedData.", true],
  ["app_caches", "Apps' own cache folders, such as those in ~/Library/Caches on macOS.", true],
  ["old_tool_versions", "Older versions of tools and editor extensions that a newer version replaced.", true],
  ["session_history", "Old chat and session logs of AI coding tools.", true],
  ["unused_clones", "Git clones untouched for 30 days that have nothing that is not already on the remote.", true],
  ["unused_apps", "Application data folders unchanged for about five months.", true],
  ["unused_toolchains", "Whole toolchains (rustup, nvm, pyenv, SDKMAN!, Android SDK) unused for about five months.", true],
  ["logs_and_temp", "Old logs, temporary files and crash dumps.", true],
  ["developer_data", "Simulators, emulators, SDKs, Go modules, conda environments. Listed, never suggested.", false],
  ["ml_models", "Downloaded models from Hugging Face, Ollama and LM Studio. Listed, never suggested.", false],
  ["old_downloads", "Installers, disk images and archives in Downloads older than 30 days.", false],
  ["duplicates", "Older copies of a folder that a newer sibling replaced.", false],
  ["growth", "Folders that grew by 256 MB or more between scans.", false],
  ["unrecognized", "Big, old folders that look like a cache or log but match no rule.", false],
  ["large_files", "Big files. Size alone is not a reason to delete, so they are only listed.", false],
];

const NEVER = [
  "Anything outside your home folder",
  "Credentials, keys and .ssh",
  "Databases and disk images",
  ".git folders and Git repositories you are working in",
  "Cloud-synced folders",
  "Container and VM disks",
  "Docker volumes and running containers",
  "On macOS: iCloud placeholders, locked files, app bundles and ~/Library itself",
];

const RULES = [
  "Every delete shows the exact paths and total size first, with Cancel selected.",
  "Just before removing each item, omg checks it is still the same one, and skips it if anything changed.",
  "Links are never followed, and a scan stops at the edge of another disk.",
  "A catalog rule can only explain a folder. It can never make a protected one deletable.",
  "omg clean never takes a whole Git repository or an app's data. Only the interface can, after you confirm.",
];

const slug = (name: string) => name.replace(/^omg\s*/, "").replace(/[^a-z0-9]+/gi, "-").replace(/-$/, "") || "omg";

const NAV: NavLink[] = [
  { href: "#start", label: "Getting started" },
  { href: "#words", label: "Words you will see" },
  { href: "#interface", label: "The interface" },
  ...GROUPS.map((g) => ({
    href: `#${g.id}`,
    label: g.title,
    items: g.commands.map((c) => ({ href: `#cmd-${slug(c.name)}`, label: c.name, code: true })),
  })),
  { href: "#global", label: "Options for every command" },
  { href: "#categories", label: "Categories" },
  { href: "#safety", label: "What is never deleted" },
  { href: "#files", label: "Data and uninstall" },
];

const SCHEMA = {
  "@context": "https://schema.org",
  "@type": "TechArticle",
  headline: "Every omg command explained",
  about: NAME,
  url: `${SITE}/docs`,
  inLanguage: "en",
};

function Options({ items, head = "Option" }: { items: Option[]; head?: string }) {
  return (
    <div className="table-wrap">
      <table className="doc-table">
        <thead>
          <tr>
            <th scope="col">{head}</th>
            <th scope="col">What it does</th>
          </tr>
        </thead>
        <tbody>
          {items.map(([opt, meaning]) => (
            <tr key={opt}>
              <td>
                <code>{opt}</code>
              </td>
              <td>{meaning}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function H2({ id, children }: { id: string; children: React.ReactNode }) {
  return (
    <h2 id={id}>
      <a href={`#${id}`}>{children}</a>
    </h2>
  );
}

export default function Docs() {
  return (
    <DocShell current="docs">
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(SCHEMA) }} />
      <DocLayout title="On this page" nav={NAV}>
        <header className="docs-head" id="start">
          <p className="eyebrow">Docs &middot; v{VERSION}</p>
          <h1>
            Every omg command, <em>in plain words.</em>
          </h1>
          <p className="docs-lede">
            omg shows where your disk space went and helps you take it back. It scans your home folder, recognises developer
            caches, build folders and old tools, explains each one, and deletes only what you confirm.
          </p>
        </header>

        <section className="docs-section" aria-labelledby="start-title">
          <h2 id="start-title" className="sr-only">
            Getting started
          </h2>
          <p>Install it with npm. You need Node 22 or newer.</p>
          <CopyCommand command={INSTALL} />
          <p>
            Then run <code>omg</code>. On its first start it scans your home folder, which takes from a few seconds to a few
            minutes. Everything the interface does also works as a command that prints JSON, so you can script it.
          </p>
          <Note>
            <p>
              <strong>On macOS</strong>, give your terminal Full Disk Access in System Settings, Privacy &amp; Security.
              Without it omg cannot see Desktop, Documents and Downloads.
            </p>
          </Note>
        </section>

        <section className="docs-section" aria-labelledby="words">
          <H2 id="words">Words you will see</H2>
          <p>A few terms come up everywhere. This is what each one means.</p>
          <dl className="terms">
            {WORDS.map(([term, meaning]) => (
              <div key={term}>
                <dt>{term}</dt>
                <dd>{meaning}</dd>
              </div>
            ))}
          </dl>
        </section>

        <section className="docs-section" aria-labelledby="interface">
          <H2 id="interface">The interface</H2>
          <p>
            Run <code>omg</code> with no command to open it. It has five tabs, switched with the number keys:
          </p>
          <ol className="tabs-list">
            {TABS.map(([tab, meaning], i) => (
              <li key={tab}>
                <kbd>{i + 1}</kbd>
                <span>
                  <strong>{tab}.</strong> {meaning}
                </span>
              </li>
            ))}
          </ol>
          <h3 id="keys">Keys</h3>
          <div className="table-wrap">
            <table className="doc-table">
              <thead>
                <tr>
                  <th scope="col">Key</th>
                  <th scope="col">What it does</th>
                </tr>
              </thead>
              <tbody>
                {KEYS.map(([keys, what]) => (
                  <tr key={what}>
                    <td className="key-cell">
                      {keys.map((k) => (
                        <kbd key={k}>{k}</kbd>
                      ))}
                    </td>
                    <td>{what}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        {GROUPS.map((group) => (
          <section key={group.id} className="docs-section" aria-labelledby={group.id}>
            <H2 id={group.id}>{group.title}</H2>
            <p>{group.lead}</p>
            {group.commands.map((c) => (
              <article key={c.name} className="cmd" aria-labelledby={`cmd-${slug(c.name)}`}>
                <h3 id={`cmd-${slug(c.name)}`}>
                  <a href={`#cmd-${slug(c.name)}`}>
                    <code>{c.name}</code>
                  </a>
                </h3>
                <p>{c.what}</p>
                <Shell>{c.example}</Shell>
                {c.options && <Options items={c.options} />}
                {c.note && (
                  <Note>
                    <p>{c.note}</p>
                  </Note>
                )}
              </article>
            ))}
          </section>
        ))}

        <section className="docs-section" aria-labelledby="global">
          <H2 id="global">Options for every command</H2>
          <p>
            These go before or after any command, such as <code>omg --state-dir /tmp/omg-test scan</code>.
          </p>
          <Options items={GLOBAL} />
          <p>
            Commands print JSON, so you can pipe them into tools like jq. Fields are only ever added, never removed or renamed,
            and a size omg does not know is <code>null</code>, never <code>0</code>. Details:{" "}
            <a href={doc("docs/compatibility.md")}>output compatibility</a>.
          </p>
        </section>

        <section className="docs-section" aria-labelledby="categories">
          <H2 id="categories">Categories</H2>
          <p>
            Every finding has a category. Use these names with <code>omg clean --category</code>. Suggested ones are added to
            OMG Trash after a scan; the others are only listed.
          </p>
          <div className="table-wrap">
            <table className="doc-table">
              <thead>
                <tr>
                  <th scope="col">Category</th>
                  <th scope="col">What it is</th>
                  <th scope="col">After a scan</th>
                </tr>
              </thead>
              <tbody>
                {CATEGORIES.map(([name, what, suggested]) => (
                  <tr key={name}>
                    <td>
                      <code>{name}</code>
                    </td>
                    <td>{what}</td>
                    <td className={suggested ? "tag is-on" : "tag"}>{suggested ? "Suggested" : "Listed only"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <section className="docs-section" aria-labelledby="safety">
          <H2 id="safety">What is never deleted</H2>
          <p>These refusals live in the deletion code itself. No flag, setting or catalog rule turns them off.</p>
          <ul className="never">
            {NEVER.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
          <Note tone="warn">
            <p>
              <strong>Delete is permanent.</strong> omg does not move files to the system trash and there is no undo. Read the
              list before you confirm: a cache can be downloaded again, but that takes time.
            </p>
          </Note>
          <h3 id="checks">How a delete is checked</h3>
          <ul className="rules">
            {RULES.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
          <p>
            The full list: <a href={doc("docs/safety.md")}>safety rules</a>.
          </p>
        </section>

        <section className="docs-section" aria-labelledby="files">
          <H2 id="files">Data and uninstall</H2>
          <p>omg keeps its index and settings in one private folder that never leaves your computer.</p>
          <Options
            head="System"
            items={[
              ["macOS", "~/Library/Application Support/omg"],
              ["Linux", "~/.local/state/omg, or $XDG_STATE_HOME/omg"],
              ["Windows", "%LOCALAPPDATA%\\omg"],
            ]}
          />
          <p>
            If omg ever says its index is damaged, move this folder aside and scan again. You lose only the delete log and the
            trend history.
          </p>
          <h3 id="uninstall">Uninstall</h3>
          <p>Turn off anything that runs in the background, then remove the package:</p>
          <Shell>{`omg auto-delete off\nomg schedule disable        # macOS, Windows\nomg service uninstall       # Linux\nnpm uninstall --global oh-my-gigabytes`}</Shell>
          <p>Delete the state folder too if you want the index gone.</p>
        </section>

        <nav className="docs-next" aria-label="Next">
          <Link href="/guides">
            <span>Next</span>
            Guides: free up disk space step by step
          </Link>
        </nav>
      </DocLayout>
    </DocShell>
  );
}
