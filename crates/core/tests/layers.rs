//! The core's modules keep to their layers. Each top-level module of `crates/core/src` uses only the modules
//! its line in `LAYERS` lists, and those come earlier in the table, so the modules take an order with no
//! cycle. `ui/eslint/layers.ts` does the same for the UI. Clippy can't see a `crate::` path, so this test
//! reads the source as text.
//!
//! The modules of `FUNCTIONAL_CORE` take facts and return values, so their lines may list no module of
//! `SHELL`, which opens connections, runs git or sends messages.
//!
//! Text is enough while the core keeps its paths plain: no grouped `crate::{...}` import, no `pub use` in
//! `lib.rs`, and no `super::` path that leaves its top-level module. The test fails on each of those, so none
//! can hide a dependency. Test code doesn't count: it lives in `tests.rs`, `testing.rs` and `tests/` folders,
//! and `crates/app/tests/rules.rs` keeps it there.
//!
//! We read arch-lint and easement and adopted neither (#88). Look at easement again first, since it reads
//! the compiler's view of the code, once both of these hold:
//!
//! - Its CI tests Windows and its releases have slowed down. Pin an exact version, not `latest`.
//! - This test has missed a real dependency, or we want rules it can't check, such as indirect chains
//!   between modules.
//!
//! Look at arch-lint again only if we want many custom rules inside the tests that Clippy can't express, and
//! its path matching limits (globs, `pub use`, macros) don't affect those rules.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tsugiki_test_git::without_visibility;

/// A top-level module of `crates/core/src`, the modules it may use, and why.
#[derive(Clone, Copy)]
struct Layer {
    module: &'static str,
    uses: &'static [&'static str],
    why: &'static str,
}

