//! Each test that looks a name up starts the real ty that CI fetches, on a Repository built with the real
//! git, and nothing reaches the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::testing::{Fixture, ty};
use super::*;
use crate::diff::FileChange;

const PATCHLEVEL: &str = "#define PY_MAJOR_VERSION        3\n#define PY_MINOR_VERSION        14\n";

const SHOP: &str = "Tools/shop.py";

const SHOP_AT_BASE: &str = "\
from spam import eggs


def old_name():
    return 1


class Shop:
    def sell(self, count):
        return old_name() + eggs(count)
";

const SHOP_AT_HEAD: &str = "\
import added
from spam import eggs


def only_head():
    return 2


class Shop:
    def sell(self, count):
        self.count = count
        return only_head() + eggs(count) + added.more()

    def show(self):
        print(self.count)
";

/// A PR in a Repository with CPython's layout: it changes `Tools/shop.py`, adds `Lib/added.py` and deletes
/// `Lib/gone.py`.
fn cpython(name: &str) -> Fixture {
    Fixture::new(
        name,
        &[
            ("Include/patchlevel.h", PATCHLEVEL),
            ("Lib/spam.py", "def eggs(count):\n    return count\n"),
            ("Lib/gone.py", "def bye():\n    pass\n"),
            (SHOP, SHOP_AT_BASE),
        ],
        &[
            (SHOP, Some(SHOP_AT_HEAD)),
            ("Lib/added.py", Some("def more():\n    return 3\n")),
            ("Lib/gone.py", None),
        ],
    )
}

/// Another PR, at other commits: two fixtures with the same files made in the same second have the same
/// commits.
fn another(name: &str) -> Fixture {
    Fixture::new(name, &[("a.py", "")], &[("a.py", Some("a = 1\n"))])
}

fn repository() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// The Python files the PR changes, as its diff lists them.
fn changed() -> Vec<String> {
    [SHOP, "Lib/added.py", "Lib/gone.py"]
        .map(str::to_owned)
        .to_vec()
}

/// Sources that give the fixture's checkout and the pinned ty.
fn sources(checkout: Option<LookupCheckout>, ty: Option<PathBuf>) -> Sources {
    Sources {
        checkout: Box::new(move |_| checkout.clone()),
        ty: Box::new(move || ty.clone()),
    }
}

/// The servers' thread, run here, with the fixture's PR opened.
fn opened(fixture: &Fixture) -> Owner {
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: changed(),
    };
    owner.opened(&repository(), pr);
    owner
}

/// A lookup of `word` on line `line` of `Tools/shop.py` at `side`.
fn lookup(fixture: &Fixture, side: DiffEnd, line: u32, word: &str) -> Lookup {
    let text = match side {
        DiffEnd::Base => SHOP_AT_BASE,
        DiffEnd::Head => SHOP_AT_HEAD,
    };
    let found = (text.lines())
        .nth(usize::try_from(line - 1).unwrap())
        .unwrap();
    let byte = found.find(word).unwrap();
    Lookup {
        repository: repository(),
        at: fixture.checkout.at.clone(),
        path: SHOP.to_owned(),
        side,
        line,
        column: tsugiki_lsp::utf16::column_of(found, byte),
    }
}

fn hits(answer: Result<Answer, Error>) -> Vec<Hit> {
    match answer.unwrap() {
        Answer::Found { hits } => hits,
        other @ (Answer::NotSetUp | Answer::NoServer) => panic!("found nothing: {other:?}"),
    }
}

/// A hit at line `line`, column `column` of `path` in the fixture's Repository at `side`.
fn hit(
    fixture: &Fixture,
    side: DiffEnd,
    (path, line, column): (&str, u32, u32),
    text: &str,
) -> Hit {
    let at = &fixture.checkout.at;
    let commit = match side {
        DiffEnd::Base => at.base.clone(),
        DiffEnd::Head => at.head.clone(),
    };
    Hit {
        place: Place::Repository {
            path: path.to_owned(),
            side,
            commit,
        },
        line,
        column,
        text: text.to_owned(),
    }
}

