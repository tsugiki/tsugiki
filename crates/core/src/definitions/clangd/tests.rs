use std::fs;

use super::*;
use crate::testing::temp_path;

/// Writes an empty file at each of `paths` under `folder`.
fn touch(folder: &Path, paths: &[&str]) {
    for path in paths {
        let file = folder.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "").unwrap();
    }
}

#[test]
fn in_a_cpython_checkout_clangd_reads_a_database_of_its_core_c_files() {
    let root = temp_path("clangd-cpython");
    let folder = root.join("lookup");
    touch(
        &folder,
        &[
            "Include/patchlevel.h",
            "PC/pyconfig.h",
            "PC/winreg.c",
            "Objects/listobject.c",
            "Objects/new.c",
            "Objects/mimalloc/alloc.c",
            "Objects/mimalloc/static.c",
            "Python/ceval.c",
            "Python/bytecodes.c",
            "Python/generated_cases.c.h",
            "Modules/_hacl/Hacl_Hash_SHA2.c",
            "Modules/expat/xmlparse.c",
            "Modules/_decimal/libmpdec/mpdecimal.c",
            "Modules/_decimal/_decimal.c",
            "Modules/_io/fileio.c",
            "Lib/test/spam.c",
        ],
    );

    let setup = setup(&folder, Path::new("clangd")).unwrap();
    let database = root.join("servers").join("clangd");
    let text = fs::read_to_string(database.join("compile_commands.json"));
    let _removed = fs::remove_dir_all(&root);

    let entries: Vec<Value> = serde_json::from_str(&text.unwrap()).unwrap();
    let files: Vec<&str> = (entries.iter())
        .map(|entry| entry["arguments"].as_array().unwrap().last().unwrap())
        .map(|file| file.as_str().unwrap())
        .collect();
    let expected = [
        "Modules/_decimal/_decimal.c",
        "Modules/_io/fileio.c",
        "Objects/listobject.c",
        "Objects/new.c",
        "PC/winreg.c",
        "Python/ceval.c",
    ];
    assert_eq!(files, expected);
    let first = &entries[0];
    assert_eq!(first["directory"], json!(folder));
    assert_eq!(first["file"], json!(folder.join(expected[0])));
    assert_eq!(first["arguments"][0], "clang");
    assert_eq!(first["arguments"][4], "-IInclude");
    let mut arg = OsString::from("--compile-commands-dir=");
    arg.push(&database);
    assert_eq!(setup.args.last(), Some(&arg));
    assert_eq!(setup.language, Language::C);
}

#[test]
fn elsewhere_clangd_uses_its_own_defaults() {
    let root = temp_path("clangd-elsewhere");
    let folder = root.join("lookup");
    touch(&folder, &["src/spam.c", "Objects/eggs.c"]);

    let setup = setup(&folder, Path::new("clangd")).unwrap();
    let written = root.join("servers").exists();
    let _removed = fs::remove_dir_all(&root);

    let args: Vec<&OsString> = setup.args.iter().collect();
    assert_eq!(
        args,
        [
            "--background-index",
            "--background-index-priority=low",
            "-j=4"
        ]
    );
    assert!(!written);
}

#[test]
fn a_database_it_can_t_write_stops_clangd_starting() {
    let root = temp_path("clangd-unwritable");
    let folder = root.join("lookup");
    touch(&folder, &["Include/patchlevel.h"]);
    fs::write(root.join("servers"), "").unwrap();

    let refused = setup(&folder, Path::new("clangd")).map(|_setup| ());
    let _removed = fs::remove_dir_all(&root);

    let Err(error) = refused else {
        panic!("clangd started without its database");
    };
    let database = root.join("servers").join("clangd");
    let start = format!(
        "Tsugiki couldn't write clangd's compile database in {}: ",
        database.display()
    );
    assert!(error.message.starts_with(&start), "{}", error.message);
}

/// A path that isn't Unicode, which the OS allows: a lone surrogate on Windows, or a byte that is no UTF-8
/// elsewhere.
fn not_unicode() -> PathBuf {
    #[cfg(windows)]
    let name: OsString = std::os::windows::ffi::OsStringExt::from_wide(&[0xD800]);
    #[cfg(unix)]
    let name: OsString = std::os::unix::ffi::OsStringExt::from_vec(vec![0xFF]);
    PathBuf::from(name)
}

#[test]
fn a_checkout_whose_path_isn_t_unicode_gets_no_database() {
    let folder = not_unicode().join("lookup");

    let refused = write_database(&folder, Path::new("never"));

    let message = format!(
        "Tsugiki can't write clangd's compile database for {}, as its path isn't Unicode",
        folder.display()
    );
    assert_eq!(refused, Err(failed(&message)));
    assert!(!Path::new("never").exists());
}