/// Lowest first. A module may use only modules above it, apart from the edges in `CYCLES`.
const LAYERS: &[Layer] = &[
    Layer {
        module: "build",
        uses: &[],
        why: "Keeps a development build apart from an installed one, a fact the modules above read.",
    },
    Layer {
        module: "error",
        uses: &[],
        why: "The errors the UI acts on. Every module returns them, so this one uses none.",
    },
    Layer {
        module: "highlight",
        uses: &[],
        why: "Highlights any text with tree-sitter, and knows nothing of PRs or diffs.",
    },
    Layer {
        module: "latest",
        uses: &[],
        why: "Shares a value between threads, and knows no domain type.",
    },
    Layer {
        module: "deadline",
        uses: &[],
        why: "Reads a channel until a deadline, and knows no domain type.",
    },
    Layer {
        module: "version",
        uses: &[],
        why: "Names the version and the commit a build came from, which the log and the HUD write down.",
    },
    Layer {
        module: "logfile",
        uses: &["version"],
        why: "Takes the log lines from anywhere in the core, so it can't use what logs to it.",
    },
    Layer {
        module: "login",
        uses: &[],
        why: "Names a GitHub account, which `github` reads and the Conventions and Activity compare.",
    },
    Layer {
        module: "measurement",
        uses: &["version"],
        why: "Records the performance HUD, which times the window and not the core's data.",
    },
    Layer {
        module: "theme",
        uses: &["error"],
        why: "Turns theme files into the window's colours, and knows nothing of GitHub or the settings.",
    },
    Layer {
        module: "misses",
        uses: &[],
        why: "Logs the budgets the window misses, from its timings alone.",
    },
    Layer {
        module: "keys",
        uses: &[],
        why: "Lists each action the user can bind, with its default key and where it works, for the window and \
              the settings.",
    },
    Layer {
        module: "launch",
        uses: &["build"],
        why: "Reads a link for this build, so a development build leaves an installed one's links alone.",
    },
    Layer {
        module: "token",
        uses: &["error", "logfile"],
        why: "Reads the token from gh, and logs why gh couldn't give it.",
    },
    Layer {
        module: "repository",
        uses: &["error"],
        why: "Names a Repository, reads the numbers of its Issues, and says how big it is and what the user \
              may do in it.",
    },
    Layer {
        module: "read_only",
        uses: &["repository", "token"],
        why: "Judges from the token's scopes and a Repository's visibility whether Tsugiki can write there, \
              and holds the Repository's state that each write control reads.",
    },
    Layer {
        module: "mirror_source",
        uses: &["repository"],
        why: "Names where each Repository's mirror gets its objects, which `settings` keeps and `mirror` \
              clones by, so `settings` needn't use `mirror`.",
    },
    Layer {
        module: "mirror_queue",
        uses: &["repository"],
        why: "Picks the Mirror worker's next job from what it holds queued in each Repository, so a test of \
              the order needs no thread and no git.",
    },
    Layer {
        module: "web",
        uses: &["error", "repository"],
        why: "Checks an address before the browser opens it, and builds a Repository's addresses.",
    },
    Layer {
        module: "releases",
        uses: &["web"],
        why: "Asks GitHub Releases for a newer Tsugiki, and links to its release page.",
    },
    Layer {
        module: "disk",
        uses: &[],
        why: "Measures how much of the disk a folder takes, and knows nothing of what the folder holds.",
    },
    Layer {
        module: "folders",
        uses: &["build", "error", "repository"],
        why: "Finds where this build keeps its files, a mirror folder for each Repository among them.",
    },
    Layer {
        module: "filters",
        uses: &[],
        why: "Holds a Filter as `settings.toml` writes it, and the Filters a new Repository starts with.",
    },
    Layer {
        module: "facts",
        uses: &["filters", "login", "repository"],
        why: "Holds the facts `work` judges: what `github` parses, the patch-ids the Mirror worker reads, \
              and the bundles the store fills. They belong to neither the client nor the store, so a module \
              that judges facts needs neither. A bundle names each Filter's results by its query, and an \
              Issue's event names another Repository.",
    },
    Layer {
        module: "conventions",
        uses: &["login", "repository"],
        why: "Picks a Repository's Conventions by its name. They name bot accounts by `Login`.",
    },
    Layer {
        module: "repository_settings",
        uses: &["conventions", "filters", "read_only", "repository"],
        why: "Holds a Repository's Conventions, Filters and the user's choice of Read-only, and starts them \
              with what adding it gives.",
    },
    Layer {
        module: "github",
        uses: &[
            "conventions",
            "error",
            "facts",
            "filters",
            "login",
            "read_only",
            "repository",
            "token",
        ],
        why: "Sends the token with each request, runs a Filter's search, parses what GitHub answers into \
              `facts` through the Repository's Conventions, and sends a write only for a writable \
              Repository.",
    },
    Layer {
        module: "patch_ids",
        uses: &["facts"],
        why: "Compares the per-file patch-ids of two commits for the Stale review check, which the store \
              keeps by commit and the Mirror worker reads.",
    },
    Layer {
        module: "status",
        uses: &["error", "github", "latest", "login", "repository"],
        why: "Announces app-wide status, which names the signed-in `Login` and the Repositories it is about, \
              and the hour's points that `github` counts. Another thread reads a copy of whether GitHub \
              can be asked through `latest`.",
    },
    Layer {
        module: "links",
        uses: &[
            "conventions",
            "facts",
            "login",
            "repository",
            "repository_settings",
        ],
        why: "Works out the Issues a PR names and a Backport's Origin PR from its facts, through the \
              Repository's Conventions.",
    },
    Layer {
        module: "store",
        uses: &[
            "conventions",
            "error",
            "facts",
            "filters",
            "github",
            "links",
            "login",
            "read_only",
            "repository",
            "token",
        ],
        why: "Saves the facts `github` parses and the Mirror worker reads, in the types of `facts`, so no \
              second copy of each type drifts from the first. A refresh's ETags come from `github`. It keeps \
              the numbers each PR names under each Conventions, which `links` works out as the PR is saved, \
              and the Read-only marks with the kind and expiry of the token they were judged for.",
    },
    Layer {
        module: "activity",
        uses: &["conventions", "facts", "links", "login"],
        why: "Works out New activity from the facts the store keeps, and counts a Backport through `links`.",
    },
    Layer {
        module: "work",
        uses: &[
            "activity",
            "conventions",
            "facts",
            "filters",
            "links",
            "login",
            "patch_ids",
            "repository_settings",
        ],
        why: "Judges each Issue and PR from its facts: its place on the Workbench, why it Needs attention \
              through `activity`, and the user's Review through `patch_ids`. It works out Reach through \
              `links`, and from Reach the Mirror's work. `sync` and `session` act on it without the screens.",
    },
    Layer {
        module: "diff",
        uses: &["error", "facts", "github", "highlight", "repository", "web"],
        why: "Reads a PR's diff and each file's patch-id from its mirror, highlights the diff, and links its \
              files to GitHub.",
    },
    Layer {
        module: "mirror",
        uses: &[
            "diff",
            "disk",
            "error",
            "facts",
            "folders",
            "github",
            "latest",
            "mirror_queue",
            "mirror_source",
            "patch_ids",
            "repository",
            "status",
        ],
        why: "Clones and fetches PR heads into this build's folders, and says how on the status board. It \
              asks a Repository's size before a clone through the session, which holds the token, and says \
              what the disk lacks in the megabytes `disk` counts in. It reads \
              the patch-ids of the commits it keeps through `diff`, and runs the jobs in the order \
              `mirror_queue` picks. Its requests read a copy of the clones that wait for an answer.",
    },
    Layer {
        module: "definitions",
        uses: &[
            "deadline",
            "diff",
            "error",
            "facts",
            "folders",
            "github",
            "highlight",
            "latest",
            "mirror",
            "repository",
        ],
        why: "Asks a language server where a name is defined, in the Lookup checkout the Mirror worker keeps, \
              with the files the PR's diff changes open at its head, read from the Mirror through `diff`. \
              clangd's compile database goes in the servers' folder `folders` names beside the checkout, and \
              how far its index has got goes through `latest`. A hit's code gets its `highlight` spans from \
              the highlighter in `diff`.",
    },
    Layer {
        module: "search",
        uses: &["error", "github", "latest", "mirror", "repository"],
        why: "Searches the open PR's code as text, in the Lookup checkout `mirror` keeps and with the git \
              `github` runs, and shares what it finds through `latest`.",
    },
    Layer {
        module: "settings",
        uses: &[
            "error",
            "filters",
            "folders",
            "keys",
            "latest",
            "mirror_source",
            "repository",
            "repository_settings",
            "theme",
        ],
        why: "Reads and writes the Repositories, their Filters' switches, the mirror sources and the theme \
              names, reads the remapped keys, and shares them through `latest`.",
    },
    Layer {
        module: "workbench",
        uses: &[
            "conventions",
            "diff",
            "error",
            "facts",
            "filters",
            "folders",
            "highlight",
            "links",
            "login",
            "repository",
            "repository_settings",
            "settings",
            "store",
            "work",
        ],
        why: "Groups what `work` judges into the Workbench's items and builds the view models, which show \
              what each module below works out. A read sends nothing: the screen's call in `calls` asks \
              `fetching` for what it found missing.",
    },
    Layer {
        module: "fetching",
        uses: &[
            "error",
            "facts",
            "filters",
            "login",
            "mirror",
            "mirror_source",
            "patch_ids",
            "repository",
            "repository_settings",
            "status",
            "work",
            "workbench",
        ],
        why: "Takes the user's demands and the rules' wants, and hands each to the queue that owns it: the \
              Mirror worker's through `mirror`, and the session's, whose messages it defines so the session \
              above can take them. A Mark Seen names what `work` marks, and Mute author the `login` it \
              mutes. A PR whose Conversation read failed keeps the `status` failure, for its Files tab. An \
              action carries the `Asked`, the number and the `workbench` view the core answers with. A saved \
              Filter's first search carries the `filters` query it searches.",
    },
    Layer {
        module: "sync",
        uses: &[
            "conventions",
            "error",
            "facts",
            "filters",
            "github",
            "latest",
            "links",
            "login",
            "repository",
            "repository_settings",
            "settings",
            "status",
            "store",
            "token",
            "work",
        ],
        why: "Asks GitHub for facts and saves them, then tells the Workbench and the status what changed. \
              `work` says which PRs it reads ahead, under each Repository's settings, which it reads from \
              the view `settings` shares through `latest`.",
    },
    Layer {
        module: "session",
        uses: &[
            "error",
            "facts",
            "fetching",
            "filters",
            "folders",
            "github",
            "latest",
            "login",
            "mirror",
            "patch_ids",
            "read_only",
            "repository",
            "repository_settings",
            "settings",
            "status",
            "store",
            "sync",
            "token",
            "work",
            "workbench",
        ],
        why: "Runs the session thread, which owns the store and the syncs and takes the messages `fetching` \
              sends it. It hands the Mirror worker the heads and commits `work` picks through `fetching`, \
              which it builds on the requests `mirror` gives. Mute author asks `workbench` which items of \
              the `login` it mutes the window lists, to mark them seen. Each Repository's settings come \
              from the view `settings` shares through `latest`. The GitHub thread has `read_only` judge \
              from the token's scopes which Repositories it can't write to, and queues a saved Filter's \
              first search with its `filters` query.",
    },
    Layer {
        module: "calls",
        uses: &[
            "definitions",
            "diff",
            "disk",
            "error",
            "facts",
            "fetching",
            "filters",
            "folders",
            "highlight",
            "latest",
            "login",
            "mirror",
            "mirror_source",
            "read_only",
            "repository",
            "repository_settings",
            "session",
            "settings",
            "status",
            "store",
            "work",
            "workbench",
        ],
        why: "The handle the app calls the core through, made from the `session`'s queues and its `store` \
              reader. A Repository's state comes from `read_only`, with its mark from the `store` and the \
              connection from the `status` copy. Each screen's call reads through a view model in \
              `workbench`, queues through \
              `fetching` what the read found missing, and holds the job in an answer that waits for its \
              data, or names the `status` failure of the read that would have brought it. Only a \
              Repository `settings` lists gets a clone, read through `latest` when the read finds no \
              mirror. The diff on show opens its files on the language servers through `definitions`, and \
              its rows and their `highlight` come from `diff`. Each of the user's actions goes through \
              `fetching`: a Mark Seen names what `work` marks, Mute author the `login` it mutes, and a \
              clone the `mirror_source` it takes its objects from. Saving a search as a Filter writes \
              the `filters` it names for the Repository into `settings`.",
    },
];

