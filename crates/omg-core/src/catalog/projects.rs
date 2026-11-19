use super::*;

/// What tools write into a clone without it being anyone's work. `build`, `dist` and `coverage` are
/// left out on purpose: those names can also be tracked folders that someone edited.
pub(crate) const CLONE_NOISE: [&str; 8] = [
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".git",
    ".DS_Store",
    "Thumbs.db",
];

/// Timestamps of a checkout and its index can differ by this much without anyone having edited.
pub(crate) const CLONE_SLACK_NS: i128 = 2_000_000_000;

/// Files at the root of a JavaScript monorepo that make its packages' `dist` and `build` output.
pub(crate) const WORKSPACE_FILES: [&str; 5] = [
    "pnpm-workspace.yaml",
    "lerna.json",
    "turbo.json",
    "nx.json",
    "rush.json",
];

/// The folder `CARGO_TARGET_DIR` names, relative to the scan root, when it is absolute.
pub(crate) fn shared_cargo_target(root: &std::path::Path) -> Option<Vec<u8>> {
    let target = absolute_env("CARGO_TARGET_DIR")?;
    crate::rollup::relative_raw(root, &target).filter(|p| !p.is_empty())
}

/// Projects worked on within this many days keep their build output out of suggestions.
pub const PROJECT_ACTIVE_DAYS: i64 = 7;

/// Folder names a build or package tool writes inside a project.
pub(crate) const PROJECT_OUTPUT_NAMES: [&str; 12] = [
    "Carthage",
    ".swiftpm",
    "bin",
    "obj",
    "vendor",
    "deps",
    "out",
    "Library",
    "Temp",
    "Intermediate",
    "Binaries",
    "public",
];

