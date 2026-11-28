export type Block = { h?: string; p?: string[]; code?: string; list?: string[] };
export type Guide = { slug: string; title: string; description: string; summary: string; blocks: Block[] };

export const GUIDES: Guide[] = [
  {
    slug: "find-what-is-using-disk-space",
    title: "Find what is using your disk space from the terminal",
    description:
      "How to see which folders take the most disk space on macOS and Linux with df and du, and how to get the same answer faster with an explained, keyboard-driven view.",
    summary: "df, du and sort, and why a developer's disk fills up with caches.",
    blocks: [
      {
        p: [
          "When the disk is almost full, the first question is where the space went. The terminal can answer it without installing anything, and on a developer machine the answer is usually caches and build folders.",
        ],
      },
      { h: "How full is the disk?", p: ["df shows each volume's size and free space:"], code: "df -h" },
      {
        h: "Which folders are biggest?",
        p: [
          "du adds up a folder's size. Sorting the result puts the biggest last. Run it on your home folder first, then on whichever folder comes out on top, and repeat:",
        ],
        code: "du -sh ~/* ~/.[!.]* 2>/dev/null | sort -h | tail -20",
      },
      {
        p: [
          "Hidden folders matter here: ~/.npm, ~/.cache, ~/.cargo, ~/.gradle and ~/Library on macOS often hold more than everything you can see in Finder. du can take minutes on a big home folder, and it has to start over each time you look somewhere new.",
        ],
      },
      {
        h: "What usually fills a developer's disk",
        list: [
          "Package manager caches: npm, pnpm, Yarn, pip, uv, Cargo, Go, Gradle, Maven, Homebrew",
          "Build output: node_modules, target, .next, dist, virtualenvs, Xcode DerivedData",
          "Old versions of toolchains: nvm, pyenv and rustup keep every version you installed",
          "Docker images and build cache",
          "Simulators, emulators and downloaded ML models",
        ],
      },
      {
        h: "The faster way",
        p: [
          "omg scans once, remembers sizes, and lets you move through folders with the arrow keys. It also recognises the folders above, tells you what each one is and whether it can be recreated, and deletes only what you confirm.",
        ],
        code: "npm install -g oh-my-gigabytes\nomg",
      },
    ],
  },
  {
    slug: "clear-xcode-deriveddata",
    title: "How to clear Xcode DerivedData and free space on a Mac",
    description:
      "Where Xcode DerivedData, iOS DeviceSupport, Archives and simulators live, how much space they take, which are safe to delete and how to clear them.",
    summary: "DerivedData, DeviceSupport, Archives and simulators: what is safe to remove.",
    blocks: [
      {
        p: [
          "Xcode keeps build output, indexes and logs for every project you open in DerivedData. It grows with every project and every Xcode version, and it is easy to find tens of gigabytes there.",
        ],
      },
      {
        h: "See how big it is",
        code: "du -sh ~/Library/Developer/Xcode/DerivedData\ndu -sh ~/Library/Developer/Xcode/*",
      },
      {
        h: "Is it safe to delete?",
        p: [
          "Yes. DerivedData is recreated the next time you build. The only cost is time: the first build of each project and its index are slower afterwards. Quit Xcode first so it is not writing there while you delete.",
        ],
        code: "rm -rf ~/Library/Developer/Xcode/DerivedData/*",
      },
      {
        h: "Other Xcode folders",
        list: [
          "iOS DeviceSupport (~/Library/Developer/Xcode/iOS DeviceSupport): symbol files copied from each device and iOS version you connected. Old versions can go; Xcode copies them again when needed.",
          "Archives (~/Library/Developer/Xcode/Archives): builds you archived for release. They hold the debug symbols for crash reports, so keep the ones for versions people still use.",
          "Simulators: xcrun simctl delete unavailable removes simulators for runtimes that are no longer installed.",
        ],
        code: "xcrun simctl delete unavailable",
      },
      {
        h: "With omg",
        p: [
          "omg lists DerivedData as build output and the DeviceSupport and simulator caches as caches, with their sizes, and adds them to OMG Trash for you to review. Archives and simulator devices are listed but never suggested, because only you know whether you still need them.",
        ],
        code: "omg",
      },
    ],
  },
  {
    slug: "delete-node-modules",
    title: "How to find and delete old node_modules folders",
    description:
      "Find every node_modules folder in your projects, see how much space each takes, delete the ones in projects you no longer work on, and reinstall when you need them.",
    summary: "Find them all, see their size, and remove the ones from old projects.",
    blocks: [
      {
        p: [
          "Every JavaScript project keeps its own node_modules, often several hundred megabytes. After a year of side projects and cloned repositories they can add up to many gigabytes.",
        ],
      },
      {
        h: "Find them and their sizes",
        p: ["-prune stops find from looking inside each node_modules, which would be slow and list nested ones twice:"],
        code: "find ~/code -name node_modules -type d -prune -exec du -sh {} + | sort -h",
      },
      {
        h: "Delete them",
        p: [
          "node_modules is recreated from package.json and the lock file, so it is safe to delete in a project you are not working on. Check the list from the command above first; the next command deletes without asking.",
        ],
        code: "find ~/code -name node_modules -type d -prune -exec rm -rf {} +",
      },
      { h: "Reinstall when you come back", p: ["In the project folder:"], code: "npm ci        # or: pnpm install, yarn install" },
      {
        h: "With omg",
        p: [
          "omg finds node_modules, .next, .turbo, dist and other build folders next to the project file that made them, and shows when each project was last worked on. A project you touched in the last 7 days is never suggested, and you confirm the exact list before anything is deleted.",
        ],
        code: "omg clean --category build_artifacts --older-than 30d",
      },
    ],
  },
  {
    slug: "docker-disk-space",
    title: "How to free disk space used by Docker",
    description:
      "See how much space Docker images, containers, volumes and build cache use, and remove the unused parts without losing volumes or running containers.",
    summary: "docker system df, build cache, dangling images, and what not to prune.",
    blocks: [
      {
        p: [
          "Docker keeps every image you pulled or built, every layer of build cache and every stopped container until you remove them. On a machine that builds images often, the build cache alone can reach tens of gigabytes.",
        ],
      },
      { h: "See what Docker uses", code: "docker system df\ndocker system df -v" },
      {
        h: "Remove the safe parts",
        p: [
          "Build cache and dangling images (untagged layers left over from rebuilds) are recreated when needed:",
        ],
        code: "docker builder prune\ndocker image prune",
      },
      {
        h: "Be careful with",
        list: [
          "docker system prune also removes all stopped containers and unused networks.",
          "docker image prune -a removes every image no container uses, including ones you will need to pull again.",
          "Volumes hold databases and other data. They are removed only when you ask with --volumes or docker volume prune, and that data is gone for good.",
        ],
      },
      {
        h: "With omg",
        p: [
          "omg docker clean asks your local Docker engine for dangling images older than 7 days and build cache unused for 30 days, shows the exact list and a fingerprint, and lets Docker remove exactly that when you confirm. It never removes volumes or running containers.",
        ],
        code: "omg docker clean\nomg docker clean --confirm <fingerprint>",
      },
    ],
  },
  {
    slug: "clear-package-manager-caches",
    title: "Clear npm, pip, Cargo, Go, Gradle and Homebrew caches",
    description:
      "Where package manager caches live on macOS, Linux and Windows, how big they get, and the command each tool provides to clean its own cache.",
    summary: "The cleanup command for each package manager, and where the caches live.",
    blocks: [
      {
        p: [
          "Package managers keep a copy of everything they download so the next install is faster. The caches are safe to clear: the only cost is downloading again. Each tool has its own command, which is the cleanest way to do it.",
        ],
      },
      {
        h: "JavaScript",
        code: "npm cache clean --force      # ~/.npm/_cacache\npnpm store prune             # removes packages no project uses\nyarn cache clean",
      },
      {
        h: "Python",
        code: "pip cache purge              # macOS ~/Library/Caches/pip, Linux ~/.cache/pip\nuv cache clean",
      },
      {
        h: "Go and Rust",
        code: "go clean -cache              # build cache\ngo clean -modcache           # downloaded modules\ndu -sh ~/.cargo/registry     # Cargo has no clean command; this folder is re-downloaded when needed",
      },
      {
        h: "Java and Homebrew",
        code: "du -sh ~/.gradle/caches ~/.m2/repository\nbrew cleanup                 # old versions and downloads",
      },
      {
        h: "Checking sizes first",
        p: ["You usually do not need to clear every cache. See which ones are big:"],
        code: "du -sh ~/.npm ~/.cache/* ~/.cargo/registry ~/.gradle/caches ~/Library/Caches/* 2>/dev/null | sort -h",
      },
      {
        h: "With omg",
        p: [
          "omg recognises the caches of dozens of tools on macOS, Linux and Windows, shows their sizes in one list and adds them to OMG Trash for review. To clear them all from a script:",
        ],
        code: "omg clean --category package_caches\nomg clean --category package_caches --confirm <fingerprint>",
      },
    ],
  },
];
