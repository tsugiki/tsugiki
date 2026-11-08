use super::*;
use crate::diff::tests::{Mirrored, git_in};
use crate::github::pull_request_content::{Side, Subject};
use crate::mirror::HeardRequests;
use crate::workbench::testing::{ViewWorld, mine};

/// A database and a mirror, where the PR's head and Target branch are those of `mirrored`.
struct World {
    view: ViewWorld,
    folders: Folders,
    diffs: Diffs,
    mirrors: MirrorRequests,
    heard: HeardRequests,
    highlighter: Highlighter,
}

impl World {
    fn new(name: &str) -> Self {
        let view = ViewWorld::new(&format!("pr-files-{name}"));
        let data = crate::testing::temp_path(&format!("pr-files-{name}-data"));
        std::fs::create_dir_all(&data).unwrap();
        let (mirrors, heard) = HeardRequests::new();
        Self {
            view,
            folders: Folders {
                data: data.clone(),
                settings: data,
            },
            diffs: Diffs::default(),
            mirrors,
            heard,
            highlighter: Highlighter::start(),
        }
    }

    fn sources(&self) -> DiffSources<'_> {
        DiffSources {
            reader: &self.view.reader,
            folders: &self.folders,
            diffs: &self.diffs,
            mirrors: &self.mirrors,
            highlighter: &self.highlighter,
        }
    }

    /// PR 7, at `head`, targeting `target`.
    fn has_pr(&self, head: Option<CommitSha>, target: &str) {
        let pr = PullRequestFacts {
            head,
            target_branch: target.to_owned(),
            ..mine(7)
        };
        self.view.has(vec![pr], vec![]);
    }

    /// The mirror of `mirrored`, where Tsugiki keeps CPython's.
    fn mirrors(&self, mirrored: &Mirrored) {
        let mirror = self.folders.mirror(&self.repository());
        std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        std::fs::rename(&mirrored.mirror, mirror).unwrap();
    }

    fn repository(&self) -> RepositoryName {
        self.view.settings().name
    }

    /// PR 7's diff from `base` to `head`.
    fn diff_of(&self, head: &CommitSha, base: &CommitSha) -> DiffOf {
        DiffOf {
            pr: PrAt {
                repository: self.repository(),
                number: 7,
                head: head.clone(),
            },
            base: base.clone(),
        }
    }

    fn files(&self) -> Option<PrFiles> {
        PrFiles::read(&self.sources(), &self.repository(), 7).unwrap()
    }

    fn finish(self) {
        std::fs::remove_dir_all(&self.folders.data).unwrap();
        self.view.finish();
    }
}

#[test]
fn a_pr_tsugiki_doesnt_hold_has_no_files() {
    let world = World::new("none");

    assert_eq!(world.files(), None);
    world.finish();
}

#[test]
fn before_its_mirror_is_cloned_a_pr_shows_the_clone() {
    let world = World::new("cloning");
    world.has_pr(None, "main");

    assert_eq!(world.files(), Some(PrFiles::Cloning));
    world.finish();
}

#[test]
fn a_pr_whose_head_tsugiki_hasnt_read_waits_for_the_sync() {
    let world = World::new("no-head");
    let mirrored = Mirrored::new("pr-files-no-head");
    world.mirrors(&mirrored);
    world.has_pr(None, "main");

    assert_eq!(world.files(), Some(PrFiles::Fetching));
    assert_eq!(world.heard.firsts(), []);
    mirrored.finish();
    world.finish();
}

#[test]
fn a_head_the_mirror_lacks_is_fetched_first() {
    let world = World::new("fetching");
    let mirrored = Mirrored::new("pr-files-fetching");
    world.mirrors(&mirrored);
    let elsewhere = CommitSha::parse(&"3".repeat(40)).unwrap();
    world.has_pr(Some(elsewhere.clone()), "main");

    assert_eq!(world.files(), Some(PrFiles::Fetching));
    let head = PrHead {
        number: 7,
        commit: elsewhere,
        target_branch: "main".to_owned(),
    };
    assert_eq!(world.heard.firsts(), [(world.repository(), head)]);
    mirrored.finish();
    world.finish();
}

