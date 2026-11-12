use std::fs;

use tsugiki_test_git::git;

use super::*;
use crate::diff::{Diff, PrAt, Start, read};
use crate::github::CommitSha;
use crate::repository::RepositoryName;

#[test]
fn lines_close_together_are_read_in_one_run_and_distant_hunks_apart() {
    let lines = [3, 4, 5, 9, 10, 5000, 5001];

    assert_eq!(runs(&lines), [3..11, 5000..5002]);
    assert_eq!(runs(&[]), Vec::<Range<u32>>::new());
}

/// A mirror whose branch `main` has the files `base`, and whose PR branch changes them to `head`. A file
/// with no text in `head` is deleted.
struct Repo {
    root: PathBuf,
    mirror: PathBuf,
    head: CommitSha,
}

impl Repo {
    fn new(name: &str, base: &[(&str, &str)], head: &[(&str, &str)]) -> Self {
        let root = crate::testing::temp_path(&format!("highlighter-{name}"));
        let work = root.join("work");
        fs::create_dir_all(&work).unwrap();
        let write = |files: &[(&str, &str)]| {
            for (path, text) in files {
                let path = work.join(path);
                if text.is_empty() {
                    fs::remove_file(path).unwrap();
                } else {
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, text).unwrap();
                }
            }
            let _ = git(&work, &["add", "-A"]);
            let _ = git(&work, &["commit", "-qm", "Files"]);
        };
        let _ = git(&work, &["init", "-q", "-b", "main"]);
        write(base);
        let _ = git(&work, &["switch", "-qc", "pr"]);
        write(head);
        let mirror = root.join("mirror.git");
        let (from, to) = (work.to_string_lossy(), mirror.to_string_lossy());
        let _ = git(&root, &["clone", "-q", "--bare", &from, &to]);
        let head = git(&mirror, &["rev-parse", "refs/heads/pr"]);
        let head = CommitSha::parse(head.trim()).unwrap();
        Self { root, mirror, head }
    }

    /// The diff of PR `number`, whose head is the PR branch.
    fn source(&self, number: u32) -> Source {
        let read = read(&self.mirror, "main", &Start::TargetBranch, &self.head).unwrap();
        let diff = OpenDiff {
            pr: pr(number, &self.head),
            target: Some(("main".to_owned(), Start::TargetBranch)),
            base: read.base,
            diff: read.diff,
            news: read.news,
        };
        Source {
            diff: Arc::new(diff),
            mirror: self.mirror.clone(),
        }
    }

    fn finish(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}

fn pr(number: u32, head: &CommitSha) -> PrAt {
    PrAt {
        repository: RepositoryName::parse("python/cpython").unwrap(),
        number,
        head: head.clone(),
    }
}

/// Python that sets `x1` to 1, and so on.
fn assigned(lines: std::ops::RangeInclusive<u32>) -> String {
    lines.map(|n| format!("x{n} = {n}\n")).collect()
}

/// A PR that changes a Python file and a C file, adds a Python file, and deletes another.
fn changes(name: &str) -> Repo {
    let base = [
        ("Lib/ast.py", &*assigned(1..=10)),
        ("Lib/gone.py", "pass\n"),
        ("Modules/a.c", "int x;\n"),
    ];
    let head = [
        (
            "Lib/ast.py",
            &*assigned(1..=10).replace("x5 = 5", "x5 = 'five'"),
        ),
        ("Lib/gone.py", ""),
        ("Lib/new.py", "y = 2\n"),
        ("Modules/a.c", "int y;\n"),
    ];
    Repo::new(name, &base, &head)
}

/// Its versions: each file's place in the diff, which lists them by path, and its side.
const AST_BASE: Version = (0, Side::Base);
const AST_HEAD: Version = (0, Side::Head);
const GONE: Version = (1, Side::Base);
const NEW: Version = (2, Side::Head);
const C_BASE: Version = (3, Side::Base);
const C_HEAD: Version = (3, Side::Head);

fn worker(cap: usize) -> (Worker, ShelfReader) {
    let (writer, reader) = shelf();
    (Worker::new(writer, cap), reader)
}

/// What the worker answers a chunk that needs `versions` of `from`.
fn ask(worker: &mut Worker, from: Source, versions: &[Version]) -> HashMap<Version, Tree> {
    let (reply, answer) = mpsc::channel();
    worker.handle(Job::Parse {
        from,
        versions: versions.to_vec(),
        reply,
    });
    answer.recv().unwrap()
}

