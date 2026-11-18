use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use tsugiki_test_git::{Snapshot, git};

use super::*;
use crate::patch_ids::{ChangedFile, changed_files};
use crate::testing::import::{Change, Commit, Mode, import};
use crate::testing::{TempPath, add_config};

/// A mirror with a branch `main` and the PRs on it that a test writes, read as the Mirror worker reads one.
/// Each commit is one `git fast-import`, and a merge or a rebase writes the tree git would have made.
struct Place {
    mirror: PathBuf,
    /// main's first commit.
    first: CommitSha,
    _root: TempPath,
}

/// The mirror [`Place::new`] copies, and main's first commit in it. Two gits build it, once in a test run.
static BUILT: OnceLock<(Snapshot, CommitSha)> = OnceLock::new();

impl Place {
    fn new(name: &str) -> Self {
        let (snapshot, first) = BUILT.get_or_init(|| {
            let root = crate::testing::temp_path("patch-ids-built");
            let mirror = root.join("mirror.git");
            fs::create_dir_all(&root).unwrap();
            let _ = git(&root, &["init", "-q", "--bare", "-b", "main", "mirror.git"]);
            let (ast, os) = (lines(1..=20), lines(1..=20));
            let changes = [
                file("Lib/ast.py", &ast),
                file("Lib/os.py", &os),
                file("README.rst", "Read me\n"),
            ];
            let first = Commit {
                branch: "main",
                message: "One",
                parents: &[],
                changes: &changes,
            };
            let commits = import(&mirror, &[first]);
            (Snapshot::of(&mirror), commits[0].clone())
        });
        let root = crate::testing::temp_path(&format!("patch-ids-{name}"));
        let mirror = root.join("mirror.git");
        snapshot.write(&mirror);
        Self {
            mirror,
            first: first.clone(),
            _root: root,
        }
    }