#[test]
fn a_name_at_the_head_is_found_where_the_head_defines_it() {
    let fixture = cpython("definitions-head");
    let mut owner = opened(&fixture);
    let at_head = |line, word| lookup(&fixture, DiffEnd::Head, line, word);

    let parameter = hits(owner.look_up(&at_head(11, "self")));
    let only_head = hits(owner.look_up(&at_head(12, "only_head")));
    let standard = hits(owner.look_up(&at_head(12, "eggs")));
    let added = hits(owner.look_up(&at_head(12, "more")));

    let sell = "    def sell(self, count):";
    assert_eq!(
        parameter,
        [hit(&fixture, DiffEnd::Head, (SHOP, 10, 13), sell)]
    );
    let only = "def only_head():";
    assert_eq!(
        only_head,
        [hit(&fixture, DiffEnd::Head, (SHOP, 5, 4), only)]
    );
    // CPython's layout puts the standard library in `Lib`, which the PR doesn't change.
    let eggs = "def eggs(count):";
    let spam = ("Lib/spam.py", 1, 4);
    assert_eq!(standard, [hit(&fixture, DiffEnd::Base, spam, eggs)]);
    // A module the PR adds is on disk in the checkout, and open with the rest.
    let more = "def more():";
    let added_file = ("Lib/added.py", 1, 4);
    assert_eq!(added, [hit(&fixture, DiffEnd::Head, added_file, more)]);
}

#[test]
fn a_name_on_a_removed_line_is_found_at_the_base_and_the_next_lookup_is_at_the_head_again() {
    let fixture = cpython("definitions-base");
    let mut owner = opened(&fixture);
    let removed = lookup(&fixture, DiffEnd::Base, 10, "old_name");
    let added = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    let at_base = hits(owner.look_up(&removed));
    let at_head = hits(owner.look_up(&added));

    let old = "def old_name():";
    assert_eq!(at_base, [hit(&fixture, DiffEnd::Base, (SHOP, 4, 4), old)]);
    let only = "def only_head():";
    assert_eq!(at_head, [hit(&fixture, DiffEnd::Head, (SHOP, 5, 4), only)]);
}

#[test]
fn a_builtin_is_found_outside_the_repository() {
    let fixture = cpython("definitions-outside");
    let mut owner = opened(&fixture);

    let found = hits(owner.look_up(&lookup(&fixture, DiffEnd::Head, 15, "print")));

    // typeshed declares `print` twice, as overloads.
    assert!(!found.is_empty());
    for hit in found {
        let Place::Outside { path } = &hit.place else {
            panic!("{hit:?}")
        };
        assert!(path.ends_with("builtins.pyi"), "{path}");
        assert_eq!(hit.text, "def print(");
        // The peek can read the stub's code, as ty named it.
        let file = HitFile::Outside {
            path: PathBuf::from(path),
        };
        let line = hit.line;
        let code = code::read(&owner.named, Path::new(""), &file, line..line + 1).unwrap();
        assert_eq!(code.lines, ["def print("]);
    }
}

#[test]
fn a_name_defined_nowhere_finds_nothing() {
    let fixture = cpython("definitions-nowhere");
    let mut owner = opened(&fixture);
    // The `return` keyword names nothing.
    let keyword = lookup(&fixture, DiffEnd::Head, 6, "return");

    assert_eq!(hits(owner.look_up(&keyword)), []);
}

#[test]
fn without_ty_a_python_lookup_says_to_set_it_up_and_another_language_has_no_server() {
    let fixture = cpython("definitions-not-set-up");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), None));
    let python = lookup(&fixture, DiffEnd::Head, 12, "eggs");
    let header = Lookup {
        path: "Include/patchlevel.h".to_owned(),
        ..python.clone()
    };

    assert_eq!(owner.look_up(&python), Ok(Answer::NotSetUp));
    assert_eq!(owner.look_up(&header), Ok(Answer::NoServer));
}