/// Which of `versions` of `diff` the shelf has a tree for.
fn shelved(shelf: &ShelfReader, diff: &OpenDiff, versions: &[Version]) -> Vec<Version> {
    let has_tree = |version: &&Version| matches!(shelf.get(diff, **version), Some(Some(_)));
    versions.iter().filter(has_tree).copied().collect()
}

const ALL: [Version; 6] = [AST_BASE, AST_HEAD, GONE, NEW, C_BASE, C_HEAD];

/// A Python file of 3,000 lines, long enough that tree-sitter asks whether to stop while parsing it, whose
/// PR changes its first line.
fn long(name: &str) -> Repo {
    let base = assigned(1..=3000);
    let head = base.replace("x1 = 1\n", "x1 = 'one'\n");
    Repo::new(name, &[("Lib/long.py", &base)], &[("Lib/long.py", &head)])
}

const LONG_BASE: Version = (0, Side::Base);
const LONG_HEAD: Version = (0, Side::Head);

/// A chunk of `from` that waits for the thread to parse `versions`.
fn waiting_for(from: Source, versions: &[Version]) -> Receiver<Job> {
    let (jobs, incoming) = mpsc::channel();
    let (reply, _answer) = mpsc::channel();
    jobs.send(Job::Parse {
        from,
        versions: versions.to_vec(),
        reply,
    })
    .unwrap();
    incoming
}

#[test]
fn a_chunk_s_versions_are_parsed_first_and_the_rest_between_chunks_in_order() {
    let repo = changes("ahead");
    let (mut worker, shelf) = worker(CAP);
    let from = repo.source(7);
    let diff = Arc::clone(&from.diff);
    let (_jobs, incoming) = mpsc::channel();
    let mut waiting = VecDeque::new();

    let trees = ask(&mut worker, from, &[C_HEAD]);
    let first = shelved(&shelf, &diff, &ALL);
    let parsed = worker.parse_ahead(&incoming, &mut waiting);
    let then = shelved(&shelf, &diff, &ALL);

    assert!(matches!(trees.get(&C_HEAD), Some(Some(_))));
    assert_eq!(first, [C_HEAD]);
    assert!(parsed);
    assert_eq!(then, ALL);
    assert!(!worker.parse_ahead(&incoming, &mut waiting));
    repo.finish();
}

#[test]
fn news_entries_are_parsed_ahead_first_then_the_files_in_the_diff_s_order() {
    let base = [("Lib/ast.py", "x = 1\n"), ("Doc/notes.txt", "Notes\n")];
    let news = "Misc/NEWS.d/next/Library/2026-11-07.gh-issue-1.rst";
    let head = [
        ("Lib/ast.py", "x = 2\n"),
        ("Doc/notes.txt", "More notes\n"),
        (news, "Fixed.\n"),
    ];
    let repo = Repo::new("news", &base, &head);

    let ahead = repo.source(7).diff.ahead();

    // Doc/notes.txt comes first, but Tsugiki doesn't highlight it.
    assert_eq!(ahead, [(2, Side::Head), (1, Side::Base), (1, Side::Head)]);
    repo.finish();
}

#[test]
fn generated_files_are_parsed_only_when_a_chunk_asks_for_them() {
    let base = [
        (
            ".gitattributes",
            "Lib/gen.py linguist-generated
",
        ),
        (
            "Lib/ast.py",
            "x = 1
",
        ),
        (
            "Lib/gen.py",
            "y = 1
",
        ),
    ];
    let head = [
        (
            "Lib/ast.py",
            "x = 2
",
        ),
        (
            "Lib/gen.py",
            "y = 2
",
        ),
    ];
    let repo = Repo::new("generated", &base, &head);
    let generated = [(1, Side::Base), (1, Side::Head)];
    let (mut worker, shelf) = worker(CAP);
    let diff = Arc::clone(&repo.source(7).diff);

    let ahead = diff.ahead();
    drop(ask(&mut worker, repo.source(7), &generated));

    assert_eq!(ahead, [(0, Side::Base), (0, Side::Head)]);
    assert_eq!(shelved(&shelf, &diff, &generated), generated);
    repo.finish();
}

