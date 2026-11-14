//! Each test that looks a name up starts the real ty that CI fetches, on a Repository built with the real
//! git, and nothing reaches the network.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::testing::{FakeClangd, Fixture, ty};
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

/// The ty the owner keeps for `repository`.
fn ty_of(repository: RepositoryName) -> Key {
    (repository, ServerName::Ty)
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
        servers: Box::new(move |server| (server == ServerName::Ty).then(|| ty.clone()).flatten()),
        idle: idle_after(Duration::from_secs(3600)),
    }
}

/// An idle time of `idle`, which the tests that don't stop an idle server set past their end.
fn idle_after(idle: Duration) -> Box<dyn Fn() -> Duration + Send> {
    Box::new(move || idle)
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
        other @ (Answer::NotSetUp { .. } | Answer::NoServer | Answer::Cancelled) => {
            panic!("found nothing: {other:?}")
        }
    }
}

/// A lookup's answer, what it heard it waits for, and what it heard of its server's index.
struct Asked {
    answer: Receiver<Result<Answer, Error>>,
    waited: Receiver<Waiting>,
    indexed: Receiver<Index>,
}

/// Asks the owner `lookup` with `ticket`, as the handle does, without waiting for the answer.
fn ask(owner: &mut Owner, lookup: &Lookup, ticket: Ticket) -> Asked {
    let (reply, answer) = mpsc::channel();
    let (heard, waited) = mpsc::channel();
    let waiting = Box::new(move |waiting| {
        let _unheard = heard.send(waiting);
    });
    let (heard, indexed) = mpsc::channel();
    let index = Box::new(move |index| {
        let _unheard = heard.send(index);
    });
    let asker = Asker {
        ticket,
        reply,
        waiting,
        index,
    };
    let _goes_on = owner.handle(Message::LookUp(lookup.clone(), asker));
    Asked {
        answer,
        waited,
        indexed,
    }
}

/// A listener to a server's index that ignores it.
fn unheard() -> OnIndex {
    Box::new(|_| {})
}

/// The answer of `asked`, once the owner has handled the messages up to it.
fn answer_of(owner: &mut Owner, asked: &Asked) -> Result<Answer, Error> {
    loop {
        if let Ok(answer) = asked.answer.try_recv() {
            return answer;
        }
        let message = owner.next().unwrap();
        let _goes_on = owner.handle(message);
    }
}

/// Where `lookup`'s name is defined, as the owner answers it.
fn look(owner: &mut Owner, lookup: &Lookup) -> Result<Answer, Error> {
    let asked = ask(owner, lookup, Ticket(0));
    answer_of(owner, &asked)
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

    let parameter = hits(look(&mut owner, &at_head(11, "self")));
    let only_head = hits(look(&mut owner, &at_head(12, "only_head")));
    let standard = hits(look(&mut owner, &at_head(12, "eggs")));
    let added = hits(look(&mut owner, &at_head(12, "more")));

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

    let at_base = hits(look(&mut owner, &removed));
    let at_head = hits(look(&mut owner, &added));

    let old = "def old_name():";
    assert_eq!(at_base, [hit(&fixture, DiffEnd::Base, (SHOP, 4, 4), old)]);
    let only = "def only_head():";
    assert_eq!(at_head, [hit(&fixture, DiffEnd::Head, (SHOP, 5, 4), only)]);
}

#[test]
fn a_builtin_is_found_outside_the_repository() {
    let fixture = cpython("definitions-outside");
    let mut owner = opened(&fixture);

    let found = hits(look(
        &mut owner,
        &lookup(&fixture, DiffEnd::Head, 15, "print"),
    ));

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
        let code = owner
            .files
            .read(Path::new(""), &file, line..line + 1)
            .unwrap();
        assert_eq!(code.lines, ["def print("]);
    }
}

#[test]
fn a_name_defined_nowhere_finds_nothing() {
    let fixture = cpython("definitions-nowhere");
    let mut owner = opened(&fixture);
    // The `return` keyword names nothing.
    let keyword = lookup(&fixture, DiffEnd::Head, 6, "return");

    assert_eq!(hits(look(&mut owner, &keyword)), []);
}

#[test]
fn without_a_server_a_lookup_says_to_set_it_up_and_another_language_has_no_server() {
    let fixture = cpython("definitions-not-set-up");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), None));
    let python = lookup(&fixture, DiffEnd::Head, 12, "eggs");
    let header = Lookup {
        path: "Include/patchlevel.h".to_owned(),
        ..python.clone()
    };
    let text = Lookup {
        path: "README.rst".to_owned(),
        ..python.clone()
    };

    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: changed(),
    };
    owner.opened(&repository(), pr);

    let rust = Lookup {
        path: "src/lib.rs".to_owned(),
        ..python.clone()
    };

    assert!(owner.servers.is_empty());
    let ty = Answer::NotSetUp {
        server: ServerName::Ty,
    };
    let rust_analyzer = Answer::NotSetUp {
        server: ServerName::RustAnalyzer,
    };
    let clangd = Answer::NotSetUp {
        server: ServerName::Clangd,
    };
    assert_eq!(look(&mut owner, &python), Ok(ty));
    assert_eq!(look(&mut owner, &rust), Ok(rust_analyzer));
    assert_eq!(look(&mut owner, &header), Ok(clangd));
    assert_eq!(look(&mut owner, &text), Ok(Answer::NoServer));
}

#[test]
fn a_lookup_before_its_pr_opened_starts_ty_with_no_file_open() {
    let fixture = cpython("definitions-unopened");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));

    let found = hits(look(
        &mut owner,
        &lookup(&fixture, DiffEnd::Base, 10, "eggs"),
    ));

    let eggs = "def eggs(count):";
    let spam = ("Lib/spam.py", 1, 4);
    assert_eq!(found, [hit(&fixture, DiffEnd::Base, spam, eggs)]);
}