#[test]
fn a_lookup_before_its_pr_opened_starts_ty_with_no_file_open() {
    let fixture = cpython("definitions-unopened");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));

    let found = hits(owner.look_up(&lookup(&fixture, DiffEnd::Base, 10, "eggs")));

    let eggs = "def eggs(count):";
    let spam = ("Lib/spam.py", 1, 4);
    assert_eq!(found, [hit(&fixture, DiffEnd::Base, spam, eggs)]);
}

#[test]
fn a_pr_at_other_commits_gets_a_new_ty() {
    let fixture = cpython("definitions-moved");
    let other = another("definitions-moved-other");
    let mut owner = opened(&fixture);
    let first = owner.servers.get(&repository()).map(|ty| ty.at().clone());
    let source_of_other = other.checkout.clone();
    owner.sources.checkout = Box::new(move |_| Some(source_of_other.clone()));

    let pr = Pr {
        at: other.checkout.at.clone(),
        changed: changed(),
    };
    owner.opened(&repository(), pr);

    let now = owner.servers.get(&repository()).map(|ty| ty.at().clone());
    assert_eq!(first, Some(fixture.checkout.at.clone()));
    assert_eq!(now, Some(other.checkout.at.clone()));
}

#[test]
fn a_lookup_while_the_checkout_is_elsewhere_fails_and_starts_nothing() {
    let fixture = cpython("definitions-elsewhere");
    let other = another("definitions-elsewhere-other");
    let mut owner = Owner::new(sources(Some(other.checkout.clone()), Some(ty())));

    let answer = owner.look_up(&lookup(&fixture, DiffEnd::Head, 12, "eggs"));

    let message = "Tsugiki has no copy of this PR's files for ty to read yet.";
    assert_eq!(answer, Err(Error::new(ErrorKind::NotFound, message)));
    assert!(owner.servers.is_empty());
}

#[test]
fn a_lookup_at_commits_the_checkout_isn_t_at_leaves_the_running_ty_alone() {
    let fixture = cpython("definitions-stale");
    let other = another("definitions-stale-other");
    let mut owner = opened(&fixture);

    let answer = owner.look_up(&lookup(&other, DiffEnd::Head, 12, "eggs"));

    assert!(answer.is_err());
    let running = owner.servers.get(&repository()).map(|ty| ty.at().clone());
    assert_eq!(running, Some(fixture.checkout.at.clone()));
}

#[test]
fn the_files_tab_reading_its_pr_again_starts_nothing_again() {
    let fixture = cpython("definitions-again");
    let asked = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&asked);
    let mut owner = Owner::new(Sources {
        checkout: Box::new(|_| None),
        ty: Box::new(move || {
            let _before = counted.fetch_add(1, Ordering::Relaxed);
            Some(PathBuf::from("no-ty.exe"))
        }),
    });
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: changed(),
    };

    owner.opened(&repository(), pr.clone());
    owner.opened(&repository(), pr);

    assert_eq!(asked.load(Ordering::Relaxed), 1);
}

#[test]
fn a_lookup_ty_is_slow_to_answer_is_cancelled_and_ty_goes_on() {
    let fixture = cpython("definitions-slow");
    let mut owner = opened(&fixture);
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    owner.patience = Duration::ZERO;
    let slow = owner.look_up(&at_head);
    owner.patience = PATIENCE;
    let again = hits(owner.look_up(&at_head));

    let message = "ty is still reading the PR's files. Ask again in a moment.";
    assert_eq!(slow, Err(Error::new(ErrorKind::Internal, message)));
    assert_eq!(again.len(), 1);
}