#[test]
fn a_collapsed_file_s_rows_get_no_spans_until_the_window_opens_it() {
    let base = [
        (".gitattributes", "Lib/gen.py linguist-generated\n"),
        ("Lib/ast.py", "x = 1\n"),
        ("Lib/gen.py", "y = 1\n"),
    ];
    let head = [("Lib/ast.py", "x = 2\n"), ("Lib/gen.py", "y = 2\n")];
    let repo = Repo::new("collapsed", &base, &head);
    let highlighter = Highlighter::start();
    let diff = repo.source(7).diff;
    let rows = diff.diff.row_count();
    // Each file's header, hunk, removed line and added line.
    let painted = |spans: &[Vec<Span>]| {
        spans
            .iter()
            .map(|spans| !spans.is_empty())
            .collect::<Vec<_>>()
    };

    let chunk = |collapsed| Chunk {
        rows: 0..rows,
        collapsed,
    };
    // The window starts the Generated file collapsed, opens it, then collapses the other one.
    let generated = highlighter.spans(&diff, &repo.mirror, chunk(&[1]));
    let opened = highlighter.spans(&diff, &repo.mirror, chunk(&[]));
    let other = highlighter.spans(&diff, &repo.mirror, chunk(&[0]));

    let code = [false, false, true, true];
    assert_eq!(painted(&generated), [code, [false; 4]].concat());
    assert_eq!(painted(&opened), [code, code].concat());
    assert_eq!(painted(&other), [[false; 4], code].concat());
    repo.finish();
}

#[test]
fn a_chunk_that_asks_stops_the_parse_ahead_which_takes_it_up_again_after() {
    let repo = long("stops");
    let (mut worker, shelf) = worker(CAP);
    let diff = Arc::clone(&repo.source(7).diff);
    let both = [LONG_BASE, LONG_HEAD];
    drop(ask(&mut worker, repo.source(7), &[]));
    let incoming = waiting_for(repo.source(7), &[LONG_HEAD]);
    let mut waiting = VecDeque::new();

    let parsed = worker.parse_ahead(&incoming, &mut waiting);
    let stopped = shelved(&shelf, &diff, &both);
    let asked = waiting.len();
    worker.handle(waiting.pop_front().unwrap());
    let asked_for = shelved(&shelf, &diff, &both);
    assert!(worker.parse_ahead(&incoming, &mut waiting));

    assert!(parsed);
    assert_eq!(stopped, []);
    assert_eq!(asked, 1);
    assert_eq!(asked_for, [LONG_HEAD]);
    assert_eq!(shelved(&shelf, &diff, &both), both);
    repo.finish();
}

#[test]
fn a_chunk_that_asks_for_the_version_under_way_lets_its_parse_finish() {
    let repo = long("finishes");
    let (mut worker, shelf) = worker(CAP);
    let diff = Arc::clone(&repo.source(7).diff);
    drop(ask(&mut worker, repo.source(7), &[]));
    let incoming = waiting_for(repo.source(7), &[LONG_BASE]);
    let mut waiting = VecDeque::new();

    assert!(worker.parse_ahead(&incoming, &mut waiting));

    // The head comes next, which the chunk doesn't ask for, so the chunk goes first.
    assert_eq!(shelved(&shelf, &diff, &[LONG_BASE, LONG_HEAD]), [LONG_BASE]);
    assert_eq!(waiting.len(), 1);
    repo.finish();
}

#[test]
fn only_a_chunk_that_waits_for_a_tree_holds_the_parse_up() {
    let repo = changes("held");
    let used = |versions: Vec<Version>| Job::Used {
        from: repo.source(7),
        versions,
    };
    let diff = repo.source(7).diff;

    let waiting = VecDeque::from([used(vec![AST_HEAD])]);

    assert!(!held_up(&waiting, &diff, AST_HEAD));
    assert!(!held_up(&VecDeque::new(), &diff, AST_HEAD));
    repo.finish();
}