/// An edge that closes a cycle, so it may point down `LAYERS`.
struct Cycle {
    module: &'static str,
    uses: &'static str,
    why: &'static str,
}

/// The edges that close a cycle. None do: a new one needs its reason here, and an issue to break it.
const CYCLES: &[Cycle] = &[];

/// The functional core: these modules take facts and return values.
const FUNCTIONAL_CORE: &[&str] = &[
    "activity",
    "facts",
    "links",
    "mirror_queue",
    "read_only",
    "work",
];

/// The imperative shell, which the functional core may not use: the database, `github`'s client, the
/// Mirrors, the values threads share, the waits on their messages, the requests for data, and the threads
/// that sync.
const SHELL: &[&str] = &[
    "deadline", "fetching", "github", "latest", "mirror", "session", "store", "sync",
];

/// What a group of modules keeps out of, and the words a broken rule says.
struct KeptOut {
    /// The group, as a broken rule names it.
    group: &'static str,
    modules: &'static [&'static str],
    /// The modules the group may not use.
    of: &'static [&'static str],
    /// Why, after "but".
    why: &'static str,
}

/// The functional core keeps out of the shell.
const CORE: KeptOut = KeptOut {
    group: "the functional core",
    modules: FUNCTIONAL_CORE,
    of: SHELL,
    why: "the functional core keeps out of the shell",
};

