use super::*;

fn id(digit: char) -> PatchId {
    PatchId::parse(&digit.to_string().repeat(40)).unwrap()
}

/// A file whose change has patch-id `verbatim`, or `loose` once whitespace is ignored.
fn file(path: &str, verbatim: char, loose: char) -> FilePatch {
    FilePatch {
        path: path.to_owned(),
        verbatim: id(verbatim),
        loose: id(loose),
        generated: false,
    }
}

fn generated(file: FilePatch) -> FilePatch {
    FilePatch {
        generated: true,
        ..file
    }
}

fn changed(path: &str) -> ChangedFile {
    ChangedFile {
        path: path.to_owned(),
        whitespace_only: false,
    }
}

#[test]
fn files_whose_changes_are_the_same_leave_the_review_as_it_was() {
    let reviewed = [file("Lib/ast.py", '1', 'a'), file("Doc/ast.rst", '2', 'b')];
    let head = [file("Doc/ast.rst", '2', 'b'), file("Lib/ast.py", '1', 'a')];

    assert_eq!(changed_files(&reviewed, &head), []);
}

#[test]
fn a_file_whose_change_differs_has_changed_since_the_review() {
    let reviewed = [file("Lib/ast.py", '1', 'a'), file("Doc/ast.rst", '2', 'b')];
    let head = [file("Lib/ast.py", '3', 'c'), file("Doc/ast.rst", '2', 'b')];

    assert_eq!(changed_files(&reviewed, &head), [changed("Lib/ast.py")]);
}

#[test]
fn a_file_the_pr_starts_or_stops_changing_has_changed_since_the_review() {
    let reviewed = [file("Lib/ast.py", '1', 'a'), file("Lib/old.py", '2', 'b')];
    let head = [file("Lib/ast.py", '1', 'a'), file("Lib/new.py", '3', 'c')];

    assert_eq!(
        changed_files(&reviewed, &head),
        [changed("Lib/new.py"), changed("Lib/old.py")]
    );
}

#[test]
fn a_change_only_in_whitespace_counts_and_says_so() {
    let reviewed = [file("Lib/ast.py", '1', 'a')];
    let head = [file("Lib/ast.py", '2', 'a')];

    assert_eq!(
        changed_files(&reviewed, &head),
        [ChangedFile {
            path: "Lib/ast.py".to_owned(),
            whitespace_only: true,
        }]
    );
}

#[test]
fn generated_files_dont_count_on_either_side() {
    let reviewed = [
        file("Lib/ast.py", '1', 'a'),
        generated(file("Python/clinic.h", '2', 'b')),
        file("Include/opcode.h", '3', 'c'),
    ];
    let head = [
        file("Lib/ast.py", '1', 'a'),
        file("Python/clinic.h", '4', 'd'),
        generated(file("Include/opcode.h", '5', 'e')),
        generated(file("Python/new_clinic.h", '6', 'f')),
    ];

    assert_eq!(changed_files(&reviewed, &head), []);
}

#[test]
fn a_path_a_diff_names_twice_compares_both_of_its_changes() {
    // A file deleted, and another renamed into its place.
    let reviewed = [file("Lib/a.py", '1', 'a'), file("Lib/a.py", '2', 'b')];
    let same = [file("Lib/a.py", '2', 'b'), file("Lib/a.py", '1', 'a')];
    let one_moved = [file("Lib/a.py", '1', 'a'), file("Lib/a.py", '3', 'b')];

    assert_eq!(changed_files(&reviewed, &same), []);
    assert_eq!(
        changed_files(&reviewed, &one_moved),
        [ChangedFile {
            path: "Lib/a.py".to_owned(),
            whitespace_only: true,
        }]
    );
}

#[test]
fn a_patch_id_is_a_whole_sha_1() {
    assert_eq!(
        PatchId::parse(&"AB".repeat(20)).map(|id| id.as_str().to_owned()),
        Some("ab".repeat(20))
    );
    assert_eq!(PatchId::parse("abc1234"), None);
    assert_eq!(PatchId::parse(&"g".repeat(40)), None);
}