#[test]
fn a_moved_head_shows_the_diff_before_with_its_threads_held_back_until_the_newer_comes() {
    let world = World::new("moved");
    let mirrored = Mirrored::new("pr-files-moved");
    let older = mirrored.head.clone();
    let whole = mirrored.read();
    let _ = git_in(
        &mirrored.mirror,
        &["update-ref", "refs/tsugiki/pull/7/head", older.as_str()],
    );
    world.mirrors(&mirrored);
    let newer = CommitSha::parse(&"3".repeat(40)).unwrap();
    world.has_pr(Some(newer.clone()), "main");
    let mut content = crate::workbench::pr_view::tests::reviewed();
    content.head = Some(newer.clone());
    let name = world.repository();
    world
        .view
        .store
        .save_pull_request_content(&name, 7, content)
        .unwrap();

    let expected = PrFiles::Ready {
        head: older,
        base: whole.base,
        files: whole.diff.files().to_vec(),
        rows: whole.diff.row_count(),
        news: whole.news,
        threads: DiffThreads::HeldBack {
            newer: newer.clone(),
            count: 2,
        },
    };
    assert_eq!(world.files(), Some(expected));
    let head = PrHead {
        number: 7,
        commit: newer,
        target_branch: "main".to_owned(),
    };
    assert_eq!(world.heard.firsts(), [(world.repository(), head)]);
    mirrored.finish();
    world.finish();
}

#[test]
fn a_target_branch_the_mirror_lacks_says_so() {
    let world = World::new("no-target");
    let mirrored = Mirrored::new("pr-files-no-target");
    world.mirrors(&mirrored);
    world.has_pr(Some(mirrored.head.clone()), "3.14");

    let problem = PrFiles::read(&world.sources(), &world.repository(), 7).unwrap_err();

    assert_eq!(
        problem.message,
        "The mirror of python/cpython has no branch 3.14, which this PR targets."
    );
    mirrored.finish();
    world.finish();
}

#[test]
fn a_head_in_the_mirror_shows_its_files_and_each_chunk_of_rows() {
    let world = World::new("ready");
    let mirrored = Mirrored::new("pr-files-ready");
    let head = mirrored.head.clone();
    let read = mirrored.read();
    let whole = read.diff;
    world.mirrors(&mirrored);
    world.has_pr(Some(head.clone()), "main");

    let files = world.files().unwrap();
    let of = world.diff_of(&head, &read.base);
    let rows = PrFiles::rows(&world.sources(), &of, 0..3).unwrap();
    // main marks Lib/ast.py generated, and the window has opened each file.
    let opened: Vec<u32> = (0..).take(whole.files().len()).collect();
    let all = 0..whole.row_count();
    let spans = PrFiles::spans(&world.sources(), &of, all, &opened).unwrap();

    let expected = PrFiles::Ready {
        head,
        base: read.base,
        files: whole.files().to_vec(),
        rows: whole.row_count(),
        news: read.news,
        threads: DiffThreads::Shown {
            threads: Vec::new(),
        },
    };
    assert_eq!(files, expected);
    assert_eq!(rows, whole.rows(0..3));
    assert_eq!(spans.len(), whole.rows(0..whole.row_count()).len());
    assert_eq!(spans.iter().filter(|spans| !spans.is_empty()).count(), 13);
    mirrored.finish();
    world.finish();
}

#[test]
fn a_thread_tsugiki_read_in_the_conversation_sits_under_its_line() {
    let world = World::new("threads");
    let mirrored = Mirrored::new("pr-files-threads");
    let head = mirrored.head.clone();
    let whole = mirrored.read().diff;
    world.mirrors(&mirrored);
    world.has_pr(Some(head.clone()), "main");
    let all = whole.rows(0..whole.row_count());
    let added = (0..).zip(all).find_map(|(row, line)| {
        if let DiffRow::Added { new, .. } = line {
            Some((row, *new))
        } else {
            None
        }
    });
    let (row, line) = added.unwrap();
    let file = whole.files().iter().rfind(|file| file.row < row).unwrap();
    let mut content = crate::workbench::pr_view::tests::reviewed();
    let thread = content.reviews[0].threads.get_mut(0).unwrap();
    thread.path.clone_from(&file.path);
    thread.subject = Subject::Line {
        side: Side::Right,
        line: Some(line),
        original: Some(line),
    };
    let name = world.repository();
    let save = |content| {
        world
            .view
            .store
            .save_pull_request_content(&name, 7, content)
            .unwrap();
    };
    // GitHub had another head when Tsugiki read the Conversation, so its lines count elsewhere.
    save(content.clone());
    let elsewhere = world.files();
    content.head = Some(head);
    save(content);

    // The threads, as the window reads them.
    let threads =
        |files: Option<PrFiles>| serde_json::to_value(files).unwrap()["threads"]["threads"].clone();
    let placed = threads(world.files());

    assert_eq!(
        (placed[0]["id"].clone(), placed[0]["at"].clone()),
        (
            serde_json::json!("TSome(12)"),
            serde_json::json!({ "kind": "row", "row": row })
        )
    );
    assert_eq!(
        placed[1]["at"]["kind"],
        serde_json::json!("outdated"),
        "the other thread is Outdated, at the top of its file"
    );
    assert_eq!(placed.as_array().map(Vec::len), Some(2));
    assert_eq!(threads(elsewhere), serde_json::json!([]));
    mirrored.finish();
    world.finish();
}