#[test]
fn a_pr_at_other_commits_gets_a_new_ty() {
    let fixture = cpython("definitions-moved");
    let other = another("definitions-moved-other");
    let mut owner = opened(&fixture);
    let first = owner
        .servers
        .get(&ty_of(repository()))
        .map(|ty| ty.at().clone());
    let asked = ask(
        &mut owner,
        &lookup(&fixture, DiffEnd::Head, 12, "eggs"),
        Ticket(1),
    );
    let source_of_other = other.checkout.clone();
    owner.sources.checkout = Box::new(move |_| Some(source_of_other.clone()));

    let pr = Pr {
        at: other.checkout.at.clone(),
        changed: changed(),
    };
    owner.opened(&repository(), pr);

    let now = owner
        .servers
        .get(&ty_of(repository()))
        .map(|ty| ty.at().clone());
    assert_eq!(first, Some(fixture.checkout.at.clone()));
    assert_eq!(now, Some(other.checkout.at.clone()));
    // The lookup the old ty worked on is cancelled, so its failure can't stop the new ty.
    assert_eq!(asked.answer.try_recv(), Ok(Ok(Answer::Cancelled)));
    assert!(owner.running.is_none());
}

#[test]
fn a_lookup_while_the_checkout_is_elsewhere_fails_and_starts_nothing() {
    let fixture = cpython("definitions-elsewhere");
    let other = another("definitions-elsewhere-other");
    let mut owner = Owner::new(sources(Some(other.checkout.clone()), Some(ty())));

    let asked = ask(
        &mut owner,
        &lookup(&fixture, DiffEnd::Head, 12, "eggs"),
        Ticket(1),
    );
    let answer = answer_of(&mut owner, &asked);

    let message = "Tsugiki has no copy of this PR's files for ty to read yet.";
    assert_eq!(answer, Err(Error::new(ErrorKind::NotFound, message)));
    assert!(owner.servers.is_empty());
    // ty didn't start, so the peek never said it did.
    assert_eq!(asked.waited.try_iter().count(), 0);
}

#[test]
fn a_lookup_at_commits_the_checkout_isn_t_at_leaves_the_running_ty_alone() {
    let fixture = cpython("definitions-stale");
    let other = another("definitions-stale-other");
    let mut owner = opened(&fixture);

    let answer = look(&mut owner, &lookup(&other, DiffEnd::Head, 12, "eggs"));

    assert!(answer.is_err());
    let running = owner
        .servers
        .get(&ty_of(repository()))
        .map(|ty| ty.at().clone());
    assert_eq!(running, Some(fixture.checkout.at.clone()));
}