#[test]
fn the_open_pr_and_the_one_before_keep_their_trees_and_a_third_drops_the_older() {
    let repo = changes("prs");
    let (mut worker, shelf) = worker(CAP);
    let [one, two, three] = [1, 2, 3].map(|number| repo.source(number).diff);

    drop(ask(&mut worker, repo.source(1), &[AST_HEAD]));
    drop(ask(&mut worker, repo.source(2), &[AST_HEAD]));
    // Back to the first, as `Alt+←` goes, which finds its trees on the shelf.
    worker.handle(Job::Used {
        from: repo.source(1),
        versions: vec![AST_HEAD],
    });
    let both = (shelved(&shelf, &one, &ALL), shelved(&shelf, &two, &ALL));
    drop(ask(&mut worker, repo.source(3), &[NEW]));

    assert_eq!(both, (vec![AST_HEAD], vec![AST_HEAD]));
    assert_eq!(shelved(&shelf, &one, &ALL), [AST_HEAD]);
    assert_eq!(shelved(&shelf, &two, &ALL), []);
    assert_eq!(shelved(&shelf, &three, &ALL), [NEW]);
    let costs: usize = worker.prs.iter().map(Opened::cost).sum();
    assert_eq!(worker.store.kept, costs);
    repo.finish();
}

#[test]
fn at_the_cap_the_parse_ahead_stops_and_a_chunk_drops_the_trees_used_longest_ago() {
    let repo = changes("cap");
    // Room for one of these small trees, but not two.
    let (mut worker, shelf) = worker(150);
    let from = repo.source(7);
    let diff = Arc::clone(&from.diff);
    let (_jobs, incoming) = mpsc::channel();
    let mut waiting = VecDeque::new();

    drop(ask(&mut worker, from, &[C_HEAD]));
    let full = worker.parse_ahead(&incoming, &mut waiting);
    drop(ask(&mut worker, repo.source(7), &[C_BASE]));
    let after = shelved(&shelf, &diff, &ALL);
    let room = worker.parse_ahead(&incoming, &mut waiting);
    // A chunk that needs more than the cap keeps all it needs.
    drop(ask(&mut worker, repo.source(7), &[AST_BASE, AST_HEAD]));

    assert!(!full);
    assert_eq!(after, [C_BASE]);
    // Under the cap again, it still parses no more ahead.
    assert!(!room);
    assert_eq!(shelved(&shelf, &diff, &ALL), [AST_BASE, AST_HEAD]);
    assert!(worker.store.kept > worker.store.cap);
    repo.finish();
}

#[test]
fn trees_no_chunk_has_used_go_first_and_the_previous_pr_s_before_the_open_one_s() {
    let repo = changes("drops");
    let (mut worker, shelf) = worker(CAP);
    let [one, two] = [1, 2].map(|number| repo.source(number).diff);
    let (_jobs, incoming) = mpsc::channel();
    for number in [1, 2] {
        drop(ask(&mut worker, repo.source(number), &[]));
        assert!(worker.parse_ahead(&incoming, &mut VecDeque::new()));
    }
    worker.store.cap = worker.store.kept - 1;

    worker.handle(Job::Used {
        from: repo.source(2),
        versions: vec![C_HEAD],
    });
    let full = shelved(&shelf, &one, &ALL);
    drop(ask(&mut worker, repo.source(2), &[C_HEAD]));

    assert_eq!(full, ALL);
    assert_eq!(
        shelved(&shelf, &one, &ALL),
        [AST_HEAD, GONE, NEW, C_BASE, C_HEAD]
    );
    assert_eq!(shelved(&shelf, &two, &ALL), ALL);
    repo.finish();
}

#[test]
fn a_retargeted_pr_s_diff_keeps_trees_of_its_own() {
    let repo = changes("retargeted");
    let (mut worker, shelf) = worker(CAP);
    let other = CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
    let source = repo.source(7);
    let first = Arc::clone(&source.diff);
    let retargeted = Source {
        diff: Arc::new(OpenDiff {
            pr: pr(7, &repo.head),
            target: Some(("3.14".to_owned(), Start::TargetBranch)),
            base: other,
            diff: first.diff.clone(),
            news: Vec::new(),
        }),
        ..source
    };
    let second = Arc::clone(&retargeted.diff);
    drop(ask(&mut worker, repo.source(7), &[AST_BASE]));

    let trees = ask(&mut worker, retargeted, &[AST_BASE]);

    let (_jobs, incoming) = mpsc::channel();
    let parsed = worker.parse_ahead(&incoming, &mut VecDeque::new());

    // git has no such merge base, so its versions of the files show plain, for good.
    assert!(matches!(trees.get(&AST_BASE), Some(None)));
    assert!(parsed);
    for version in [AST_BASE, GONE, C_BASE] {
        assert!(
            matches!(shelf.get(&second, version), Some(None)),
            "{version:?}"
        );
    }
    assert_eq!(shelved(&shelf, &second, &ALL), [AST_HEAD, NEW, C_HEAD]);
    assert_eq!(shelved(&shelf, &first, &ALL), [AST_BASE]);
    repo.finish();
}