    /// Writes a commit on `branch` after `parents` with `changes`, and gives it.
    fn commit(&self, branch: &str, parents: &[&CommitSha], changes: &[Change<'_>]) -> CommitSha {
        let commit = Commit {
            branch,
            message: branch,
            parents,
            changes,
        };
        import(&self.mirror, &[commit]).remove(0)
    }

    /// The patch-ids of `commit` against `main`.
    fn patches(&self, commit: &CommitSha) -> Vec<FilePatch> {
        read_patch_ids(&self.mirror, "main", commit).unwrap()
    }
}

/// Writes `text` at `path`.
fn file<'a>(path: &'a str, text: &'a str) -> Change<'a> {
    Change::Write(Mode::File, path, text.as_bytes())
}

fn lines(numbers: std::ops::RangeInclusive<u32>) -> String {
    numbers.map(|n| format!("x{n} = {n}\n")).collect()
}

/// `text` with line 5 changed, as the PR changes `Lib/ast.py`.
fn as_the_pr_has_it(text: &str) -> String {
    text.replace("x5 = 5", "x5 = 'five'")
}

/// `text` with line 18 changed, as main changes `Lib/ast.py` after the PR started.
fn as_main_has_it(text: &str) -> String {
    text.replace("x18 = 18", "x18 = 'eighteen'")
}

/// `Lib/ast.py` as the PR has it.
fn ast_as_the_pr_has_it() -> String {
    as_the_pr_has_it(&lines(1..=20))
}

/// `Lib/ast.py` with both changes, as a merge or a rebase of the PR onto main leaves it.
fn ast_with_both() -> String {
    as_main_has_it(&ast_as_the_pr_has_it())
}

/// What [`main_moves_on`] changes, with `ast` as `Lib/ast.py`.
fn main_changes(ast: &str) -> [Change<'_>; 2] {
    [
        file("README.rst", "Read me first\n"),
        file("Lib/ast.py", ast),
    ]
}

/// A PR that changes line 5 of `Lib/ast.py` from main's first commit, and gives its commit.
fn reviewed_pr(place: &Place) -> CommitSha {
    let ast = ast_as_the_pr_has_it();
    place.commit("pr", &[&place.first], &[file("Lib/ast.py", &ast)])
}

/// main moves on in a file the PR doesn't touch, and in another part of the one it does. Gives main.
fn main_moves_on(place: &Place) -> CommitSha {
    let ast = as_main_has_it(&lines(1..=20));
    place.commit("main", &[&place.first], &main_changes(&ast))
}

/// The PR at `pr` merges main at `main`, as [`main_moves_on`] left it. Gives the merge.
fn merge_main(place: &Place, pr: &CommitSha, main: &CommitSha) -> CommitSha {
    let ast = ast_with_both();
    place.commit("pr", &[pr, main], &main_changes(&ast))
}

#[test]
fn each_file_the_diff_changes_gets_a_patch_id_by_its_path() {
    let place = Place::new("each-file");
    let reviewed = reviewed_pr(&place);

    let patches = place.patches(&reviewed);

    assert_eq!(
        patches
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["Lib/ast.py"]
    );
}

#[test]
fn a_merge_from_the_target_branch_leaves_each_files_patch_id_alone() {
    let place = Place::new("merge");
    let reviewed = reviewed_pr(&place);
    let main = main_moves_on(&place);
    let merged = merge_main(&place, &reviewed, &main);

    let (before, now) = (place.patches(&reviewed), place.patches(&merged));

    assert_eq!(changed_files(&before, &now), []);
}

#[test]
fn a_rebase_that_leaves_each_files_diff_the_same_leaves_its_patch_id_alone() {
    let place = Place::new("rebase");
    let reviewed = reviewed_pr(&place);
    let main = main_moves_on(&place);
    let ast = ast_with_both();
    // A branch of its own, as fast-import moves a branch only to a commit that descends from it.
    let rebased = place.commit("rebased", &[&main], &[file("Lib/ast.py", &ast)]);

    let (before, now) = (place.patches(&reviewed), place.patches(&rebased));

    assert_ne!(reviewed, rebased);
    assert_eq!(changed_files(&before, &now), []);
}

#[test]
fn a_new_change_to_a_file_changes_its_patch_id() {
    let place = Place::new("new-change");
    let reviewed = reviewed_pr(&place);
    let (ast, os) = (
        lines(1..=20).replace("x5 = 5", "x5 = 'FIVE'"),
        lines(1..=21),
    );
    let changes = [file("Lib/ast.py", &ast), file("Lib/os.py", &os)];
    let pushed = place.commit("pr", &[&reviewed], &changes);

    let (before, now) = (place.patches(&reviewed), place.patches(&pushed));

    assert_eq!(
        changed_files(&before, &now),
        [
            ChangedFile {
                path: "Lib/ast.py".to_owned(),
                whitespace_only: false,
            },
            ChangedFile {
                path: "Lib/os.py".to_owned(),
                whitespace_only: false,
            },
        ]
    );
}

#[test]
fn trailing_spaces_change_the_verbatim_patch_id_only() {
    let place = Place::new("whitespace");
    let reviewed = reviewed_pr(&place);
    let ast = lines(1..=20).replace("x5 = 5", "x5 = 'five'  ");
    let pushed = place.commit("pr", &[&reviewed], &[file("Lib/ast.py", &ast)]);

    let (before, now) = (place.patches(&reviewed), place.patches(&pushed));

    assert_eq!(
        changed_files(&before, &now),
        [ChangedFile {
            path: "Lib/ast.py".to_owned(),
            whitespace_only: true,
        }]
    );
}

#[test]
fn a_binary_files_patch_id_follows_its_content() {
    let place = Place::new("binary");
    let reviewed = place.commit("pr", &[&place.first], &[file("logo.png", "\u{0}one")]);
    let pushed = place.commit("pr", &[&reviewed], &[file("logo.png", "\u{0}two")]);

    let (before, now) = (place.patches(&reviewed), place.patches(&pushed));

    assert_eq!(
        changed_files(&before, &now),
        [ChangedFile {
            path: "logo.png".to_owned(),
            whitespace_only: false,
        }]
    );
}

#[test]
fn the_attributes_on_the_target_branch_say_which_files_are_generated() {
    let place = Place::new("generated");
    let reviewed = reviewed_pr(&place);
    let attributes = file(".gitattributes", "Lib/ast.py linguist-generated\n");
    let _main = place.commit("main", &[&place.first], &[attributes]);

    let patches = place.patches(&reviewed);

    assert_eq!(
        patches
            .iter()
            .map(|file| (file.path.as_str(), file.generated))
            .collect::<Vec<_>>(),
        [("Lib/ast.py", true)]
    );
}

#[test]
fn a_file_turned_into_a_symlink_or_a_submodule_a_rename_and_a_mode_change_each_get_a_patch_id() {
    let place = Place::new("typechange");
    let first = &place.first;
    // `Lib/ast.py` holds this too, which the PR makes executable.
    let text = lines(1..=20);
    let main = place.commit("main", &[first], &[file("Lib/abc.py", &text)]);
    let changes = [
        Change::Rename("Lib/os.py", "Lib/posix.py"),
        Change::Write(Mode::Executable, "Lib/ast.py", text.as_bytes()),
        Change::Write(Mode::Symlink, "README.rst", b"Lib/ast.py"),
        Change::Submodule("Lib/abc.py", first),
    ];
    let pr = place.commit("pr", &[&main], &changes);

    let read = read_patch_ids(&place.mirror, "main", &pr);

    let paths: Vec<String> = read.unwrap().into_iter().map(|file| file.path).collect();
    assert_eq!(
        paths,
        ["Lib/abc.py", "Lib/ast.py", "Lib/posix.py", "README.rst"]
    );
}

#[test]
fn the_users_diff_settings_leave_the_patch_ids_alone() {
    let place = Place::new("settings");
    let changed = ast_as_the_pr_has_it().replace("x12 = 12", "x12 = 'twelve'");
    let changes = [
        file("Lib/ast.py", &changed),
        file("Lib/\u{fc}nicode.py", "\nx = 1\n\ny = 2\n"),
    ];
    let pr = place.commit("pr", &[&place.first], &changes);
    let plain = place.patches(&pr);

    // git reads the mirror's own config as it reads the user's.
    let settings = "[diff]\n\
        \tnoprefix = true\n\
        \tmnemonicPrefix = true\n\
        \tsrcPrefix = old/\n\
        \tdstPrefix = new/\n\
        \talgorithm = histogram\n\
        \tindentHeuristic = false\n\
        \tinterHunkContext = 10\n\
        \tcontext = 10\n\
        \tsuppressBlankEmpty = true\n\
        \trenames = false\n\
        \trelative = true\n\
        \texternal = false\n\
        \tignoreSubmodules = all\n\
        [core]\n\
        \tquotePath = false\n";
    add_config(&place.mirror, settings);
    let configured = place.patches(&pr);

    assert_eq!(configured, plain);
}

#[test]
fn a_commit_already_on_the_target_branch_gives_no_patch_ids_to_keep() {
    let place = Place::new("level");

    // A commit on the Target branch that no merge commit brought in has an empty diff from there, which
    // would read as every file changed for good.
    let read = read_patch_ids(&place.mirror, "main", &place.first);

    assert!(read.is_err());
}

#[test]
fn a_pr_merged_with_a_merge_commit_keeps_the_patch_ids_it_had_while_open() {
    let place = Place::new("merge-commit");
    let reviewed = reviewed_pr(&place);
    let two = main_moves_on(&place);
    let merged_main = merge_main(&place, &reviewed, &two);
    let os = lines(1..=20).replace("x3 = 3", "x3 = 'three'");
    let head = place.commit("pr", &[&merged_main], &[file("Lib/os.py", &os)]);
    let open = [place.patches(&reviewed), place.patches(&head)];
    let three = place.commit("main", &[&two], &[file("README.rst", "Read me again\n")]);
    // main keeps its README, and takes the PR's other changes.
    let ast = ast_with_both();
    let changes = [file("Lib/ast.py", &ast), file("Lib/os.py", &os)];
    let merge = place.commit("main", &[&three, &head], &changes);
    let _four = place.commit("main", &[&merge], &[file("README.rst", "Read me last\n")]);

    // Both commits are on main now, and their merge base with it is themselves.
    let merged = [place.patches(&reviewed), place.patches(&head)];

    assert_eq!(merged, open);
}

#[test]
fn a_patch_with_more_files_than_the_list_names_is_refused() {
    let patch = b"diff --git a/a b/a\n+x\ndiff --git a/b b/b\n+y\n";

    assert!(numbered(patch, 1).is_err());
    assert_eq!(
        numbered(patch, 2).unwrap(),
        [
            format!("commit {:040x}\ndiff --git a/a b/a\n+x\n", 1),
            format!("commit {:040x}\ndiff --git a/b b/b\n+y\n", 2),
        ]
        .concat()
        .into_bytes()
    );
}

#[test]
fn a_file_patch_id_gave_no_id_is_refused() {
    let id = PatchId::parse(&"a".repeat(40)).unwrap();
    let ids = Ids {
        verbatim: BTreeMap::from([(0, id)]),
        loose: BTreeMap::new(),
    };

    assert!(file_patches(&["Lib/ast.py"], vec![false], ids).is_err());
}

#[test]
fn a_mirror_without_the_target_branch_has_no_patch_ids_to_give() {
    let place = Place::new("no-target");
    let reviewed = reviewed_pr(&place);

    let read = read_patch_ids(&place.mirror, "3.14", &reviewed);

    assert!(read.is_err());
}