#[test]
fn the_files_tab_reading_its_pr_again_starts_nothing_again() {
    let fixture = cpython("definitions-again");
    let asked = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&asked);
    let mut owner = Owner::new(Sources {
        checkout: Box::new(|_| None),
        servers: Box::new(move |_| {
            let _before = counted.fetch_add(1, Ordering::Relaxed);
            Some(PathBuf::from("no-ty.exe"))
        }),
        idle: idle_after(Duration::from_secs(3600)),
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
fn a_ty_no_lookup_reaches_for_the_idle_time_stops_and_the_next_lookup_starts_it_again() {
    let fixture = cpython("definitions-idle");
    let idle = Duration::from_secs(1);
    let mut owner = Owner::new(Sources {
        idle: idle_after(idle),
        ..sources(Some(fixture.checkout.clone()), Some(ty()))
    });
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: changed(),
    };
    owner.opened(&repository(), pr);
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    let asked = ask(&mut owner, &at_head, Ticket(1));
    // A lookup keeps the server it asked, however long the server takes.
    owner.stop_idle(Instant::now() + idle * 10, idle);
    let first = hits(answer_of(&mut owner, &asked));
    // The answer counts as a use.
    owner.stop_idle(Instant::now() + idle / 2, idle);
    let kept = owner.servers.contains_key(&ty_of(repository()));
    // The thread waits for its next message only until ty has been idle that long, and then stops it.
    let own = owner.own.clone();
    let _later = thread::spawn(move || {
        thread::sleep(idle * 2);
        let _unheard = own.send(Message::Cancel(Ticket(9)));
    });
    let message = owner.next().unwrap();
    let stopped = owner.servers.is_empty();
    let again = ask(&mut owner, &at_head, Ticket(2));
    let second = hits(answer_of(&mut owner, &again));

    assert!(kept);
    assert!(matches!(message, Message::Cancel(Ticket(9))));
    assert!(stopped);
    assert_eq!(second, first);
    let starting = Waiting::Starting {
        server: ServerName::Ty,
    };
    assert_eq!(again.waited.try_iter().next(), Some(starting));
}

#[test]
fn removing_a_repository_stops_its_servers_cancels_the_lookup_on_one_and_leaves_the_others() {
    let fixture = cpython("definitions-remove");
    let second = another("definitions-remove-other");
    let other = RepositoryName::parse("tsugiki/tsugiki").unwrap();
    let (checkout, elsewhere, theirs) = (
        fixture.checkout.clone(),
        second.checkout.clone(),
        other.clone(),
    );
    let mut owner = Owner::new(Sources {
        checkout: Box::new(move |repository| {
            Some(if *repository == theirs {
                elsewhere.clone()
            } else {
                checkout.clone()
            })
        }),
        ..sources(None, Some(ty()))
    });
    let pr = |fixture: &Fixture, changed: Vec<String>| Pr {
        at: fixture.checkout.at.clone(),
        changed,
    };
    owner.opened(&repository(), pr(&fixture, changed()));
    owner.opened(&other, pr(&second, vec!["a.py".to_owned()]));
    let asked = ask(
        &mut owner,
        &lookup(&fixture, DiffEnd::Head, 12, "only_head"),
        Ticket(1),
    );

    let (reply, removed) = mpsc::channel();
    let goes_on = owner.handle(Message::Remove(repository(), reply));
    // ty's process works in the checkout, so on Windows the folder goes only once it has ended.
    let deleted = std::fs::remove_dir_all(fixture.checkout.folder.parent().unwrap());

    assert!(goes_on);
    removed.try_recv().unwrap();
    assert!(matches!(asked.answer.try_recv(), Ok(Ok(Answer::Cancelled))));
    deleted.unwrap();
    let left: Vec<&Key> = owner.servers.keys().collect();
    assert_eq!(left, [&ty_of(other)]);
    assert!(!owner.prs.contains_key(&repository()));
}

#[test]
fn the_handle_removes_a_repository_once_its_ty_has_ended() {
    let fixture = cpython("definitions-handle-remove");
    let definitions = Definitions::start(sources(Some(fixture.checkout.clone()), Some(ty())));
    let at = fixture.checkout.at.clone();

    definitions.opened(repository(), at, &[diff_file(SHOP)]);
    definitions.remove(repository());
    let deleted = std::fs::remove_dir_all(fixture.checkout.folder.parent().unwrap());
    definitions.stop(Duration::from_secs(10));
    definitions.remove(repository());

    deleted.unwrap();
}

#[test]
fn a_new_lookup_cancels_the_one_running_whose_late_answer_goes_nowhere() {
    let fixture = cpython("definitions-cancel-newer");
    let mut owner = opened(&fixture);
    let first = lookup(&fixture, DiffEnd::Head, 12, "eggs");
    let second = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    let asked = ask(&mut owner, &first, Ticket(1));
    let found = hits(look(&mut owner, &second));

    assert_eq!(
        asked.answer.try_iter().collect::<Vec<_>>(),
        [Ok(Answer::Cancelled)]
    );
    let only = "def only_head():";
    assert_eq!(found, [hit(&fixture, DiffEnd::Head, (SHOP, 5, 4), only)]);
    assert!(owner.running.is_none());
}

#[test]
fn a_cancel_cancels_the_lookup_running_and_ty_goes_on() {
    let fixture = cpython("definitions-cancel");
    let mut owner = opened(&fixture);
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    let asked = ask(&mut owner, &at_head, Ticket(2));
    // A late cancel for an older lookup leaves this one be.
    let _goes_on = owner.handle(Message::Cancel(Ticket(1)));
    let _goes_on = owner.handle(Message::Cancel(Ticket(2)));
    let cancelled = answer_of(&mut owner, &asked);
    // A cancel for a lookup that has answered does nothing.
    let _goes_on = owner.handle(Message::Cancel(Ticket(2)));
    let again = hits(look(&mut owner, &at_head));

    assert_eq!(cancelled, Ok(Answer::Cancelled));
    assert_eq!(again.len(), 1);
}

#[test]
fn a_cancel_the_core_takes_before_its_lookup_cancels_it_and_ty_is_asked_nothing() {
    let fixture = cpython("definitions-cancel-early");
    let mut owner = opened(&fixture);
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    let _goes_on = owner.handle(Message::Cancel(Ticket(1)));
    let asked = ask(&mut owner, &at_head, Ticket(1));

    assert_eq!(asked.answer.try_recv(), Ok(Ok(Answer::Cancelled)));
    assert_eq!(asked.waited.try_iter().count(), 0);
    assert!(owner.running.is_none());
    assert_eq!(owner.asked, 0);
}

#[test]
fn a_lookup_says_ty_starts_then_that_it_asks_and_a_running_ty_is_only_asked() {
    let fixture = cpython("definitions-waiting");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "eggs");

    let first = ask(&mut owner, &at_head, Ticket(1));
    let _found = hits(answer_of(&mut owner, &first));
    let second = ask(&mut owner, &at_head, Ticket(2));
    let _found = hits(answer_of(&mut owner, &second));

    let server = ServerName::Ty;
    let starting = Waiting::Starting { server };
    let asking = Waiting::Asking { server };
    assert_eq!(
        first.waited.try_iter().collect::<Vec<_>>(),
        [starting, asking]
    );
    assert_eq!(second.waited.try_iter().collect::<Vec<_>>(), [asking]);
}

#[test]
fn a_lookup_asked_again_while_ty_starts_asks_ty_nothing() {
    let fixture = cpython("definitions-again-starting");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "eggs");
    // The newer lookup waits in the thread's messages while ty starts.
    let (reply, _answer) = mpsc::channel();
    let newer = Asker {
        ticket: Ticket(2),
        reply,
        waiting: Box::new(|_| {}),
        index: Box::new(|_| {}),
    };
    owner
        .own
        .send(Message::LookUp(at_head.clone(), newer))
        .unwrap();

    let asked = ask(&mut owner, &at_head, Ticket(1));

    assert_eq!(asked.answer.try_recv(), Ok(Ok(Answer::Cancelled)));
    assert!(owner.running.is_none());
    assert_eq!(owner.asked, 0);
}

#[test]
fn a_lookup_cancelled_while_ty_starts_asks_ty_nothing() {
    let fixture = cpython("definitions-cancel-starting");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "eggs");
    // The cancels wait in the thread's messages while ty starts: one for an older lookup, and one for this.
    owner.own.send(Message::Cancel(Ticket(1))).unwrap();
    owner.own.send(Message::Cancel(Ticket(2))).unwrap();

    let asked = ask(&mut owner, &at_head, Ticket(2));

    assert_eq!(asked.answer.try_recv(), Ok(Ok(Answer::Cancelled)));
    let server = ServerName::Ty;
    assert_eq!(
        asked.waited.try_iter().collect::<Vec<_>>(),
        [Waiting::Starting { server }]
    );
    assert!(owner.running.is_none());
    assert_eq!(owner.queued.len(), 2);
}

#[test]
fn a_cancel_for_an_older_lookup_while_ty_starts_leaves_the_new_one_be() {
    let fixture = cpython("definitions-cancel-older");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(ty())));
    let at_base = lookup(&fixture, DiffEnd::Base, 10, "eggs");
    // Nor does a PR that opens meanwhile.
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: changed(),
    };
    owner.own.send(Message::Opened(repository(), pr)).unwrap();
    owner.own.send(Message::Cancel(Ticket(1))).unwrap();

    let asked = ask(&mut owner, &at_base, Ticket(2));
    let found = hits(answer_of(&mut owner, &asked));

    assert_eq!(found.len(), 1);
}

