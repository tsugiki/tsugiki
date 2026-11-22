use super::{FOLD_PAST, FileNotes, FileTree, TreeRow};

fn folder(path: &str, name: &str, depth: u32, end: u32) -> TreeRow {
    TreeRow::Folder {
        path: path.to_owned(),
        name: name.to_owned(),
        depth,
        end,
    }
}

fn file(file: u32, name: &str, depth: u32) -> TreeRow {
    TreeRow::File {
        file,
        name: name.to_owned(),
        depth,
        threads: 0,
        drafts: 0,
    }
}

#[test]
fn files_sit_under_their_folders_in_the_diffs_order_each_folder_where_the_diff_first_reaches_it() {
    let tree = FileTree::new(
        [
            "Lib/ast.py",
            "Lib/test/test_ast.py",
            "Lib/token.py",
            "README.rst",
        ],
        &[],
    );

    let rows = vec![
        folder("Lib", "Lib", 0, 5),
        file(0, "ast.py", 1),
        folder("Lib/test", "test", 1, 4),
        file(1, "test_ast.py", 2),
        file(2, "token.py", 1),
        file(3, "README.rst", 0),
    ];
    assert_eq!(
        tree,
        FileTree {
            rows,
            folded: false
        }
    );
}

#[test]
fn a_folder_that_holds_only_a_folder_joins_it_as_github_does() {
    let tree = FileTree::new(
        [
            "Lib/test/test_ast/test_one.py",
            "Lib/test/test_ast/test_two.py",
            "Lib/test/test_ast/data/a.txt",
            "Doc/library/ast.rst",
        ],
        &[],
    );

    let rows = vec![
        folder("Lib/test/test_ast", "Lib/test/test_ast", 0, 5),
        file(0, "test_one.py", 1),
        file(1, "test_two.py", 1),
        folder("Lib/test/test_ast/data", "data", 1, 5),
        file(2, "a.txt", 2),
        folder("Doc/library", "Doc/library", 0, 7),
        file(3, "ast.rst", 1),
    ];
    assert_eq!(tree.rows, rows);
}

#[test]
fn a_folder_that_holds_a_file_beside_its_one_folder_stays_apart() {
    let tree = FileTree::new(["Lib/test/test_ast.py", "Lib/ast.py"], &[]);

    let rows = vec![
        folder("Lib", "Lib", 0, 4),
        folder("Lib/test", "test", 1, 3),
        file(0, "test_ast.py", 2),
        file(1, "ast.py", 1),
    ];
    assert_eq!(tree.rows, rows);
}

#[test]
fn each_file_counts_its_threads_and_drafts_by_its_place_and_one_past_the_notes_holds_none() {
    let notes = [
        FileNotes::default(),
        FileNotes {
            threads: 2,
            drafts: 1,
        },
    ];
    let tree = FileTree::new(["a.py", "b.py", "c.py"], &notes);

    let held = |file: u32, name: &str, threads: u32, drafts: u32| TreeRow::File {
        file,
        name: name.to_owned(),
        depth: 0,
        threads,
        drafts,
    };
    let rows = vec![
        held(0, "a.py", 0, 0),
        held(1, "b.py", 2, 1),
        held(2, "c.py", 0, 0),
    ];
    assert_eq!(tree.rows, rows);
}

#[test]
fn the_folders_start_folded_only_past_a_few_hundred_files() {
    let paths: Vec<String> = (0..=FOLD_PAST)
        .map(|n| format!("Lib/test_{n}.py"))
        .collect();
    let few = FileTree::new(paths.iter().take(FOLD_PAST).map(String::as_str), &[]);
    let many = FileTree::new(paths.iter().map(String::as_str), &[]);

    assert!(!few.folded);
    assert!(many.folded);
    assert_eq!(many.rows.len(), FOLD_PAST + 2);
}

#[test]
fn no_files_make_no_rows() {
    assert_eq!(FileTree::new([], &[]), FileTree::default());
}