#[test]
fn the_lines_a_gap_leaves_out_come_from_the_head_highlighted_and_stop_at_its_end() {
    let world = World::new("lines");
    let mirrored = Mirrored::new("pr-files-lines");
    let head = mirrored.head.clone();
    let read = mirrored.read();
    world.mirrors(&mirrored);
    world.has_pr(Some(head.clone()), "main");
    let of = world.diff_of(&head, &read.base);
    let at = |path: &str| {
        let mut files = read.diff.files().iter();
        u32::try_from(files.position(|file| file.path == path).unwrap()).unwrap()
    };
    let lines = |file, range| PrFiles::lines(&world.sources(), &of, file, range).unwrap();

    let first = lines(at("Lib/ast.py"), 1..3);
    let last = lines(at("Lib/ast.py"), 10..30);
    let plain = lines(at("Doc/news.txt"), 2..3);
    let unknown = lines(99, 1..3);

    let texts = |lines: &[HeadLine]| {
        lines
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&first), ["x1 = 1", "x2 = 2"]);
    assert!(first.iter().all(|line| !line.spans.is_empty()));
    assert_eq!(texts(&last), ["x10 = 10", "x11 = 11"]);
    assert_eq!(
        plain,
        [HeadLine {
            text: "line 2".to_owned(),
            spans: Vec::new()
        }]
    );
    assert_eq!(unknown, []);
    mirrored.finish();
    world.finish();
}

#[test]
fn the_diff_on_show_answers_without_asking_git_again() {
    let world = World::new("on-show");
    let mirrored = Mirrored::new("pr-files-on-show");
    world.mirrors(&mirrored);
    world.has_pr(Some(mirrored.head.clone()), "main");
    let first = world.files();
    let mirror = world.folders.mirror(&world.repository());

    // git would now say the mirror has no Target branch.
    let _ = git_in(&mirror, &["update-ref", "-d", "refs/heads/main"]);

    assert_eq!(world.files(), first);
    mirrored.finish();
    world.finish();
}

#[test]
fn rows_of_a_diff_no_longer_on_show_are_read_again_from_its_base() {
    let world = World::new("read-again");
    let mirrored = Mirrored::new("pr-files-read-again");
    let head = mirrored.head.clone();
    let whole = mirrored.read();
    let _ = git_in(&mirrored.mirror, &["branch", "3.14", head.as_str()]);
    world.mirrors(&mirrored);
    // The PR moved to 3.14 after the window read its files against main.
    world.has_pr(Some(head.clone()), "3.14");
    let of = world.diff_of(&head, &whole.base);
    let elsewhere = DiffOf {
        pr: PrAt {
            number: 8,
            ..of.pr.clone()
        },
        ..of.clone()
    };

    let rows = PrFiles::rows(&world.sources(), &of, 3..5).unwrap();
    let unknown = PrFiles::rows(&world.sources(), &elsewhere, 0..5).unwrap_err();

    assert_eq!(rows, whole.diff.rows(3..5));
    assert_eq!(unknown.message, "Tsugiki doesn't hold PR #8.");
    mirrored.finish();
    world.finish();
}

#[test]
fn a_pr_moved_to_another_branch_shows_its_diff_against_that_branch() {
    let world = World::new("retargeted");
    let mirrored = Mirrored::new("pr-files-retargeted");
    let head = mirrored.head.clone();
    // 3.14 already holds the PR's commit, so the diff against it is empty.
    let _ = git_in(&mirrored.mirror, &["branch", "3.14", head.as_str()]);
    world.mirrors(&mirrored);
    world.has_pr(Some(head.clone()), "main");
    let before = world.files();

    world.has_pr(Some(head.clone()), "3.14");

    let expected = PrFiles::Ready {
        head: head.clone(),
        base: head,
        files: Vec::new(),
        rows: 0,
        news: Vec::new(),
        threads: DiffThreads::Shown {
            threads: Vec::new(),
        },
    };
    assert_ne!(before, Some(expected.clone()));
    assert_eq!(world.files(), Some(expected));
    mirrored.finish();
    world.finish();
}