#[test]
fn a_ty_that_ends_before_it_answers_fails_its_lookup_and_the_next_one_starts_it_again() {
    let fixture = cpython("definitions-stopped");
    let mut owner = opened(&fixture);
    let at_head = lookup(&fixture, DiffEnd::Head, 12, "only_head");

    let asked = ask(&mut owner, &at_head, Ticket(1));
    let id = owner.running.as_ref().unwrap().id;
    // What the client hands back when ty ends before it answers. ty's own answer then goes nowhere.
    let _goes_on = owner.handle(Message::Answered(id, Err(RequestError::Exited)));
    let failed = answer_of(&mut owner, &asked);
    let again = hits(look(&mut owner, &at_head));

    let message = failed.unwrap_err().message;
    assert!(message.starts_with("ty couldn't answer: "), "{message}");
    assert_eq!(again.len(), 1);
}

#[test]
fn a_ty_that_doesnt_start_says_why() {
    let fixture = cpython("definitions-no-start");
    let missing = fixture.checkout.folder.join("no-ty.exe");
    let mut owner = Owner::new(sources(Some(fixture.checkout.clone()), Some(missing)));

    let found = look(&mut owner, &lookup(&fixture, DiffEnd::Head, 12, "eggs"));

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

    let found = look(&mut owner, &lookup(&fixture, DiffEnd::Head, 12, "eggs"));

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

/// A path on another drive, but not from its root, stays relative when joined to the checkout's.
#[cfg(windows)]
#[test]
fn a_lookup_at_a_path_that_isn_t_a_file_of_the_checkout_fails_and_ty_goes_on() {
    let fixture = cpython("definitions-odd-path");
    let mut owner = opened(&fixture);
    let odd = Lookup {
        path: "D:odd.py".to_owned(),
        ..lookup(&fixture, DiffEnd::Head, 12, "eggs")
    };

    let failed = look(&mut owner, &odd);
    let again = hits(look(
        &mut owner,
        &lookup(&fixture, DiffEnd::Head, 12, "eggs"),
    ));

    let message = "The Lookup checkout isn't at a full path.";
    assert_eq!(failed, Err(Error::new(ErrorKind::Internal, message)));
    assert_eq!(again.len(), 1);
}

#[test]
fn a_plain_path_drops_its_dot_and_dot_dot_parts() {
    // A path keeps a `.` only at its start.
    let path = Path::new(".")
        .join("work")
        .join("lookup")
        .join("..")
        .join("a.py");

    assert_eq!(plain(&path), Path::new("work").join("a.py"));
}

#[test]
fn the_files_a_server_reads_are_python_rust_and_c_files() {
    let files = [
        "a.py",
        "b.pyi",
        "c.c",
        "d.pyc",
        "odd\nname.py",
        "e.rs",
        "f.rsx",
        "g.h",
        "h.c.h",
        "i.cpp",
    ]
    .map(diff_file);

    assert_eq!(
        served_files(&files),
        ["a.py", "b.pyi", "c.c", "e.rs", "g.h", "h.c.h"]
    );
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
    let (heard, waited) = mpsc::channel();
    let hears = Box::new(move |waiting| {
        let _unheard = heard.send(waiting);
    });
    let found = hits(definitions.look_up(at_head.clone(), Ticket(1), (hears, unheard())));
    // The lookup has answered, so nothing is cancelled.
    definitions.cancel(Ticket(1));
    definitions.stop(Duration::from_secs(10));
    // ty's process works in the checkout, so on Windows the folder goes only once it has ended.
    let removed = std::fs::remove_dir_all(fixture.checkout.folder.parent().unwrap());
    let after = definitions.look_up(at_head, Ticket(2), (Box::new(|_| {}), unheard()));

    let only = "def only_head():";
    assert_eq!(found, [hit(&fixture, DiffEnd::Head, (SHOP, 5, 4), only)]);
    // ty started when the PR opened.
    let asking = Waiting::Asking {
        server: ServerName::Ty,
    };
    assert_eq!(waited.try_iter().collect::<Vec<_>>(), [asking]);
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
    let found = definitions.look_up(
        lookup(&fixture, DiffEnd::Head, 12, "eggs"),
        Ticket(1),
        (Box::new(|_| {}), unheard()),
    );

    let message = found.unwrap_err().message;
    let start = "Tsugiki couldn't start ty at ";
    assert!(message.starts_with(start), "{message}");
}

#[test]
fn the_thread_stops_once_the_last_handle_is_gone() {
    let fixture = cpython("definitions-dropped");
    // The thread holds the sources, so this channel closes once it has ended.
    let (alive, ended) = mpsc::channel::<()>();
    let definitions = Definitions::start(Sources {
        checkout: Box::new(|_| None),
        servers: Box::new(move |_| {
            let _alive = &alive;
            None
        }),
        idle: idle_after(Duration::from_secs(3600)),
    });
    let copy = definitions.clone();

    drop(definitions);
    let python = lookup(&fixture, DiffEnd::Head, 12, "eggs");
    let answer = copy.look_up(python, Ticket(1), (Box::new(|_| {}), unheard()));
    let still = ended.try_recv();
    drop(copy);
    let after = ended.recv_timeout(Duration::from_secs(10));

    let ty = ServerName::Ty;
    assert_eq!(answer, Ok(Answer::NotSetUp { server: ty }));
    assert_eq!(still, Err(mpsc::TryRecvError::Empty));
    assert_eq!(after, Err(mpsc::RecvTimeoutError::Disconnected));
}

const SHOP_RS: &str = "src/lib.rs";

const SHOP_RS_AT_BASE: &str = "\
mod stock;
shout::shout!();

pub struct Shop {
    count: u32,
}

impl Shop {
    pub fn sell(&self) -> String {
        let total = self.count + stock::left();
        self.price(total)
    }

    fn price(&self, total: u32) -> String {
        total.to_string()
    }
}
";

const SHOP_RS_AT_HEAD: &str = "\
mod stock;
shout::shout!();

pub struct Shop {
    count: u32,
}

impl Shop {
    pub fn sell(&self) -> String {
        let total = self.count + stock::left() + only_head();
        self.price(total)
    }

    fn price(&self, total: u32) -> String {
        total.to_string()
    }
}

fn only_head() -> u32 {
    2
}
";

/// A Cargo workspace whose build script and proc macro would each write a file into the checkout, with a
/// `rust-analyzer.toml` that turns both on, a `rust-toolchain.toml` that names a toolchain nobody has, and a
/// dependency no cargo cache holds. The PR changes `src/lib.rs`.
fn crate_with_a_build_script(name: &str) -> Fixture {
    let manifest = "\
[package]
name = \"shop\"
version = \"0.1.0\"
edition = \"2021\"

[workspace]
members = [\"shout\"]

[dependencies]
shout = { path = \"shout\" }
in-no-cache = \"1\"
";
    let build = "fn main() {\n    std::fs::write(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/built\"), \"\").unwrap();\n}\n";
    let shout_manifest = "[package]\nname = \"shout\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nproc-macro = true\n";
    let shout = "\
use proc_macro::TokenStream;

#[proc_macro]
pub fn shout(_: TokenStream) -> TokenStream {
    std::fs::write(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/expanded\"), \"\").unwrap();
    TokenStream::new()
}
";
    Fixture::new(
        name,
        &[
            ("Cargo.toml", manifest),
            ("build.rs", build),
            ("shout/Cargo.toml", shout_manifest),
            ("shout/src/lib.rs", shout),
            ("src/stock.rs", "pub fn left() -> u32 {\n    3\n}\n"),
            (SHOP_RS, SHOP_RS_AT_BASE),
            (
                "rust-analyzer.toml",
                "[cargo.buildScripts]\nenable = true\n\n[procMacro]\nenable = true\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.42.0\"\n"),
        ],
        &[(SHOP_RS, Some(SHOP_RS_AT_HEAD))],
    )
}

/// The servers' thread, run here, with rust-analyzer set up and the fixture's PR opened.
fn rust_opened(fixture: &Fixture) -> Owner {
    let checkout = fixture.checkout.clone();
    let program = super::testing::rust_analyzer();
    let mut owner = Owner::new(Sources {
        checkout: Box::new(move |_| Some(checkout.clone())),
        servers: Box::new(move |server| {
            (server == ServerName::RustAnalyzer).then(|| program.clone())
        }),
        idle: idle_after(Duration::from_secs(3600)),
    });
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: vec![SHOP_RS.to_owned()],
    };
    owner.opened(&repository(), pr);
    owner
}

/// A lookup of `word` on line `line` of `src/lib.rs` at the head.
fn rust_lookup(fixture: &Fixture, line: u32, word: &str) -> Lookup {
    let found = (SHOP_RS_AT_HEAD.lines())
        .nth(usize::try_from(line - 1).unwrap())
        .unwrap();
    let byte = found.find(word).unwrap();
    Lookup {
        repository: repository(),
        at: fixture.checkout.at.clone(),
        path: SHOP_RS.to_owned(),
        side: DiffEnd::Head,
        line,
        column: tsugiki_lsp::utf16::column_of(found, byte),
    }
}

#[test]
fn rust_analyzer_finds_a_local_a_method_a_function_elsewhere_or_at_the_head_and_a_std_type() {
    let fixture = crate_with_a_build_script("definitions-rust");
    let mut owner = rust_opened(&fixture);
    // rust-analyzer waits for the first lookup in Rust.
    let started_with_pr = owner.servers.len();

    let mut found = |line, word| hits(look(&mut owner, &rust_lookup(&fixture, line, word)));
    let local = found(11, "total");
    let method = found(11, "price");
    let elsewhere = found(10, "left");
    let head = found(10, "only_head");
    let standard = found(9, "String");

    let at = |side, place, text| vec![hit(&fixture, side, place, text)];
    assert_eq!(started_with_pr, 0);
    let total = "        let total = self.count + stock::left() + only_head();";
    assert_eq!(local, at(DiffEnd::Head, (SHOP_RS, 10, 12), total));
    let price = "    fn price(&self, total: u32) -> String {";
    assert_eq!(method, at(DiffEnd::Head, (SHOP_RS, 14, 7), price));
    let left = "pub fn left() -> u32 {";
    assert_eq!(elsewhere, at(DiffEnd::Base, ("src/stock.rs", 1, 7), left));
    let only_head = "fn only_head() -> u32 {";
    assert_eq!(head, at(DiffEnd::Head, (SHOP_RS, 19, 3), only_head));
    let [string] = standard.as_slice() else {
        panic!("{standard:?}");
    };
    let Place::Outside { path } = &string.place else {
        panic!("{string:?}");
    };
    assert!(path.ends_with("string.rs"), "{path}");
    assert_eq!(string.text, "pub struct String {");
    // Nothing built or ran the build script or the proc macro, whatever `rust-analyzer.toml` says.
    let folder = &fixture.checkout.folder;
    assert!(!folder.join("built").exists());
    assert!(!folder.join("shout/expanded").exists());
    assert!(!folder.join("target").exists());
}

#[test]
fn a_pr_at_other_commits_stops_its_rust_analyzer_and_cancels_the_lookup_on_it() {
    let fixture = crate_with_a_build_script("definitions-rust-moved");
    let moved = another("definitions-rust-moved-on");
    let mut owner = rust_opened(&fixture);
    let asked = ask(&mut owner, &rust_lookup(&fixture, 10, "left"), Ticket(1));
    let started = owner.servers.len();

    let pr = Pr {
        at: moved.checkout.at.clone(),
        changed: Vec::new(),
    };
    owner.opened(&repository(), pr);

    assert_eq!(started, 1);
    assert!(owner.servers.is_empty());
    assert_eq!(asked.answer.try_recv(), Ok(Ok(Answer::Cancelled)));
}

const EGGS_C: &str = "Objects/eggs.c";

const EGGS_C_AT_BASE: &str = "\
#include \"spam.h\"

static int old_helper(void)
{
    return 1;
}

static int eggs_twice(int count)
{
    int total = count * 2;
    return total + old_helper() + spam_count(SPAM_LIMIT);
}

int eggs(void)
{
    return eggs_twice(3);
}
";

const EGGS_C_AT_HEAD: &str = "\
#include \"spam.h\"

static int only_head(void)
{
    return 2;
}

static int eggs_twice(int count)
{
    int total = count * 2;
    return total + only_head() + spam_count(SPAM_LIMIT);
}

int eggs(void)
{
    return eggs_twice(3);
}
";

/// A Repository with CPython's layout, whose `Objects/other.c` defines what `Include/spam.h` declares. The PR
/// changes `Objects/eggs.c`.
fn c_files(name: &str) -> Fixture {
    Fixture::new(
        name,
        &[
            ("Include/patchlevel.h", PATCHLEVEL),
            (
                "Include/spam.h",
                "#define SPAM_LIMIT 10\n\nint spam_count(int limit);\n",
            ),
            (
                "Objects/other.c",
                "#include \"spam.h\"\n\nint spam_count(int limit)\n{\n    return limit + 1;\n}\n",
            ),
            (EGGS_C, EGGS_C_AT_BASE),
        ],
        &[(EGGS_C, Some(EGGS_C_AT_HEAD))],
    )
}

/// The servers' thread, run here, with clangd set up and the fixture's PR opened.
fn c_opened(fixture: &Fixture) -> Owner {
    let checkout = fixture.checkout.clone();
    let program = super::testing::clangd();
    let mut owner = Owner::new(Sources {
        checkout: Box::new(move |_| Some(checkout.clone())),
        servers: Box::new(move |server| (server == ServerName::Clangd).then(|| program.clone())),
        idle: idle_after(Duration::from_secs(3600)),
    });
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: vec![EGGS_C.to_owned()],
    };
    owner.opened(&repository(), pr);
    owner
}

/// A lookup of `word` on line `line` of `Objects/eggs.c` at `side`.
fn c_lookup(fixture: &Fixture, side: DiffEnd, line: u32, word: &str) -> Lookup {
    let text = match side {
        DiffEnd::Base => EGGS_C_AT_BASE,
        DiffEnd::Head => EGGS_C_AT_HEAD,
    };
    let found = (text.lines())
        .nth(usize::try_from(line - 1).unwrap())
        .unwrap();
    let byte = found.find(word).unwrap();
    Lookup {
        repository: repository(),
        at: fixture.checkout.at.clone(),
        path: EGGS_C.to_owned(),
        side,
        line,
        column: tsugiki_lsp::utf16::column_of(found, byte),
    }
}

#[test]
fn clangd_finds_a_local_a_parameter_a_macro_a_static_function_and_a_name_at_either_side() {
    let fixture = c_files("definitions-c");
    let mut owner = c_opened(&fixture);
    // clangd starts with the PR.
    let started_with_pr = owner.servers.len();

    let mut found =
        |side, line, word| hits(look(&mut owner, &c_lookup(&fixture, side, line, word)));
    let local = found(DiffEnd::Head, 11, "total");
    let parameter = found(DiffEnd::Head, 10, "count");
    let r#macro = found(DiffEnd::Head, 11, "SPAM_LIMIT");
    let function = found(DiffEnd::Head, 16, "eggs_twice");
    let head = found(DiffEnd::Head, 11, "only_head");
    let base = found(DiffEnd::Base, 11, "old_helper");

    let at = |side, place, text| vec![hit(&fixture, side, place, text)];
    assert_eq!(started_with_pr, 1);
    let total = "    int total = count * 2;";
    assert_eq!(local, at(DiffEnd::Head, (EGGS_C, 10, 8), total));
    let eggs_twice = "static int eggs_twice(int count)";
    assert_eq!(parameter, at(DiffEnd::Head, (EGGS_C, 8, 26), eggs_twice));
    let limit = "#define SPAM_LIMIT 10";
    assert_eq!(r#macro, at(DiffEnd::Base, ("Include/spam.h", 1, 8), limit));
    assert_eq!(function, at(DiffEnd::Head, (EGGS_C, 8, 11), eggs_twice));
    let only_head = "static int only_head(void)";
    assert_eq!(head, at(DiffEnd::Head, (EGGS_C, 3, 11), only_head));
    let old_helper = "static int old_helper(void)";
    assert_eq!(base, at(DiffEnd::Base, (EGGS_C, 3, 11), old_helper));
}

/// Until its index has `Objects/other.c`, clangd names the declaration in `Include/spam.h`.
#[test]
fn after_its_index_clangd_finds_a_function_defined_in_another_file() {
    let fixture = c_files("definitions-c-index");
    let mut owner = c_opened(&fixture);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");
    let defined = hit(
        &fixture,
        DiffEnd::Base,
        ("Objects/other.c", 3, 4),
        "int spam_count(int limit)",
    );

    let until = Instant::now() + PATIENCE;
    let mut found = hits(look(&mut owner, &lookup));
    while found != [defined.clone()] && Instant::now() < until {
        thread::sleep(Duration::from_millis(100));
        found = hits(look(&mut owner, &lookup));
    }

    assert_eq!(found, [defined]);
    // clangd keeps its index beside its compile database, outside the checkout.
    let folder = &fixture.checkout.folder;
    let database = crate::folders::servers_beside(folder).join("clangd");
    assert!(
        database
            .join(".cache")
            .join("clangd")
            .join("index")
            .is_dir()
    );
    assert!(!folder.join(".cache").exists());
}

/// Sources that give the checkout `checkout` holds, and a clangd at `program`, which the tests that insert a
/// fake clangd start only when they replace it.
fn clangd_sources(checkout: Arc<Mutex<LookupCheckout>>, program: PathBuf) -> Sources {
    Sources {
        checkout: Box::new(move |_| Some(checkout.lock().unwrap().clone())),
        servers: Box::new(move |server| (server == ServerName::Clangd).then(|| program.clone())),
        idle: idle_after(Duration::from_secs(3600)),
    }
}

/// A location in `path` of the fixture's checkout, at `line` and `character` from 0, as a server names one.
fn location(fixture: &Fixture, path: &str, (line, character): (u32, u32)) -> serde_json::Value {
    let file = plain(&fixture.checkout.folder).join(path);
    let start = json!({"line": line, "character": character});
    let end = json!({"line": line, "character": character + 10});
    json!([{"uri": Uri::from_path(&file).unwrap(), "range": {"start": start, "end": end}}])
}

/// The servers' thread, run here, with the fixture's PR and a fake clangd on its checkout. The fake names
/// the declaration of `spam_count` in `Include/spam.h` for the first lookup, as clangd does until its index
/// holds `Objects/other.c`, and the definition there for the rest.
fn with_fake_clangd(fixture: &Fixture, sources: Sources) -> (Owner, FakeClangd) {
    let mut owner = Owner::new(sources);
    let answers = vec![
        location(fixture, "Include/spam.h", (2, 4)),
        location(fixture, "Objects/other.c", (2, 4)),
    ];
    let (fake, client) = FakeClangd::start(answers);
    let key = (repository(), ServerName::Clangd);
    let setup = clangd::setup(&fixture.checkout.folder).unwrap();
    let changed = [EGGS_C.to_owned()];
    let server = LanguageServer::initialize(
        ServerName::Clangd,
        (setup, client),
        (fixture.checkout.clone(), &changed),
        index_moved(&owner.own, &key),
    )
    .unwrap();
    let _none = owner.servers.insert(key, server);
    let pr = Pr {
        at: fixture.checkout.at.clone(),
        changed: changed.to_vec(),
    };
    let _none = owner.prs.insert(repository(), pr);
    (owner, fake)
}

/// [`with_fake_clangd`] with sources that hold the fixture's checkout.
fn fake_clangd(fixture: &Fixture) -> (Owner, FakeClangd) {
    let checkout = Arc::new(Mutex::new(fixture.checkout.clone()));
    with_fake_clangd(fixture, clangd_sources(checkout, PathBuf::from("clangd")))
}

/// The declaration the fake clangd names first.
fn declaration(fixture: &Fixture) -> Vec<Hit> {
    let declared = ("Include/spam.h", 3, 4);
    vec![hit(
        fixture,
        DiffEnd::Base,
        declared,
        "int spam_count(int limit);",
    )]
}

/// The definition the fake clangd names once asked again.
fn definition(fixture: &Fixture) -> Vec<Hit> {
    let defined = ("Objects/other.c", 3, 4);
    vec![hit(
        fixture,
        DiffEnd::Base,
        defined,
        "int spam_count(int limit)",
    )]
}

/// Handles the next `count` messages, each of which says the index has moved on.
fn index_moved_on(owner: &mut Owner, count: usize) {
    for _ in 0..count {
        let message = owner.incoming.recv_timeout(PATIENCE).unwrap();
        assert!(matches!(message, Message::Indexing(_)));
        let _goes_on = owner.handle(message);
    }
}

/// The next news `asked` hears of its server's index, once the owner has handled the messages up to it.
fn heard_next(owner: &mut Owner, asked: &Asked) -> Index {
    loop {
        if let Ok(index) = asked.indexed.try_recv() {
            return index;
        }
        let message = owner.next().unwrap();
        let _goes_on = owner.handle(message);
    }
}

/// clangd builds its index, and has indexed `done` of 452 files.
fn building(done: u32) -> Index {
    let files = IndexedFiles { done, total: 452 };
    Index::Building {
        server: ServerName::Clangd,
        files: Some(files),
    }
}

/// clangd builds its index, and hasn't counted the files yet.
fn uncounted() -> Index {
    Index::Building {
        server: ServerName::Clangd,
        files: None,
    }
}

/// The fake reports that clangd begins its index, and has indexed `done` of 452 files.
fn begin(fake: &FakeClangd, done: u32) {
    fake.progress(json!({"kind": "begin", "title": "indexing", "percentage": 0}));
    let message = format!("{done}/452");
    fake.progress(json!({"kind": "report", "message": message, "percentage": done * 100 / 452}));
}

#[test]
fn an_answer_while_clangd_indexes_hears_how_far_it_has_got_and_the_answer_once_the_index_is_built()
{
    let fixture = c_files("definitions-indexing");
    let (mut owner, fake) = fake_clangd(&fixture);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");
    begin(&fake, 120);
    index_moved_on(&mut owner, 2);

    let asked = ask(&mut owner, &lookup, Ticket(1));
    let found = hits(answer_of(&mut owner, &asked));
    let with_answer: Vec<Index> = asked.indexed.try_iter().collect();
    // A count comes at most once a second.
    thread::sleep(server::NEWS_EVERY);
    fake.progress(json!({"kind": "report", "message": "200/452", "percentage": 44}));
    index_moved_on(&mut owner, 1);
    let reported: Vec<Index> = asked.indexed.try_iter().collect();
    fake.progress(json!({"kind": "end"}));
    index_moved_on(&mut owner, 1);
    let built = heard_next(&mut owner, &asked);
    let later = ask(&mut owner, &lookup, Ticket(2));
    let found_later = hits(answer_of(&mut owner, &later));

    assert_eq!(found, declaration(&fixture));
    assert_eq!(with_answer, [building(120)]);
    assert_eq!(reported, [building(200)]);
    // The core asked clangd again, and the window hears what it names now.
    let hits = definition(&fixture);
    assert_eq!(built, Index::Built { hits: hits.clone() });
    assert_eq!(found_later, hits);
    assert_eq!(later.indexed.try_iter().count(), 0);
    assert_eq!(fake.asked(), 3);
}

#[test]
fn a_lookup_cancelled_after_it_answered_hears_nothing_more_and_isn_t_asked_again() {
    let fixture = c_files("definitions-indexing-cancelled");
    let (mut owner, fake) = fake_clangd(&fixture);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");
    fake.progress(json!({"kind": "begin", "title": "indexing", "percentage": 0}));
    index_moved_on(&mut owner, 1);

    let asked = ask(&mut owner, &lookup, Ticket(1));
    let _found = hits(answer_of(&mut owner, &asked));
    let _goes_on = owner.handle(Message::Cancel(Ticket(1)));
    fake.progress(json!({"kind": "report", "message": "300/452", "percentage": 66}));
    fake.progress(json!({"kind": "end"}));
    index_moved_on(&mut owner, 2);

    assert_eq!(asked.indexed.try_iter().collect::<Vec<_>>(), [uncounted()]);
    assert!(owner.running.is_none());
    assert_eq!(fake.asked(), 1);
}

#[test]
fn an_index_built_after_clangd_was_asked_and_before_it_answered_asks_it_again_at_once() {
    let fixture = c_files("definitions-indexing-built");
    let (mut owner, fake) = fake_clangd(&fixture);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");
    fake.progress(json!({"kind": "report", "message": "451/452", "percentage": 99}));
    index_moved_on(&mut owner, 1);
    let key = (repository(), ServerName::Clangd);

    let asked = ask(&mut owner, &lookup, Ticket(1));
    fake.progress(json!({"kind": "end"}));
    let until = Instant::now() + PATIENCE;
    while owner.indexing_of(&key) != Indexing::Built && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    let found = hits(answer_of(&mut owner, &asked));
    let built = heard_next(&mut owner, &asked);

    assert_eq!(found, declaration(&fixture));
    let hits = definition(&fixture);
    assert_eq!(built, Index::Built { hits });
}

/// clangd reports its index once it has read its compile database, which may come after its first answer.
#[test]
fn an_answer_before_clangd_reports_its_index_hears_of_the_index_once_it_does() {
    let fixture = c_files("definitions-indexing-unreported");
    let (mut owner, fake) = fake_clangd(&fixture);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");

    let asked = ask(&mut owner, &lookup, Ticket(1));
    let found = hits(answer_of(&mut owner, &asked));
    let with_answer = asked.indexed.try_iter().count();
    begin(&fake, 120);
    index_moved_on(&mut owner, 2);
    let reported: Vec<Index> = asked.indexed.try_iter().collect();
    fake.progress(json!({"kind": "end"}));
    index_moved_on(&mut owner, 1);
    let built = heard_next(&mut owner, &asked);

    assert_eq!(found, declaration(&fixture));
    assert_eq!(with_answer, 0);
    // The owner reads the count once the fake has sent it, which may be before it reads the begin.
    assert_eq!(reported.last(), Some(&building(120)));
    let hits = definition(&fixture);
    assert_eq!(built, Index::Built { hits });
}

#[test]
fn a_removed_repository_or_a_crash_tells_the_lookup_that_clangd_stopped() {
    let fixture = c_files("definitions-indexing-removed");
    let (mut owner, fake) = fake_clangd(&fixture);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");
    fake.progress(json!({"kind": "begin", "title": "indexing", "percentage": 0}));
    index_moved_on(&mut owner, 1);
    let asked = ask(&mut owner, &lookup, Ticket(1));
    let _found = hits(answer_of(&mut owner, &asked));
    let crashing = c_files("definitions-indexing-crash");
    let (mut other, crashes) = fake_clangd(&crashing);
    crashes.progress(json!({"kind": "begin", "title": "indexing", "percentage": 0}));
    index_moved_on(&mut other, 1);
    let lookup = c_lookup(&crashing, DiffEnd::Head, 11, "spam_count");
    let crashed = ask(&mut other, &lookup, Ticket(1));
    let _found = hits(answer_of(&mut other, &crashed));

    owner.remove(&repository());
    crashes.crash();
    index_moved_on(&mut other, 1);

    let heard: Vec<Index> = asked.indexed.try_iter().collect();
    assert_eq!(heard, [uncounted(), Index::Stopped]);
    let heard: Vec<Index> = crashed.indexed.try_iter().collect();
    assert_eq!(heard, [uncounted(), Index::Stopped]);
}

#[test]
fn a_pr_at_other_commits_tells_the_lookup_that_its_clangd_stopped() {
    let fixture = c_files("definitions-indexing-moved");
    let checkout = Arc::new(Mutex::new(fixture.checkout.clone()));
    let sources = clangd_sources(Arc::clone(&checkout), super::testing::clangd());
    let (mut owner, fake) = with_fake_clangd(&fixture, sources);
    let lookup = c_lookup(&fixture, DiffEnd::Head, 11, "spam_count");
    fake.progress(json!({"kind": "begin", "title": "indexing", "percentage": 0}));
    index_moved_on(&mut owner, 1);
    let asked = ask(&mut owner, &lookup, Ticket(1));
    let _found = hits(answer_of(&mut owner, &asked));
    let moved = Fixture::new(
        "definitions-indexing-moved-on",
        &[("a.c", "")],
        &[("a.c", Some("int a;\n"))],
    );
    checkout.lock().unwrap().clone_from(&moved.checkout);

    let pr = Pr {
        at: moved.checkout.at.clone(),
        changed: vec!["a.c".to_owned()],
    };
    owner.opened(&repository(), pr);

    let heard: Vec<Index> = asked.indexed.try_iter().collect();
    assert_eq!(heard, [uncounted(), Index::Stopped]);
}