/// A view model's `read` sends nothing (ADR 0022): the screen's call asks for what the read found missing
/// (ADR 0023).
const VIEW_MODELS: KeptOut = KeptOut {
    group: "the view models",
    modules: &["workbench"],
    of: &["fetching", "mirror", "session", "sync"],
    why: "a view model's read sends nothing",
};

/// A file of the core: its path below `crates/core/src` with `/` between folders, and its text.
struct Source {
    path: String,
    text: String,
}

/// The scan's findings: each use of a module by another, at the first place that shows it, and the places
/// whose paths could hide one.
#[derive(Default)]
struct Scan {
    uses: BTreeMap<(String, String), String>,
    hidden: Vec<String>,
}

/// Whether the file at `path` holds only test code.
fn is_test(path: &str) -> bool {
    path.split('/')
        .any(|part| matches!(part, "tests" | "testing" | "tests.rs" | "testing.rs"))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The modules `lib.rs` declares, apart from its test modules.
fn declared_modules(lib: &str) -> Vec<&str> {
    let names = lib.lines().filter_map(|line| {
        let declared = without_visibility(line.trim()).strip_prefix("mod ")?;
        let name = declared.trim_end_matches(|c: char| !is_ident(c));
        declared
            .trim_start_matches(|c: char| is_ident(c))
            .trim()
            .chars()
            .next()
            .is_some_and(|c| matches!(c, ';' | '{'))
            .then_some(name)
    });
    names
        .filter(|name| !matches!(*name, "tests" | "testing"))
        .collect()
}

/// Each line of `text`, 1-based, without the comment at its end. A doc link isn't a use.
fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let lines = text.lines().enumerate();
    lines.map(|(index, line)| (index + 1, line.split("//").next().unwrap_or_default()))
}