/// A `$/progress` on `token` with `value`, as the client reads one.
fn progress(token: &str, value: Value) -> Progress {
    serde_json::from_value(json!({"token": token, "value": value})).unwrap()
}

#[test]
fn clangd_s_index_counts_its_files_until_it_ends_and_other_progress_says_nothing() {
    let index = |value| indexing(&progress("backgroundIndexProgress", value));

    let begun = index(json!({"kind": "begin", "title": "indexing", "percentage": 0}));
    let counted = index(json!({"kind": "report", "message": "120/452", "percentage": 26}));
    let uncounted = index(json!({"kind": "report", "message": "most of them"}));
    let ended = index(json!({"kind": "end"}));
    let other = indexing(&progress("other", json!({"kind": "end"})));

    let files = IndexedFiles {
        done: 120,
        total: 452,
    };
    assert_eq!(begun, Some(Indexing::Building(None)));
    assert_eq!(counted, Some(Indexing::Building(Some(files))));
    assert_eq!(uncounted, Some(Indexing::Building(None)));
    assert_eq!(ended, Some(Indexing::Built));
    assert_eq!(other, None);
}

#[test]
fn built_in_headers_are_a_stddef_h_in_the_folder_above_clangd_s_own() {
    let root = temp_path("clangd-built-ins");
    touch(
        &root,
        &[
            "whole/bin/clangd.exe",
            "whole/lib/clang/23/include/stddef.h",
            "lone/bin/clangd.exe",
            "empty/bin/clangd.exe",
            "empty/lib/clang/23/include/stdint.h",
        ],
    );

    let found = ["whole", "lone", "empty"]
        .map(|folder| has_built_ins(&root.join(folder).join("bin").join("clangd.exe")));
    let bare = has_built_ins(Path::new("clangd"));
    let _removed = fs::remove_dir_all(&root);

    assert_eq!(found, [true, false, false]);
    assert!(!bare);
}

#[test]
fn a_missing_header_is_read_from_clangd_s_message_with_the_file_its_note_names() {
    let direct = Missing::read("'stdlib.h' file not found").unwrap();
    let nested = "In included file: 'corecrt.h' file not found\n\nC:\\SDK\\stdlib.h:12:10:\nnote: error occurred here";
    let nested = Missing::read(nested).unwrap();
    let unnoted = Missing::read("In included file: 'corecrt.h' file not found").unwrap();

    assert_eq!(direct.header, "stdlib.h");
    assert!(matches!(direct.includer, Includer::Diagnosed));
    assert_eq!(nested.header, "corecrt.h");
    assert!(matches!(nested.includer, Includer::File("C:\\SDK\\stdlib.h")));
    assert!(matches!(unnoted.includer, Includer::File("")));
    assert!(Missing::read("use of undeclared identifier 'spam'").is_none());
    assert!(Missing::read("In included file: unknown type name 'spam'").is_none());
}

#[test]
fn an_include_in_angle_brackets_names_its_header_between_them() {
    let text = "int a;\n  #  include   < stdlib.h >\n#include \"spam.h\"\n#define X <eggs.h>\n";

    assert!(includes_bracketed(text, "stdlib.h"));
    assert!(!includes_bracketed(text, "spam.h"));
    assert!(!includes_bracketed(text, "eggs.h"));
    assert!(!includes_bracketed("#include <stdlib.h", "stdlib.h"));
}

#[test]
fn a_header_clangd_can_t_find_in_a_file_that_isn_t_open_or_a_folder_git_can_t_read_counts_as_the_system_s()
{
    // A URI keeps no `..`, as the server names files.
    let root = super::super::plain(&temp_path("clangd-headers-no-git"));
    touch(&root, &["bin/clangd.exe", "lookup/eggs.c"]);
    let file = root.join("lookup").join("eggs.c");
    let uri = Uri::from_path(&file).unwrap();
    let head = CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
    let open = vec![(uri.clone(), vec![Arc::from("#include <stdlib.h>\n")])];
    let mut headers = Headers::new(
        root.join("bin").join("clangd.exe"),
        (root.join("lookup"), head),
        open,
    );
    let diagnosed = |uri: &Uri| -> Diagnostics {
        serde_json::from_value(json!({
            "uri": uri,
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 19}},
                "message": "'stdlib.h' file not found",
            }],
        }))
        .unwrap()
    };

    let open_file = headers.problem(&diagnosed(&uri));
    let other = Uri::from_path(&root.join("lookup").join("other.c")).unwrap();
    let other_file = headers.problem(&diagnosed(&other));
    let _removed = fs::remove_dir_all(&root);

    let header = "stdlib.h".to_owned();
    assert_eq!(
        open_file,
        Some(ServerProblem::SystemHeadersMissing { header })
    );
    assert_eq!(other_file, None);
}