#[test]
fn a_ty_that_stopped_fails_its_lookup_and_the_next_one_starts_it_again() {
    let fixture = cpython("definitions-stopped");
    let mut owner = opened(&fixture);
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");
    let running = owner.servers.get(&repository()).unwrap();
    running.shutdown().wait().unwrap();

    let failed = owner.look_up(&at_head);
    let again = hits(owner.look_up(&at_head));

    let message = failed.unwrap_err().message;
    assert!(message.starts_with("ty couldn't answer: "), "{message}");
    assert_eq!(again.len(), 1);
}

#[test]
fn a_ty_that_doesnt_start_says_why() {
    let fixture = cpython("definitions-no-start");
    let missing = fixture.checkout.folder.join("no-ty.exe");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(missing)));

    let found = owner.look_up(&lookup(&fixture, DiffEnd::Head, 12, "eggs"));

    let message = found.unwrap_err().message;
    let start = "Tsugiki couldn't start ty at ";
    assert!(message.starts_with(start), "{message}");
}

#[test]
fn a_program_that_isnt_a_language_server_doesnt_start() {
    let fixture = cpython("definitions-not-a-server");
    // git ends at once on `git server`, before it answers `initialize`.
    let exec_path = tsugiki_test_git::git(&fixture.checkout.folder, &["--exec-path"]);
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let git = PathBuf::from(exec_path.trim()).join(name);
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(git)));

    let found = owner.look_up(&lookup(&fixture, DiffEnd::Head, 12, "eggs"));

    let message = found.unwrap_err().message;
    assert!(message.starts_with("ty didn't start: "), "{message}");
}

fn diff_file(path: &str) -> DiffFile {
    DiffFile {
        path: path.to_owned(),
        old_path: None,
        change: FileChange::Lines {
            added: 1,
            removed: 0,
        },
        row: 0,
        columns: 0,
        generated: false,
        gaps: Vec::new(),
        rest: None,
        pairs: Vec::new(),
    }
}

#[test]
fn the_python_files_a_pr_changes_are_its_py_and_pyi_files() {
    let files = ["a.py", "b.pyi", "c.c", "d.pyc", "odd\nname.py"].map(diff_file);

    assert_eq!(python_files(&files), ["a.py", "b.pyi"]);
}

#[test]
fn the_handle_starts_ty_for_a_python_pr_and_looks_up_on_its_thread() {
    let fixture = cpython("definitions-handle");
    let definitions = Definitions::start(sources(Some(fixture.checkout.clone()), Some(ty())));
    let files = [SHOP, "Lib/added.py", "README.rst"].map(diff_file);
    let at = fixture.checkout.at.clone();
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    definitions.opened(repository(), at.clone(), &files);
    definitions.opened(repository(), at, &[diff_file("c.c")]);
    let found = hits(definitions.look_up(at_head.clone()));
    definitions.stop(Duration::from_secs(10));
    // ty's process works in the checkout, so on Windows the folder goes only once it has ended.
    let removed = std::fs::remove_dir_all(fixture.checkout.folder.parent().unwrap());
    let after = definitions.look_up(at_head);

    let only = "def only_head():";
    assert_eq!(found, [hit(&fixture, DiffEnd::Head, (SHOP, 5, 4), only)]);
    removed.unwrap();
    let stopped = Error::new(ErrorKind::Internal, "Tsugiki's lookups have stopped.");
    assert_eq!(after, Err(stopped));
}

#[test]
fn a_pr_whose_ty_doesnt_start_is_logged_and_its_lookup_says_why() {
    let fixture = cpython("definitions-handle-no-start");
    let missing = fixture.checkout.folder.join("no-ty.exe");
    let definitions = Definitions::start(sources(Some(fixture.checkout.clone()), Some(missing)));
    let at = fixture.checkout.at.clone();

    definitions.opened(repository(), at, &[diff_file(SHOP)]);
    let found = definitions.look_up(lookup(&fixture, DiffEnd::Head, 12, "eggs"));

    let message = found.unwrap_err().message;
    let start = "Tsugiki couldn't start ty at ";
    assert!(message.starts_with(start), "{message}");
}