/// The text after each `prefix` in `code` that starts a path.
fn paths_after<'a>(code: &'a str, prefix: &str) -> impl Iterator<Item = &'a str> {
    code.match_indices(prefix).filter_map(move |(index, _)| {
        let (before, after) = code.split_at(index);
        let inside_a_path = before
            .chars()
            .next_back()
            .is_some_and(|c| is_ident(c) || c == ':');
        if inside_a_path {
            None
        } else {
            after.strip_prefix(prefix)
        }
    })
}

fn scan(sources: &[Source]) -> Scan {
    let mut found = Scan::default();
    for source in sources.iter().filter(|source| !is_test(&source.path)) {
        if source.path == "lib.rs" {
            found.hidden.extend(reexports(&source.text));
        } else {
            crate_paths(source, &mut found);
            super_paths(source, &mut found);
        }
    }
    found
}

/// Each `pub use` in `lib.rs`.
fn reexports(lib: &str) -> impl Iterator<Item = String> + '_ {
    let reexport = |code: &str| code.trim().starts_with("pub") && code.contains(" use ");
    let lines = code_lines(lib).filter(move |(_, code)| reexport(code));
    lines.map(|(line, _)| {
        format!("lib.rs:{line}: `pub use` gives an item a second path, which skips the module that owns it")
    })
}

/// The top-level module the file at `path` belongs to.
fn module_of(path: &str) -> &str {
    path.split('/')
        .next()
        .unwrap_or_default()
        .trim_end_matches(".rs")
}

fn crate_paths(Source { path, text }: &Source, found: &mut Scan) {
    let module = module_of(path);
    for (line, code) in code_lines(text) {
        for after in paths_after(code, "crate::") {
            let at = format!("{path}:{line}");
            let target: String = after.chars().take_while(|c| is_ident(*c)).collect();
            if after.starts_with('{') {
                found.hidden.push(format!(
                    "{at}: a grouped `crate::{{...}}` import hides the modules it uses"
                ));
            } else if target.is_empty() {
                found
                    .hidden
                    .push(format!("{at}: a `crate::` path this test can't read"));
            } else if target != module && target != "testing" {
                // The first place a module uses another is the one a failure names.
                let used = (module.to_owned(), target);
                if !found.uses.contains_key(&used) {
                    found.uses.extend([(used, at)]);
                }
            }
        }
    }
}

/// Finds each `super::` path that climbs to the crate root: as many steps as the file sits deep. An inline
/// `mod name { ... }` counts as no deeper, so a `super::` in one can fail the test; move it to a file.
fn super_paths(Source { path, text }: &Source, found: &mut Scan) {
    let module = module_of(path);
    let depth = path.split('/').count() - usize::from(path.ends_with("/mod.rs"));
    for (line, code) in code_lines(text) {
        for mut rest in paths_after(code, "super::") {
            let mut steps = 1;
            while let Some(next) = rest.strip_prefix("super::") {
                steps += 1;
                rest = next;
            }
            if steps >= depth {
                found
                    .hidden
                    .push(format!("{path}:{line}: `super::` leaves `{module}`"));
            }
        }
    }
}

/// The rules: `LAYERS` and `CYCLES`, or a sample's own.
struct Table<'a> {
    layers: &'a [Layer],
    cycles: &'a [Cycle],
}

