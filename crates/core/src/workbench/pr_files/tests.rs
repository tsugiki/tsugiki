use std::path::Path;

use tsugiki_test_git::git;

use super::*;
use crate::diff::MirrorDiff;
use crate::diff::testing::Mirrored;
use crate::facts::pull_request_content::{Side, Subject, ThreadCommentId};
use crate::facts::pull_requests::PrHead;
use crate::workbench::testing::{ViewWorld, mine};

/// A database and a mirror, where the PR's head and Target branch are those of `mirrored`.
struct World {
    view: ViewWorld,
    folders: Folders,
    diffs: Diffs,
    highlighter: Highlighter,
    /// The data folder, dropped last.
    _data: crate::testing::TempPath,
}

impl World {
    fn new(name: &str) -> Self {
        let view = ViewWorld::new(&format!("pr-files-{name}"));
        let data = crate::testing::temp_path(&format!("pr-files-{name}-data"));
        std::fs::create_dir_all(&data).unwrap();
        Self {
            view,
            folders: Folders {
                data: data.to_path_buf(),
                settings: data.to_path_buf(),
                logs: data.to_path_buf(),
            },
            diffs: Diffs::default(),
            highlighter: Highlighter::start(),
            _data: data,
        }
    }

    fn sources(&self) -> DiffSources<'_> {
        DiffSources {
            reader: &self.view.db.reader,
            folders: &self.folders,
            diffs: &self.diffs,
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

    /// PR 7, merged into main as `merge_commit`, from `head`. GitHub compared it with `base` at the
    /// merge, and a PR stored before Tsugiki read that has none.
    fn has_merged(&self, head: &CommitSha, merge_commit: CommitSha, base: Option<CommitSha>) {
        let pr = PullRequestFacts {
            state: PrState::Merged,
            merge_commit: Some(merge_commit),
            base,
            head: Some(head.clone()),
            target_branch: "main".to_owned(),
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

    /// What a read of PR 7's Files tab finds, while its clone doesn't wait for an answer.
    fn read(&self) -> Option<FilesRead> {
        PrFiles::read(&self.sources(), &self.repository(), 7, &|| false).unwrap()
    }

    fn files(&self) -> Option<PrFiles> {
        self.read().map(|read| read.files)
    }

    /// What the mirror holds of PR 7's head, as a read of its Files tab finds.
    fn head(&self) -> Option<HeadInMirror> {
        self.read().and_then(|read| read.head)
    }

    fn head_in_mirror(&self) -> Option<HeadInMirror> {
        PrFiles::head_in_mirror(&self.sources(), &self.repository(), 7).unwrap()
    }

    fn finish(self) {
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
    assert_eq!(world.head(), None, "no mirror to look in");
    world.finish();
}

#[test]
fn before_the_user_says_where_its_mirror_gets_its_objects_a_pr_asks() {
    let world = World::new("asking");
    world.has_pr(None, "main");

    let read = PrFiles::read(&world.sources(), &world.repository(), 7, &|| true).unwrap();

    assert_eq!(read.map(|read| read.files), Some(PrFiles::Asking));
    world.finish();
}

#[test]
fn a_pr_whose_head_tsugiki_hasnt_read_waits_for_the_sync() {
    let world = World::new("no-head");
    let mirrored = Mirrored::new("pr-files-no-head");
    world.mirrors(&mirrored);
    world.has_pr(None, "main");

    assert_eq!(world.files(), Some(PrFiles::Fetching));
    assert_eq!(world.head(), Some(HeadInMirror::Unknown));
    world.finish();
}

/// PR 7's head at a commit no mirror holds.
fn elsewhere() -> PrHead {
    PrHead {
        number: 7,
        commit: CommitSha::parse(&"3".repeat(40)).unwrap(),
        target_branch: "main".to_owned(),
    }
}

#[test]
fn a_head_the_mirror_lacks_is_missing() {
    let world = World::new("fetching");
    let mirrored = Mirrored::new("pr-files-fetching");
    world.mirrors(&mirrored);
    world.has_pr(Some(elsewhere().commit), "main");

    assert_eq!(world.files(), Some(PrFiles::Fetching));
    assert_eq!(world.head(), Some(HeadInMirror::Missing(elsewhere())));
    world.finish();
}

#[test]
fn opening_a_pr_finds_a_head_the_mirror_lacks_without_reading_a_diff() {
    let world = World::new("open-fetching");
    let mirrored = Mirrored::new("pr-files-open-fetching");
    world.mirrors(&mirrored);
    world.has_pr(Some(elsewhere().commit), "main");

    assert_eq!(
        world.head_in_mirror(),
        Some(HeadInMirror::Missing(elsewhere()))
    );
    world.finish();
}

#[test]
fn opening_a_pr_finds_the_head_the_mirror_holds_and_reads_no_diff() {
    let world = World::new("open-held");
    let mirrored = Mirrored::new("pr-files-open-held");
    world.mirrors(&mirrored);
    world.has_pr(Some(mirrored.head.clone()), "main");

    assert_eq!(world.head_in_mirror(), Some(HeadInMirror::Held));
    let at = PrAt {
        repository: world.repository(),
        number: 7,
        head: mirrored.head.clone(),
    };
    let target = Against::Branch("main", &Start::TargetBranch);
    assert!(world.diffs.shown(&at, target).is_none());
    world.finish();
}

#[test]
fn opening_a_pr_whose_head_tsugiki_hasnt_read_finds_it_unknown() {
    let world = World::new("open-no-head");
    let mirrored = Mirrored::new("pr-files-open-no-head");
    world.mirrors(&mirrored);
    world.has_pr(None, "main");

    assert_eq!(world.head_in_mirror(), Some(HeadInMirror::Unknown));
    world.finish();
}

#[test]
fn opening_a_pr_tsugiki_doesnt_hold_finds_nothing() {
    let world = World::new("open-none");

    assert_eq!(world.head_in_mirror(), None);
    world.finish();
}

#[test]
fn opening_a_pr_before_its_mirror_is_cloned_finds_nothing() {
    let world = World::new("open-cloning");
    world.has_pr(Some(elsewhere().commit), "main");

    assert_eq!(world.head_in_mirror(), None);
    world.finish();
}

#[test]
fn a_moved_head_shows_the_diff_before_with_its_threads_held_back_until_the_newer_comes() {
    let world = World::new("moved");
    let mirrored = Mirrored::new("pr-files-moved");
    let older = mirrored.head.clone();
    let whole = mirrored.read();
    let _ = git(
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
        .db
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
    let head = PrHead {
        number: 7,
        commit: newer,
        target_branch: "main".to_owned(),
    };
    let found = FilesRead {
        files: expected,
        head: Some(HeadInMirror::Missing(head)),
    };
    assert_eq!(world.read(), Some(found));
    world.finish();
}

#[test]
fn the_files_tab_gives_the_diffs_merge_base_and_head_read_and_on_show_with_the_head_held() {
    let world = World::new("look-up");
    let mirrored = Mirrored::new("pr-files-look-up");
    let whole = mirrored.read();
    world.mirrors(&mirrored);
    world.has_pr(Some(mirrored.head.clone()), "main");

    let read = world.read().unwrap();
    let shown = world.read().unwrap();

    assert_eq!(read, shown);
    assert_eq!(read.head, Some(HeadInMirror::Held));
    let PrFiles::Ready { head, base, .. } = read.files else {
        panic!("no diff in {read:?}");
    };
    assert_eq!((head, base), (mirrored.head.clone(), whole.base));
    world.finish();
}

#[test]
fn a_moved_head_whose_older_diff_has_no_target_branch_waits_for_the_newer() {
    let world = World::new("moved-no-target");
    let mirrored = Mirrored::new("pr-files-moved-no-target");
    let _ = git(
        &mirrored.mirror,
        &[
            "update-ref",
            "refs/tsugiki/pull/7/head",
            mirrored.head.as_str(),
        ],
    );
    world.mirrors(&mirrored);
    world.has_pr(Some(CommitSha::parse(&"3".repeat(40)).unwrap()), "3.14");

    assert_eq!(world.files(), Some(PrFiles::Fetching));
    world.finish();
}

#[test]
fn a_target_branch_the_mirror_lacks_says_so() {
    let world = World::new("no-target");
    let mirrored = Mirrored::new("pr-files-no-target");
    world.mirrors(&mirrored);
    world.has_pr(Some(mirrored.head.clone()), "3.14");

    let problem = PrFiles::read(&world.sources(), &world.repository(), 7, &|| false).unwrap_err();

    assert_eq!(
        problem.message,
        "The mirror of python/cpython has no branch 3.14, which this PR targets."
    );
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
    // main marks Lib/ast.py generated, and the window has collapsed no file.
    let all = 0..whole.row_count();
    let spans = PrFiles::spans(&world.sources(), &of, all, &[]).unwrap();

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
    let numbered = thread.comments[0].id.map(ThreadCommentId::get).unwrap();
    let name = world.repository();
    let save = |content| {
        world
            .view
            .db
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
    assert_eq!(
        placed[0]["comments"][0]["id"],
        serde_json::json!(numbered),
        "a link names a thread's comment by GitHub's number"
    );
    assert_eq!(threads(elsewhere), serde_json::json!([]));
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
    let _ = git(&mirror, &["update-ref", "-d", "refs/heads/main"]);

    assert_eq!(world.files(), first);
    world.finish();
}

#[test]
fn rows_of_a_diff_no_longer_on_show_are_read_again_from_its_base() {
    let world = World::new("read-again");
    let mirrored = Mirrored::new("pr-files-read-again");
    let head = mirrored.head.clone();
    let whole = mirrored.read();
    let _ = git(&mirrored.mirror, &["branch", "3.14", head.as_str()]);
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
    world.finish();
}

#[test]
fn a_line_has_its_row_or_its_line_at_the_head_where_no_hunk_shows_it() {
    let world = World::new("line-row");
    let mirrored = Mirrored::new("pr-files-line-row");
    let head = mirrored.head.clone();
    let whole = mirrored.read();
    world.mirrors(&mirrored);
    world.has_pr(Some(head.clone()), "main");
    let of = world.diff_of(&head, &whole.base);
    let row = |path: &str, side, line| {
        let line = FileLine {
            path: path.to_owned(),
            side,
            line,
        };
        PrFiles::row(&world.sources(), &of, &line).unwrap()
    };
    // The row of the diff that shows `wanted`.
    let showing = |wanted: DiffRow| {
        (0..whole.diff.row_count())
            .find(|&row| whole.diff.rows(row..row + 1).first() == Some(&wanted))
            .map(|row| LineInDiff::Row { row })
    };
    let news = (0_u32..)
        .zip(whole.diff.files())
        .find_map(|(index, file)| (file.path == "Doc/news.txt").then_some(index))
        .unwrap();

    // The PR renames Doc/notes.txt to Doc/news.txt, and changes its line 20.
    let removed = row("Doc/notes.txt", DiffEnd::Base, 20);
    let added = row("Doc/news.txt", DiffEnd::Head, 20);
    let context = row("Doc/news.txt", DiffEnd::Head, 18);
    let hidden_at_head = row("Doc/news.txt", DiffEnd::Head, 5);
    let hidden_at_base = row("Doc/notes.txt", DiffEnd::Base, 4);
    let old_name_at_head = row("Doc/notes.txt", DiffEnd::Head, 20);
    let unchanged = row("README.rst", DiffEnd::Base, 1);

    let removed_row = DiffRow::Removed {
        old: 20,
        text: "line 20".to_owned(),
    };
    assert_eq!(removed, showing(removed_row));
    let added_row = DiffRow::Added {
        new: 20,
        text: "line twenty".to_owned(),
    };
    assert_eq!(added, showing(added_row));
    let context_row = DiffRow::Context {
        old: 18,
        new: 18,
        text: "line 18".to_owned(),
    };
    assert_eq!(context, showing(context_row));
    // The gap above the hunk hides lines 1 to 16, the same at either end.
    let hidden = |head_line| LineInDiff::Hidden {
        file: news,
        head_line,
    };
    assert_eq!(hidden_at_head, Some(hidden(5)));
    assert_eq!(hidden_at_base, Some(hidden(4)));
    assert_eq!(old_name_at_head, None);
    assert_eq!(unchanged, None);
    world.finish();
}

#[test]
fn a_pr_moved_to_another_branch_shows_its_diff_against_that_branch() {
    let world = World::new("retargeted");
    let mirrored = Mirrored::new("pr-files-retargeted");
    let head = mirrored.head.clone();
    // 3.14 already holds the PR's commit, so the diff against it is empty.
    let _ = git(&mirrored.mirror, &["branch", "3.14", head.as_str()]);
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
    world.finish();
}

/// The commit main is at in `mirror`.
fn main_tip(mirror: &Path) -> CommitSha {
    CommitSha::parse(git(mirror, &["rev-parse", "main"]).trim()).unwrap()
}

/// Moves main in `mirror` to a new commit with `parents` and main's files, as a merge does, and gives it.
fn commit_onto_main(mirror: &Path, parents: &[&CommitSha]) -> CommitSha {
    let mut args = vec!["commit-tree", "main^{tree}", "-m", "Merged"];
    for parent in parents {
        args.extend(["-p", parent.as_str()]);
    }
    let commit = CommitSha::parse(git(mirror, &args).trim()).unwrap();
    let _ = git(mirror, &["update-ref", "refs/heads/main", commit.as_str()]);
    commit
}

/// What the Files tab shows of the diff `read` at `head`, with no threads.
fn ready(head: CommitSha, read: MirrorDiff) -> PrFiles {
    PrFiles::Ready {
        head,
        base: read.base,
        files: read.diff.files().to_vec(),
        rows: read.diff.row_count(),
        news: read.news,
        threads: DiffThreads::Shown {
            threads: Vec::new(),
        },
    }
}

#[test]
fn a_pr_merged_with_a_merge_commit_shows_the_diff_it_had() {
    let world = World::new("merge-commit");
    let mirrored = Mirrored::new("pr-files-merge-commit");
    let head = mirrored.head.clone();
    let before = mirrored.read();
    let base = main_tip(&mirrored.mirror);
    let merge_commit = commit_onto_main(&mirrored.mirror, &[&base, &head]);
    world.mirrors(&mirrored);

    world.has_merged(&head, merge_commit, Some(base));

    assert_eq!(world.files(), Some(ready(head, before)));
    world.finish();
}

#[test]
fn a_pr_merged_by_fast_forward_shows_the_diff_from_where_main_was() {
    let world = World::new("fast-forward");
    let mirrored = Mirrored::new("pr-files-fast-forward");
    let before = mirrored.read();
    // The PR merges main in, and main then moves to its head.
    let base = main_tip(&mirrored.mirror);
    let tree = git(
        &mirrored.mirror,
        &["merge-tree", "--write-tree", "main", mirrored.head.as_str()],
    );
    let args = ["commit-tree", tree.trim(), "-m", "Merged main"];
    let parents = ["-p", base.as_str(), "-p", mirrored.head.as_str()];
    let head =
        CommitSha::parse(git(&mirrored.mirror, &[&args[..], &parents].concat()).trim()).unwrap();
    let _ = git(
        &mirrored.mirror,
        &["update-ref", "refs/heads/main", head.as_str()],
    );
    world.mirrors(&mirrored);

    world.has_merged(&head, head.clone(), Some(base.clone()));

    let expected = ready(head, MirrorDiff { base, ..before });
    assert_eq!(world.files(), Some(expected));
    world.finish();
}

#[test]
fn a_merged_pr_stored_before_tsugiki_read_its_base_starts_from_its_merge_commits_parent() {
    let world = World::new("merged-before");
    let mirrored = Mirrored::new("pr-files-merged-before");
    let head = mirrored.head.clone();
    let before = mirrored.read();
    let main = main_tip(&mirrored.mirror);
    let merge_commit = commit_onto_main(&mirrored.mirror, &[&main, &head]);
    world.mirrors(&mirrored);

    world.has_merged(&head, merge_commit, None);

    assert_eq!(world.files(), Some(ready(head, before)));
    world.finish();
}

#[test]
fn a_squashed_pr_stored_before_tsugiki_read_its_base_shows_its_diff() {
    let world = World::new("squashed-before");
    let mirrored = Mirrored::new("pr-files-squashed-before");
    let head = mirrored.head.clone();
    let before = mirrored.read();
    let main = main_tip(&mirrored.mirror);
    let squashed = commit_onto_main(&mirrored.mirror, &[&main]);
    world.mirrors(&mirrored);

    world.has_merged(&head, squashed, None);

    assert_eq!(world.files(), Some(ready(head, before)));
    world.finish();
}

#[test]
fn a_merge_the_mirror_hasnt_fetched_yet_shows_the_diff_against_the_target_branch() {
    let world = World::new("merge-unfetched");
    let mirrored = Mirrored::new("pr-files-merge-unfetched");
    let head = mirrored.head.clone();
    let before = mirrored.read();
    world.mirrors(&mirrored);
    // main moved on and took the merge after the mirror last fetched it.
    let base = CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
    let merge_commit = CommitSha::parse("456789abcdef0123456789abcdef0123456789ab").unwrap();

    world.has_merged(&head, merge_commit.clone(), Some(base));
    let with_base = world.files();
    world.has_merged(&head, merge_commit, None);

    assert_eq!(with_base, Some(ready(head.clone(), before)));
    assert_eq!(world.files(), with_base);
    world.finish();
}

#[test]
fn a_pr_that_merges_while_its_diff_shows_starts_where_github_compared_it() {
    let world = World::new("merges-on-show");
    let mirrored = Mirrored::new("pr-files-merges-on-show");
    let head = mirrored.head.clone();
    let before = mirrored.read();
    let base = main_tip(&mirrored.mirror);
    let merge_commit = commit_onto_main(&mirrored.mirror, &[&base, &head]);
    world.mirrors(&mirrored);
    // The mirror has the merge before the sync learns of it, so the open PR's diff is empty.
    world.has_pr(Some(head.clone()), "main");
    let open = world.files();

    world.has_merged(&head, merge_commit, Some(base));

    assert!(matches!(open, Some(PrFiles::Ready { files, .. }) if files.is_empty()));
    assert_eq!(world.files(), Some(ready(head, before)));
    world.finish();
}

#[test]
fn a_fast_forward_stored_before_tsugiki_read_its_base_has_nothing_to_show() {
    let world = World::new("fast-forward-before");
    let mirrored = Mirrored::new("pr-files-fast-forward-before");
    let head = mirrored.head.clone();
    let _ = git(
        &mirrored.mirror,
        &["update-ref", "refs/heads/main", head.as_str()],
    );
    world.mirrors(&mirrored);

    world.has_merged(&head, head.clone(), None);

    assert!(matches!(world.files(), Some(PrFiles::Ready { files, .. }) if files.is_empty()));
    world.finish();
}