impl Index {
    /// Git clones untouched for `policy::STALE_DAYS` with nothing local to lose: see
    /// `policy::pristine_clone`, plus nothing in the working tree changed after the checkout. The
    /// change time is used because moving or copying a file in keeps its old modification time.
    pub(crate) fn unused_clones(&self, scan: i64, root: &std::path::Path) -> Result<Vec<Vec<u8>>> {
        let mut statement = self.db.prepare(
            "SELECT parent FROM entries WHERE scan_id=? AND kind='directory' AND name=CAST('/.git' AS BLOB) UNION SELECT parent FROM entries WHERE scan_id=? AND kind='directory' AND parent=x'' AND name=CAST('.git' AS BLOB)",
        )?;
        let mut repositories: Vec<Vec<u8>> = statement
            .query_map(params![scan, scan], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        repositories.sort_by_key(|p| p.len());
        let gaps = self.gaps(scan)?;
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let mut changed = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent>=?2 AND parent<?3 AND (parent=?2 OR parent>?4) AND changed_ns>?5",
        )?;
        let mut chosen: Vec<Vec<u8>> = Vec::new();
        for repository in repositories {
            let hidden = repository
                .split(|b| *b == b'/')
                .any(|part| part.first() == Some(&b'.'));
            if repository.is_empty()
                || hidden
                || gaps.covers(&repository)
                || chosen.iter().any(|c| policy::inside(&repository, c, true))
            {
                continue;
            }
            let newest = self.subtree(&repository)?.map_or(0, |t| t.newest);
            if newest <= 0 || newest >= cutoff {
                continue;
            }
            let Some(checkout) = policy::pristine_clone(&root.join(platform::path(&repository)?))
            else {
                continue;
            };
            if !self.matches_checkout(scan, &repository, &checkout)? {
                continue;
            }
            let limit =
                i64::try_from(checkout.written.saturating_add(CLONE_SLACK_NS)).unwrap_or(i64::MAX);
            let mut upper = repository.clone();
            upper.push(b'0');
            let mut prefix = repository.clone();
            prefix.push(b'/');
            let mut rows = changed.query(params![scan, repository, upper, prefix, limit])?;
            let mut edited = false;
            while let Some(row) = rows.next()? {
                let path: Vec<u8> = row.get(0)?;
                let below = &path[repository.len()..];
                let noise = below
                    .split(|b| *b == b'/')
                    .any(|part| CLONE_NOISE.iter().any(|name| name.as_bytes() == part));
                if !noise {
                    edited = true;
                    break;
                }
            }
            if !edited {
                chosen.push(repository);
            }
        }
        Ok(chosen)
    }
    /// Whether the working tree is exactly what Git's index recorded: every tracked file present
    /// with its recorded size and time, and nothing else apart from what tools write
    /// (`CLONE_NOISE`). An untracked or ignored file, such as a `.env`, is local work too.
    pub(crate) fn matches_checkout(
        &self,
        scan: i64,
        repository: &[u8],
        checkout: &policy::Checkout,
    ) -> Result<bool> {
        let mut upper = repository.to_vec();
        upper.push(b'0');
        let mut prefix = repository.to_vec();
        prefix.push(b'/');
        let mut statement = self.db.prepare_cached(
            "SELECT CAST(parent||name AS BLOB),kind,apparent,modified_ns,excluded IS NOT NULL FROM entries WHERE scan_id=?1 AND parent>=?2 AND parent<?3 AND (parent=?2 OR parent>?4)",
        )?;
        let mut rows = statement.query(params![scan, repository, upper, prefix])?;
        let mut seen = 0;
        while let Some(row) = rows.next()? {
            let path: Vec<u8> = row.get(0)?;
            let kind: String = row.get(1)?;
            let Some(relative) = path.strip_prefix(prefix.as_slice()) else {
                return Ok(false);
            };
            if relative == b".git" {
                continue;
            }
            if let Some(tracked) = checkout.tracked.get(relative) {
                let modified: i64 = row.get(3)?;
                // Git itself compares contents for a file modified at or after the index was
                // written ("racily clean"), because its times cannot tell.
                if i128::from(modified) >= checkout.written
                    || !tracked.matches(&kind, row.get::<_, i64>(2)?.max(0) as u64, modified)
                {
                    return Ok(false);
                }
                seen += 1;
                continue;
            }
            let noise = relative.split(|b| *b == b'/').any(|part| {
                part != b".git" && CLONE_NOISE.iter().any(|name| name.as_bytes() == part)
            });
            // Git tracks no folders, but one not looked into could hide anything.
            let unread = kind == "directory" && row.get::<_, bool>(4)?;
            if !noise && (kind != "directory" || unread) {
                return Ok(false);
            }
        }
        Ok(seen == checkout.tracked.len())
    }
    /// How recently the project folder `project` was worked on, split by where the change was.
    pub fn project_activity(&self, scan: i64, project: &[u8]) -> Result<Activity> {
        let mut activity = Activity::default();
        let bump = |slot: &mut Option<i64>, at: i64| {
            if at > 0 {
                *slot = Some(slot.map_or(at, |s| s.max(at)));
            }
        };
        let mut folders = self.db.prepare_cached(
            "SELECT path,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x''",
        )?;
        let rows = folders.query_map(params![scan, project], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows.take(5000) {
            let (path, newest) = row?;
            let name = crate::index::split_path(&path).1;
            let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
            if name == ".git" {
                bump(&mut activity.git, newest);
            } else if policy::REGENERABLE.contains(&name.as_ref())
                || PROJECT_OUTPUT_NAMES.contains(&name.as_ref())
            {
                bump(&mut activity.build, newest);
            } else {
                bump(&mut activity.source, newest);
            }
        }
        let mut files = self.db.prepare_cached(
            "SELECT modified_ns FROM entries WHERE scan_id=? AND parent=? AND kind<>'directory' LIMIT 5000",
        )?;
        for at in files.query_map(params![scan, project], |r| r.get::<_, i64>(0))? {
            bump(&mut activity.source, at? / 1_000_000_000);
        }
        Ok(activity)
    }
    /// Top-level dependency folders (`node_modules` next to `package.json`, `target` next to
    /// `Cargo.toml`, Python virtual environments) outside hidden (tool/application) folders.
    pub(crate) fn stale_dependencies(
        &self,
        scan: i64,
        root: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        self.project_outputs(scan, root, 1 << 20, Some(cutoff))
    }
    /// When a project's output was last worked on. A shared Cargo target folder belongs to no one
    /// project, so only its own newest change counts.
    pub(crate) fn output_activity(
        &self,
        scan: i64,
        root: &std::path::Path,
        path: &[u8],
    ) -> Result<Activity> {
        if shared_cargo_target(root).as_deref() == Some(path) {
            let newest = self.subtree(path)?.map(|t| t.newest).filter(|at| *at > 0);
            return Ok(Activity {
                build: newest,
                ..Activity::default()
            });
        }
        self.project_activity(scan, crate::index::split_path(path).0)
    }
    /// The same folders at least `min_bytes` large, and untouched since `untouched_since` if given.
    pub(crate) fn project_outputs(
        &self,
        scan: i64,
        root: &std::path::Path,
        min_bytes: i64,
        untouched_since: Option<i64>,
    ) -> Result<Vec<Vec<u8>>> {
        let shared_target = shared_cargo_target(root);
        let mut statement = self.db.prepare(
            "SELECT path FROM dir_totals WHERE scan_id=? AND allocated>=? AND path<>x''",
        )?;
        let mut candidates: Vec<Vec<u8>> = statement
            .query_map(params![scan, min_bytes], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        candidates.sort_by_key(|p| p.len());
        let exists = |path: Vec<u8>| -> Result<bool> {
            Ok(self
                .db
                .query_row(
                    "SELECT 1 FROM entries WHERE scan_id=? AND parent=? AND name=?",
                    params![
                        scan,
                        crate::index::split_path(&path).0,
                        crate::index::split_path(&path).1
                    ],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some())
        };
        let mut chosen: Vec<Vec<u8>> = Vec::new();
        for path in candidates {
            let (parent, name) = match path.iter().rposition(|b| *b == b'/') {
                Some(n) => (path[..n].to_vec(), &path[n + 1..]),
                None => (Vec::new(), path.as_slice()),
            };
            let join = |base: &[u8], leaf: &str| {
                let mut out = base.to_vec();
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(leaf.as_bytes());
                out
            };
            let beside = |files: &[&str]| -> Result<bool> {
                for file in files {
                    if exists(join(&parent, file))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            let beside_kind = |extensions: &[&str]| -> Result<bool> {
                let mut statement = self.db.prepare_cached(
                    "SELECT name FROM entries WHERE scan_id=? AND parent=? LIMIT 2000",
                )?;
                let names =
                    statement.query_map(params![scan, parent], |r| r.get::<_, Vec<u8>>(0))?;
                for name in names {
                    let name = name?;
                    if extensions.iter().any(|e| name.ends_with(e.as_bytes())) {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            let (grandparent, parent_name) = crate::index::split_path(&parent);
            let parent_name = parent_name.strip_prefix(b"/").unwrap_or(parent_name);
            let beside_parent = |files: &[&str]| -> Result<bool> {
                for file in files {
                    if exists(join(grandparent, file))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            // A package of a JavaScript monorepo: a workspace file and `.git` a few folders up.
            let workspace_member = || -> Result<bool> {
                let mut above = parent.clone();
                for _ in 0..4 {
                    if above.is_empty() {
                        break;
                    }
                    above = crate::index::split_path(&above).0.to_vec();
                    if !exists(join(&above, ".git"))? {
                        continue;
                    }
                    for file in WORKSPACE_FILES {
                        if exists(join(&above, file))? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            };
            let shared = shared_target.as_deref() == Some(path.as_slice());
            let evidence = match name {
                // Cargo tags every target folder it creates.
                _ if shared => exists(join(&path, "CACHEDIR.TAG"))?,
                b"node_modules" | b".next" | b".nuxt" | b".svelte-kit" | b".turbo"
                | b".parcel-cache" | b".angular" | b".expo" | b".output" | b"storybook-static"
                | b".pnpm-store" | b".nyc_output" | b".vite" => beside(&["package.json"])?,
                // Coverage reports by name alone may be committed; these files are the tools' own.
                b"coverage" => {
                    beside(&["package.json"])?
                        && (exists(join(&path, "lcov.info"))?
                            || exists(join(&path, "coverage-final.json"))?
                            || exists(join(&path, "lcov-report"))?)
                }
                b".vs" => beside_kind(&[".sln", ".slnx"])?,
                b"Build" if parent_name == b"Carthage" => {
                    beside_parent(&["Cartfile", "Cartfile.resolved"])?
                }
                // Xcode recreates the workspace data; registry and mirror settings are the user's.
                b".swiftpm" => {
                    beside(&["Package.swift"])? && !exists(join(&path, "configuration"))?
                }
                b"out" => beside(&[
                    "next.config.js",
                    "next.config.mjs",
                    "next.config.ts",
                    "forge.config.js",
                    "forge.config.ts",
                ])?,
                b".cache" | b"public" => beside(&["gatsby-config.js", "gatsby-config.ts"])?,
                b".docusaurus" => beside(&[
                    "docusaurus.config.js",
                    "docusaurus.config.ts",
                    "docusaurus.config.mjs",
                ])?,
                b".serverless" => beside(&["serverless.yml", "serverless.yaml", "serverless.ts"])?,
                b"bower_components" => beside(&["bower.json"])?,
                b"elm-stuff" => beside(&["elm.json"])?,
                b"vendor" => beside(&["composer.json"])?,
                // Bundler's `--path vendor/bundle`; the rest of `vendor` may be committed code.
                b"bundle" if parent_name == b"vendor" => beside_parent(&["Gemfile"])?,
                b"library" if parent_name == b"renv" => beside_parent(&["renv.lock"])?,
                b"deps" | b".elixir_ls" => beside(&["mix.exs"])?,
                b"bin" | b"obj" => beside_kind(&[".csproj", ".fsproj", ".vbproj"])?,
                b".terraform" => beside_kind(&[".tf"])?,
                b".terragrunt-cache" => beside(&["terragrunt.hcl"])?,
                b"Intermediate" | b"Binaries" | b"DerivedDataCache" => beside_kind(&[".uproject"])?,
                // Unity's own folders, beside the file only Unity writes.
                b"Library" | b"Temp" | b"Obj" => {
                    exists(join(&parent, "ProjectSettings/ProjectVersion.txt"))?
                }
                b".godot" => beside(&["project.godot"])?,
                b".bloop" | b".metals" | b".bsp" => beside(&["build.sbt", "build.sc"])?,
                b".cpcache" => beside(&["deps.edn"])?,
                b".shadow-cljs" => beside(&["shadow-cljs.edn"])?,
                b"_opam" => beside(&["dune-project"])? || beside_kind(&[".opam"])?,
                b".cxx" | b".externalNativeBuild" | b".kotlin" => beside(&[
                    "build.gradle",
                    "build.gradle.kts",
                    "settings.gradle",
                    "settings.gradle.kts",
                ])?,
                b"vcpkg_installed" => beside(&["vcpkg.json"])?,
                b".pytest_cache" | b".mypy_cache" | b".ruff_cache" | b".nox" | b".hypothesis" => {
                    beside(&[
                        "pyproject.toml",
                        "setup.py",
                        "setup.cfg",
                        "requirements.txt",
                        "noxfile.py",
                        "pytest.ini",
                    ])?
                }
                b".direnv" => beside(&[".envrc"])?,
                // Ordinary names: only at the root of a Git project, where the build made them.
                b"dist" | b"build" => {
                    (beside(&["package.json"])? && (beside(&[".git"])? || workspace_member()?))
                        || beside(&[
                            "build.gradle",
                            "build.gradle.kts",
                            "pubspec.yaml",
                            "CMakeLists.txt",
                            "setup.py",
                        ])?
                        || beside_kind(&[".xcodeproj"])?
                }
                b"target" => beside(&["Cargo.toml", "pom.xml", "build.sbt", "project.clj"])?,
                b".gradle" => beside(&[
                    "build.gradle",
                    "build.gradle.kts",
                    "settings.gradle",
                    "settings.gradle.kts",
                ])?,
                b".dart_tool" => beside(&["pubspec.yaml"])?,
                b".tox" => beside(&["tox.ini", "pyproject.toml", "setup.py"])?,
                b"Pods" => beside(&["Podfile"])?,
                b".build" => beside(&["Package.swift"])?,
                b".stack-work" => beside(&["stack.yaml"])?,
                b"dist-newstyle" => beside(&["cabal.project"])?,
                b"_build" => beside(&["mix.exs", "dune-project"])?,
                b"zig-cache" | b".zig-cache" | b"zig-out" => beside(&["build.zig"])?,
                b"cmake-build-debug" | b"cmake-build-release" => beside(&["CMakeLists.txt"])?,
                // Any folder holding a virtual environment's marker is one, whatever its name.
                _ => exists(join(&path, "pyvenv.cfg"))?,
            };
            // Projects live in ordinary folders. Hidden folders hold installed tools (nvm global
            // packages, editor servers, pnpm's own versions), whose dependencies are not stale
            // project output: deleting them would break the tool.
            let hidden = parent
                .split(|b| *b == b'/')
                .any(|part| part.first() == Some(&b'.'));
            if !evidence
                || (hidden && !shared)
                || chosen.iter().any(|c| policy::inside(&path, c, true))
            {
                continue;
            }
            let Some(cutoff) = untouched_since else {
                chosen.push(path);
                continue;
            };
            let owner = if shared { &path } else { &parent };
            let newest = self.subtree(owner)?.map_or(0, |t| t.newest);
            if newest > 0 && newest < cutoff {
                chosen.push(path);
            }
        }
        Ok(chosen)
    }
}