impl Table<'_> {
    fn position(&self, module: &str) -> Option<usize> {
        self.layers.iter().position(|layer| layer.module == module)
    }

    fn allows(&self, module: &str, target: &str) -> bool {
        let listed = |layer: &&Layer| layer.module == module && layer.uses.contains(&target);
        self.layers.iter().any(|layer| listed(&layer))
    }

    /// Whether `target` sits above `module` in the table.
    fn below(&self, module: &str, target: &str) -> bool {
        self.position(target)
            .is_some_and(|above| Some(above) < self.position(module))
    }

    fn closes_a_cycle(&self, module: &str, target: &str) -> bool {
        let listed = |cycle: &&Cycle| (cycle.module, cycle.uses) == (module, target);
        self.cycles.iter().any(|cycle| listed(&cycle))
    }

    /// Each broken rule, in words that name the file and the modules.
    fn problems(&self, modules: &[&str], scan: &Scan) -> Vec<String> {
        let mut found = scan.hidden.clone();
        let without_a_line = modules
            .iter()
            .filter(|module| self.position(module).is_none());
        found.extend(without_a_line.map(|module| format!("`{module}` has no line in LAYERS")));
        for ((module, target), at) in &scan.uses {
            if !self.allows(module, target) {
                found.push(format!(
                    "{at}: `{module}` uses `{target}`, which its line in LAYERS doesn't list"
                ));
            }
        }
        for (index, layer) in self.layers.iter().enumerate() {
            if self.position(layer.module) != Some(index) {
                found.push(format!("`{}` has two lines in LAYERS", layer.module));
            }
            found.extend(self.line_problems(layer, modules, scan));
        }
        found.extend(self.cycle_problems());
        found
    }

    fn line_problems(&self, layer: &Layer, modules: &[&str], scan: &Scan) -> Vec<String> {
        let module = layer.module;
        let mut found = Vec::new();
        if !modules.contains(&module) {
            found.push(format!(
                "LAYERS has a line for `{module}`, which lib.rs doesn't declare"
            ));
        }
        if layer.why.is_empty() {
            found.push(format!("`{module}`'s line in LAYERS gives no reason"));
        }
        for target in layer.uses {
            let used = (module.to_owned(), (*target).to_owned());
            if !scan.uses.contains_key(&used) {
                found.push(format!(
                    "`{module}` may use `{target}`, but nothing in it does any more"
                ));
            }
            if self.position(target).is_none() {
                found.push(format!(
                    "`{module}` may use `{target}`, which has no line in LAYERS"
                ));
            } else if !self.below(module, target) && !self.closes_a_cycle(module, target) {
                found.push(format!(
                    "`{module}` may use `{target}`, which comes after it in LAYERS, so the two can form \
                     a cycle"
                ));
            }
        }
        found
    }

    fn cycle_problems(&self) -> Vec<String> {
        let mut found = Vec::new();
        for Cycle { module, uses, why } in self.cycles {
            if !self.allows(module, uses) {
                found.push(format!(
                    "CYCLES lists `{module}` using `{uses}`, which LAYERS doesn't"
                ));
            } else if self.below(module, uses) {
                found.push(format!(
                    "CYCLES lists `{module}` using `{uses}`, which no longer points down LAYERS"
                ));
            }
            if why.is_empty() {
                found.push(format!(
                    "CYCLES gives no reason for `{module}` using `{uses}`"
                ));
            }
        }
        found
    }
}

#[expect(
    clippy::expect_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn sources(root: &Path) -> Vec<Source> {
    let read = |path: PathBuf| {
        let relative = path.strip_prefix(root).expect("a path below the root");
        let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
        let text = fs::read_to_string(&path).expect("read a source file");
        Source {
            path: parts.join("/"),
            text,
        }
    };
    tsugiki_test_git::rust_sources(root)
        .into_iter()
        .map(read)
        .collect()
}

/// Each module of `rule`'s group whose line in `layers` lets it use a module it keeps out of, and each with
/// no line. Since a module may use only what its line lists, none of the group can then use them.
fn kept_out_problems(layers: &[Layer], rule: &KeptOut) -> Vec<String> {
    let mut found = Vec::new();
    for module in rule.modules {
        let Some(layer) = layers.iter().find(|layer| layer.module == *module) else {
            found.push(format!(
                "`{module}` is in {}, but has no line in LAYERS",
                rule.group
            ));
            continue;
        };
        let used = layer.uses.iter().filter(|target| rule.of.contains(target));
        found.extend(used.map(|target| format!("`{module}` may use `{target}`, but {}", rule.why)));
    }
    found
}