#[test]
fn a_git_that_fails_leaves_its_versions_for_a_chunk_to_ask_for_again() {
    let repo = changes("unreadable");
    let (mut worker, shelf) = worker(CAP);
    let from = Source {
        mirror: repo.root.join("no-mirror"),
        ..repo.source(7)
    };
    let diff = Arc::clone(&from.diff);
    let (_jobs, incoming) = mpsc::channel();

    let trees = ask(&mut worker, from, &[AST_HEAD]);
    let parsed = worker.parse_ahead(&incoming, &mut VecDeque::new());

    assert!(trees.is_empty());
    assert!(parsed);
    for version in ALL {
        assert!(shelf.get(&diff, version).is_none(), "{version:?}");
    }
    let again = ask(&mut worker, repo.source(7), &[AST_HEAD]);
    assert!(matches!(again.get(&AST_HEAD), Some(Some(_))));
    repo.finish();
}

#[test]
fn a_highlighter_whose_thread_has_stopped_shows_the_diff_plain() {
    let repo = changes("stopped");
    let (jobs, incoming) = mpsc::channel();
    drop(incoming);
    let highlighter = Highlighter {
        jobs,
        shelf: shelf().1,
    };

    let chunk = Chunk {
        rows: 0..100,
        collapsed: &[],
    };
    let spans = highlighter.spans(&repo.source(7).diff, &repo.mirror, chunk);

    assert!(spans.iter().all(Vec::is_empty));
    repo.finish();
}

#[test]
fn one_git_reads_sixteen_versions_ahead() {
    let paths: Vec<String> = (10..27).map(|n| format!("Lib/m{n}.py")).collect();
    let files: Vec<(&str, &str)> = paths.iter().map(|path| (&**path, "x = 1\n")).collect();
    let repo = Repo::new("batch", &[("README", "Hi\n")], &files);
    let (mut worker, shelf) = worker(CAP);
    let from = repo.source(7);
    let diff = Arc::clone(&from.diff);
    let versions: Vec<Version> = (0..17).map(|file| (file, Side::Head)).collect();
    let (_jobs, incoming) = mpsc::channel();
    let mut waiting = VecDeque::new();
    worker.handle(Job::Used {
        from,
        versions: Vec::new(),
    });

    assert!(worker.parse_ahead(&incoming, &mut waiting));
    assert_eq!(shelved(&shelf, &diff, &versions).len(), BATCH);
    assert!(worker.parse_ahead(&incoming, &mut waiting));
    assert_eq!(shelved(&shelf, &diff, &versions), versions);
    repo.finish();
}

#[test]
fn the_thread_parses_ahead_between_chunks_and_ends_with_the_last_highlighter() {
    let repo = changes("thread");
    let (worker, shelf) = worker(CAP);
    let diff = repo.source(7).diff;
    let (jobs, incoming) = mpsc::channel();
    jobs.send(Job::Used {
        from: repo.source(7),
        versions: Vec::new(),
    })
    .unwrap();
    drop(jobs);

    worker.run(&incoming);

    assert_eq!(shelved(&shelf, &diff, &ALL), ALL);
    repo.finish();
}

#[test]
fn git_s_batch_gives_each_blob_s_text_and_nothing_for_the_rest() {
    let output = b"1111 blob 3\nab\n\n\
        2222:Lib/x.py missing\n\
        3333 tree 4\n\0\0\0\0\n\
        4444:Lib/spaced name.py missing\n\
        5555 blob 20\ncut short";

    let blobs = parse_batch(output);

    let ab = Some("ab\n".to_owned());
    assert_eq!(blobs, [ab, None, None, None]);
}

#[test]
fn a_diff_with_no_file_tsugiki_highlights_asks_git_for_nothing() {
    let diff = Diff::parse("1\t1\tREADME\0", "diff --git a/README b/README\n").unwrap();
    let repo = changes("nothing");
    let open = OpenDiff {
        diff,
        ..Arc::into_inner(repo.source(7).diff).unwrap()
    };

    let read = open.read(Path::new("no-mirror"), &[(0, Side::Head)]);

    assert!(matches!(read.as_slice(), [((0, Side::Head), Read::Plain)]));
    assert!(blobs(Path::new("no-mirror"), &[]).is_empty());
    repo.finish();
}
