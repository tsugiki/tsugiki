use std::fs;
use std::path::PathBuf;

use tsugiki_test_git::git;

use super::*;
use crate::patch_ids::{ChangedFile, changed_files};

/// A repository with a branch `main` and a PR on it, whose commits a test makes and reads as a mirror would.
struct Place {
    root: PathBuf,
    work: PathBuf,
}

impl Place {
    fn new(name: &str) -> Self {
        let root = crate::testing::temp_path(&format!("patch-ids-{name}"));
        let work = root.join("work");
        fs::create_dir_all(work.join("Lib")).unwrap();
        let _ = git(&work, &["init", "-q", "-b", "main"]);
        let place = Self { root, work };
        place.write("Lib/ast.py", &lines(1..=20));
        place.write("Lib/os.py", &lines(1..=20));
        place.write("README.rst", "Read me\n");
        let _ = place.commit("One");
        place
    }

    fn write(&self, path: &str, text: &str) {
        fs::write(self.work.join(path), text).unwrap();
    }

    /// Commits everything, and gives the commit.
    fn commit(&self, message: &str) -> CommitSha {
        let _ = git(&self.work, &["add", "-A"]);
        let _ = git(&self.work, &["commit", "-qm", message]);
        self.at("HEAD")
    }

    fn at(&self, revision: &str) -> CommitSha {
        CommitSha::parse(git(&self.work, &["rev-parse", revision]).trim()).unwrap()
    }

    fn switch(&self, args: &[&str]) {
        let _ = git(&self.work, &[["switch", "-q"].as_slice(), args].concat());
    }

    /// A bare repository that borrows every object of the work, with its `main`, as a mirror has.
    fn mirror(&self) -> PathBuf {
        let mirror = self.root.join("mirror.git");
        if !mirror.exists() {
            let _ = git(&self.root, &["init", "-q", "--bare", "mirror.git"]);
            let objects = self.work.join(".git").join("objects");
            let alternates = mirror.join("objects").join("info").join("alternates");
            fs::write(alternates, objects.to_string_lossy().as_bytes()).unwrap();
        }
        let main = self.at("main");
        let _ = git(&mirror, &["update-ref", "refs/heads/main", main.as_str()]);
        mirror
    }

    /// The patch-ids of `commit` against `main`, read from the mirror.
    fn patches(&self, commit: &CommitSha) -> Vec<FilePatch> {
        read_patch_ids(&self.mirror(), "main", commit).unwrap()
    }

    fn finish(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}

fn lines(numbers: std::ops::RangeInclusive<u32>) -> String {
    numbers.map(|n| format!("x{n} = {n}\n")).collect()
}

/// A PR that changes line 5 of `Lib/ast.py` from main's first commit, and gives its commit.
fn reviewed_pr(place: &Place) -> CommitSha {
    place.switch(&["-c", "pr"]);
    place.write(
        "Lib/ast.py",
        &lines(1..=20).replace("x5 = 5", "x5 = 'five'"),
    );
    place.commit("The PR")
}

/// main moves on in a file the PR doesn't touch, and in another part of the one it does.
fn main_moves_on(place: &Place) {
    place.switch(&["main"]);
    place.write("README.rst", "Read me first\n");
    place.write(
        "Lib/ast.py",
        &lines(1..=20).replace("x18 = 18", "x18 = 'eighteen'"),
    );
    let _ = place.commit("Two");
}

#[test]
fn each_file_the_diff_changes_gets_a_patch_id_by_its_path() {
    let place = Place::new("each-file");
    let reviewed = reviewed_pr(&place);

    let patches = place.patches(&reviewed);
    place.finish();

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
    main_moves_on(&place);
    place.switch(&["pr"]);
    let _ = git(&place.work, &["merge", "-q", "--no-edit", "main"]);
    let merged = place.at("HEAD");

    let (before, now) = (place.patches(&reviewed), place.patches(&merged));
    place.finish();

    assert_eq!(changed_files(&before, &now), []);
}

#[test]
fn a_rebase_that_leaves_each_files_diff_the_same_leaves_its_patch_id_alone() {
    let place = Place::new("rebase");
    let reviewed = reviewed_pr(&place);
    main_moves_on(&place);
    place.switch(&["pr"]);
    let _ = git(&place.work, &["rebase", "-q", "main"]);
    let rebased = place.at("HEAD");

    let (before, now) = (place.patches(&reviewed), place.patches(&rebased));
    place.finish();

    assert_ne!(reviewed, rebased);
    assert_eq!(changed_files(&before, &now), []);
}

#[test]
fn a_new_change_to_a_file_changes_its_patch_id() {
    let place = Place::new("new-change");
    let reviewed = reviewed_pr(&place);
    place.write(
        "Lib/ast.py",
        &lines(1..=20).replace("x5 = 5", "x5 = 'FIVE'"),
    );
    place.write("Lib/os.py", &lines(1..=21));
    let pushed = place.commit("More");

    let (before, now) = (place.patches(&reviewed), place.patches(&pushed));
    place.finish();

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
    place.write(
        "Lib/ast.py",
        &lines(1..=20).replace("x5 = 5", "x5 = 'five'  "),
    );
    let pushed = place.commit("Spaces");

    let (before, now) = (place.patches(&reviewed), place.patches(&pushed));
    place.finish();

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
    place.switch(&["-c", "pr"]);
    place.write("logo.png", "\u{0}one");
    let reviewed = place.commit("Logo");
    place.write("logo.png", "\u{0}two");
    let pushed = place.commit("Another logo");

    let (before, now) = (place.patches(&reviewed), place.patches(&pushed));
    place.finish();

    assert_eq!(
        changed_files(&before, &now),
        [ChangedFile {
            path: "logo.png".to_owned(),
            whitespace_only: false,
        }]
    );
}

#[test]
fn the_target_branchs_attributes_say_which_files_are_generated() {
    let place = Place::new("generated");
    let reviewed = reviewed_pr(&place);
    place.switch(&["main"]);
    place.write(".gitattributes", "Lib/ast.py linguist-generated\n");
    let _ = place.commit("Generated");

    let patches = place.patches(&reviewed);
    place.finish();

    assert_eq!(
        patches
            .iter()
            .map(|file| (file.path.as_str(), file.generated))
            .collect::<Vec<_>>(),
        [("Lib/ast.py", true)]
    );
}

#[test]
fn a_commit_already_on_the_target_branch_gives_no_patch_ids_to_keep() {
    let place = Place::new("level");
    let main = place.at("main");

    // A PR merged before the sync said so has its head on the Target branch, and an empty diff from there
    // would read as every file changed for good.
    let read = read_patch_ids(&place.mirror(), "main", &main);
    place.finish();

    assert!(read.is_err());
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

    let read = read_patch_ids(&place.mirror(), "3.14", &reviewed);
    place.finish();

    assert!(read.is_err());
}