fn source(path: &str, text: &str) -> Source {
    Source {
        path: path.to_owned(),
        text: text.to_owned(),
    }
}

const SAMPLE: &[Layer] = &[
    Layer {
        module: "error",
        uses: &[],
        why: "Below everything.",
    },
    Layer {
        module: "github",
        uses: &["error"],
        why: "Fails with an `Error`.",
    },
];

/// The problems a sample table finds in sample sources.
fn check(layers: &[Layer], cycles: &[Cycle], modules: &[&str], sources: &[Source]) -> Vec<String> {
    Table { layers, cycles }.problems(modules, &scan(sources))
}

#[test]
fn a_use_the_table_doesnt_list_is_found() {
    let sources = [
        source("error.rs", ""),
        source("github/mod.rs", "use crate::error::Error;\n"),
        source("github/live.rs", "use std::fs;\nuse crate::store::Store;\n"),
    ];
    let store = Layer {
        module: "store",
        uses: &[],
        why: "Saves facts.",
    };
    assert_eq!(
        check(
            &[SAMPLE[0], SAMPLE[1], store],
            &[],
            &["error", "github", "store"],
            &sources
        ),
        ["github/live.rs:2: `github` uses `store`, which its line in LAYERS doesn't list"]
    );
}

#[test]
fn a_listed_use_nothing_makes_any_more_is_found() {
    let sources = [source(
        "github.rs",
        "/// Fails with [`crate::error::Error`].\nfn fetch() {}\n",
    )];
    assert_eq!(
        check(SAMPLE, &[], &["error", "github"], &sources),
        ["`github` may use `error`, but nothing in it does any more"]
    );
}

#[test]
fn a_line_for_a_module_that_isnt_there_is_found() {
    let sources = [source("github.rs", "use crate::error::Error;\n")];
    let web = Layer {
        module: "github",
        uses: &["error", "web"],
        why: "Fails with an `Error` and links to the web.",
    };
    assert_eq!(
        check(&[SAMPLE[0], web], &[], &["github"], &sources),
        [
            "LAYERS has a line for `error`, which lib.rs doesn't declare",
            "`github` may use `web`, but nothing in it does any more",
            "`github` may use `web`, which has no line in LAYERS",
        ]
    );
}

#[test]
fn a_module_without_a_line_or_with_two_is_found() {
    let lib = "pub mod error;\npub mod github;\npub(crate) mod web;\npub mod inline {\n}\n\
               // mod gone;\npub mod unfinished\n\
               #[cfg(test)]\npub(crate) mod testing;\n#[cfg(test)]\nmod tests;\n";
    let modules = declared_modules(lib);
    assert_eq!(modules, ["error", "github", "web", "inline"]);
    let sources = [source("github.rs", "use crate::error::Error;\n")];
    assert_eq!(
        check(&[SAMPLE[0], SAMPLE[1], SAMPLE[0]], &[], &modules, &sources),
        [
            "`web` has no line in LAYERS",
            "`inline` has no line in LAYERS",
            "`error` has two lines in LAYERS",
        ]
    );
}

#[test]
fn a_path_that_could_hide_a_use_is_found() {
    let sources = [
        source(
            "lib.rs",
            "pub mod github;\npub use github::GitHub;\npub(crate) use github::Login;\n// pub use old;\n",
        ),
        source(
            "github.rs",
            "use crate::{error::Error};\nuse super::web;\nlet path = crate::\n",
        ),
        source(
            "github/live.rs",
            "use super::GitHub;\nuse super::super::web;\n",
        ),
        source("github/checks/mod.rs", "use super::GitHub;\n"),
    ];
    assert_eq!(
        scan(&sources).hidden,
        [
            "lib.rs:2: `pub use` gives an item a second path, which skips the module that owns it",
            "lib.rs:3: `pub use` gives an item a second path, which skips the module that owns it",
            "github.rs:1: a grouped `crate::{...}` import hides the modules it uses",
            "github.rs:3: a `crate::` path this test can't read",
            "github.rs:2: `super::` leaves `github`",
            "github/live.rs:2: `super::` leaves `github`",
        ]
    );
}

