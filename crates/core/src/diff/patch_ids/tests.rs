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
fn the_attributes_on_the_target_branch_say_which_files_are_generated() {
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

/// Puts an entry of `mode` in the index at `path`, with `content` as its blob, or as the commit a gitlink
/// names.
fn put(place: &Place, mode: &str, content: &str, path: &str) {
    let object = if mode == "160000" {
        content.to_owned()
    } else {
        let blob = place.root.join("blob");
        fs::write(&blob, content).unwrap();
        let written = git(&place.work, &["hash-object", "-w", &blob.to_string_lossy()]);
        written.trim().to_owned()
    };
    let entry = format!("{mode},{object},{path}");
    let _ = git(
        &place.work,
        &["update-index", "--add", "--cacheinfo", &entry],
    );
}

#[test]
fn a_file_turned_into_a_symlink_or_a_submodule_a_rename_and_a_mode_change_each_get_a_patch_id() {
    let place = Place::new("typechange");
    let first = place.at("main");
    place.write("Lib/abc.py", &lines(1..=20));
    let _ = place.commit("Two");
    place.switch(&["-c", "pr"]);
    let _ = git(&place.work, &["mv", "Lib/os.py", "Lib/posix.py"]);
    let _ = git(&place.work, &["update-index", "--chmod=+x", "Lib/ast.py"]);
    put(&place, "120000", "Lib/ast.py", "README.rst");
    let _ = git(&place.work, &["rm", "-q", "--cached", "Lib/abc.py"]);
    put(&place, "160000", first.as_str(), "Lib/abc.py");
    let _ = git(&place.work, &["commit", "-qm", "Odd changes"]);
    let pr = place.at("HEAD");

    let read = read_patch_ids(&place.mirror(), "main", &pr);
    place.finish();

    let paths: Vec<String> = read.unwrap().into_iter().map(|file| file.path).collect();
    assert_eq!(
        paths,
        ["Lib/abc.py", "Lib/ast.py", "Lib/posix.py", "README.rst"]
    );
}

#[test]
fn the_users_diff_settings_leave_the_patch_ids_alone() {
    let place = Place::new("settings");
    place.switch(&["-c", "pr"]);
    let changed = lines(1..=20)
        .replace("x5 = 5", "x5 = 'five'")
        .replace("x12 = 12", "x12 = 'twelve'");
    place.write("Lib/ast.py", &changed);
    place.write("Lib/\u{fc}nicode.py", "\nx = 1\n\ny = 2\n");
    let pr = place.commit("The PR");
    let plain = place.patches(&pr);

    // git reads the mirror's own config as it reads the user's.
    let mirror = place.mirror();
    for setting in [
        "diff.noprefix=true",
        "diff.mnemonicPrefix=true",
        "diff.srcPrefix=old/",
        "diff.dstPrefix=new/",
        "diff.algorithm=histogram",
        "diff.indentHeuristic=false",
        "diff.interHunkContext=10",
        "diff.context=10",
        "diff.suppressBlankEmpty=true",
        "diff.renames=false",
        "diff.relative=true",
        "diff.external=false",
        "diff.ignoreSubmodules=all",
        "core.quotePath=false",
    ] {
        let (name, value) = setting.split_once('=').unwrap();
        let _ = git(&mirror, &["config", name, value]);
    }
    let configured = place.patches(&pr);
    place.finish();

    assert_eq!(configured, plain);
}

#[test]
fn a_commit_already_on_the_target_branch_gives_no_patch_ids_to_keep() {
    let place = Place::new("level");
    let main = place.at("main");

    // A commit on the Target branch that no merge commit brought in has an empty diff from there, which
    // would read as every file changed for good.
    let read = read_patch_ids(&place.mirror(), "main", &main);
    place.finish();

    assert!(read.is_err());
}

#[test]
fn a_pr_merged_with_a_merge_commit_keeps_the_patch_ids_it_had_while_open() {
    let place = Place::new("merge-commit");
    let reviewed = reviewed_pr(&place);
    main_moves_on(&place);
    place.switch(&["pr"]);
    let _ = git(&place.work, &["merge", "-q", "--no-edit", "main"]);
    place.write(
        "Lib/os.py",
        &lines(1..=20).replace("x3 = 3", "x3 = 'three'"),
    );
    let head = place.commit("More");
    let open = [place.patches(&reviewed), place.patches(&head)];
    place.switch(&["main"]);
    place.write("README.rst", "Read me again\n");
    let _ = place.commit("Three");
    let _ = git(&place.work, &["merge", "-q", "--no-ff", "--no-edit", "pr"]);
    place.write("README.rst", "Read me last\n");
    let _ = place.commit("Four");

    // Both commits are on main now, and their merge base with it is themselves.
    let merged = [place.patches(&reviewed), place.patches(&head)];
    place.finish();

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

    let read = read_patch_ids(&place.mirror(), "3.14", &reviewed);
    place.finish();

    assert!(read.is_err());
}