#[test]
fn a_use_that_points_down_the_table_needs_a_line_in_cycles() {
    let error = Layer {
        module: "error",
        uses: &["github"],
        why: "Names the account that failed.",
    };
    let layers = &[error, SAMPLE[1]];
    let sources = [
        source("error.rs", "use crate::github::Login;\n"),
        source("github.rs", "use crate::error::Error;\n"),
    ];
    let modules = ["error", "github"];
    assert_eq!(
        check(layers, &[], &modules, &sources),
        ["`error` may use `github`, which comes after it in LAYERS, so the two can form a cycle"]
    );
    let cycle = |module, uses, why| Cycle { module, uses, why };
    let cycles = [cycle("error", "github", "Until #1.")];
    assert_eq!(check(layers, &cycles, &modules, &sources), [""; 0]);
    let stale = [
        cycle("github", "web", "Until #2."),
        cycle("github", "error", "Until #3."),
        cycle("error", "github", ""),
    ];
    assert_eq!(
        check(layers, &stale, &modules, &sources),
        [
            "CYCLES lists `github` using `web`, which LAYERS doesn't",
            "CYCLES lists `github` using `error`, which no longer points down LAYERS",
            "CYCLES gives no reason for `error` using `github`",
        ]
    );
}

#[test]
fn test_code_comments_and_the_modules_own_paths_dont_count() {
    let sources = [
        source(
            "github.rs",
            "use crate::github::Login;\nuse crate::testing::temp_path;\n",
        ),
        source("github/tests.rs", "use crate::store::Store;\n"),
        source("github/testing.rs", "use crate::store::Store;\n"),
        source("mirror/tests/upkeep.rs", "use crate::store::Store;\n"),
        source(
            "store.rs",
            "use tsugiki_crate::github;\nuse $crate::error::Error; // Not crate::web.\n",
        ),
    ];
    let uses: Vec<_> = scan(&sources).uses.into_keys().collect();
    assert_eq!(uses, [("store".to_owned(), "error".to_owned())]);
}

#[test]
fn a_line_without_a_reason_is_found() {
    let error = Layer {
        module: "error",
        uses: &[],
        why: "",
    };
    assert_eq!(
        check(&[error], &[], &["error"], &[]),
        ["`error`'s line in LAYERS gives no reason"]
    );
}

#[test]
fn a_functional_core_module_that_may_use_the_shell_is_found() {
    let links = Layer {
        module: "links",
        uses: &["error", "store"],
        why: "Links PRs, and reads them from the store.",
    };
    let rule = KeptOut {
        modules: &["links", "work"],
        of: &["store"],
        ..CORE
    };
    assert_eq!(
        kept_out_problems(&[SAMPLE[0], links], &rule),
        [
            "`links` may use `store`, but the functional core keeps out of the shell",
            "`work` is in the functional core, but has no line in LAYERS",
        ]
    );
}

#[test]
fn the_functional_core_keeps_out_of_the_shell() {
    let problems = kept_out_problems(LAYERS, &CORE);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn a_view_model_that_may_send_requests_is_found() {
    let workbench = Layer {
        module: "workbench",
        uses: &["error", "mirror"],
        why: "Builds the Files tab, and moves the Lookup checkout.",
    };
    assert_eq!(
        kept_out_problems(&[SAMPLE[0], workbench], &VIEW_MODELS),
        ["`workbench` may use `mirror`, but a view model's read sends nothing"]
    );
}

#[test]
fn the_view_models_send_nothing() {
    let problems = kept_out_problems(LAYERS, &VIEW_MODELS);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn the_core_keeps_to_its_layers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources = sources(&root);
    let lib = &sources
        .iter()
        .find(|source| source.path == "lib.rs")
        .unwrap()
        .text;
    let modules = declared_modules(lib);
    assert!(modules.len() > 20, "lib.rs declares {modules:?}");
    let problems = check(LAYERS, CYCLES, &modules, &sources);
    assert!(problems.is_empty(), "{problems:#?}");
}
