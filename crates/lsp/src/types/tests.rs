use std::path::Path;

use serde_json::{Value, json};

use super::{
    Definitions, Diagnostic, Diagnostics, Language, Location, LocationLink, Position, Progress,
    ProgressToken, Range, Severity, Uri, WorkDone,
};

fn uri(text: &str) -> Uri {
    Uri::try_from(text.to_owned()).unwrap()
}

fn range(line: u32, from: u32, to: u32) -> Range {
    Range {
        start: Position {
            line,
            character: from,
        },
        end: Position {
            line,
            character: to,
        },
    }
}

fn definitions(reply: Value) -> Definitions {
    serde_json::from_value(reply).unwrap()
}

#[cfg(windows)]
const ABSOLUTE: &str = r"X:\Work\cpython\Lib\os.py";
#[cfg(not(windows))]
const ABSOLUTE: &str = "/work/cpython/Lib/os.py";

#[test]
fn a_path_and_its_uri_turn_into_each_other() {
    let path = Path::new(ABSOLUTE);
    let made = Uri::from_path(path).unwrap();
    assert!(made.as_str().starts_with("file:///"));
    assert_eq!(made.to_string(), made.as_str());
    assert_eq!(made.to_path().unwrap(), path);
    assert_eq!(Uri::from_path(Path::new("Lib/os.py")), None);
    assert_eq!(uri("https://example.com/os.py").to_path(), None);
}

#[cfg(windows)]
#[test]
fn a_drive_in_either_case_or_with_its_colon_encoded_is_the_same_uri() {
    let made = Uri::from_path(Path::new(r"x:\Work\os.py")).unwrap();
    for written in [
        "file:///X%3A/Work/os.py",
        "file:///x:/Work/os.py",
        "file:///X:/Work/os.py",
    ] {
        let read = uri(written);
        assert_eq!(read, made, "{written}");
        assert_eq!(read.as_str(), "file:///X:/Work/os.py");
        assert_eq!(read.to_path().unwrap(), Path::new(r"X:\Work\os.py"));
    }
}

#[test]
fn a_uri_is_written_and_read_as_text() {
    let read: Uri = serde_json::from_value(json!("file:///a/b.py")).unwrap();
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        json!("file:///a/b.py")
    );
    assert!(serde_json::from_value::<Uri>(json!("not a uri")).is_err());
}

#[test]
fn no_definition_is_null_or_an_empty_list() {
    for reply in [Value::Null, json!([])] {
        let found = definitions(reply);
        assert_eq!(found, Definitions::Locations(Vec::new()));
        assert!(found.is_empty());
    }
    assert!(Definitions::Links(Vec::new()).is_empty());
}

#[test]
fn one_location_or_several_are_locations() {
    let one = json!({"uri": "file:///a.py", "range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 7}}});
    let location = Location {
        uri: uri("file:///a.py"),
        range: range(1, 4, 7),
    };
    assert_eq!(
        definitions(one.clone()),
        Definitions::Locations(vec![location.clone()])
    );
    let found = definitions(json!([one, one]));
    assert_eq!(
        found,
        Definitions::Locations(vec![location.clone(), location])
    );
    assert!(!found.is_empty());
}

#[test]
fn links_keep_both_ranges_and_where_the_name_was() {
    let link = json!({
        "originSelectionRange": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 3}},
        "targetUri": "file:///b.c",
        "targetRange": {"start": {"line": 2, "character": 0}, "end": {"line": 5, "character": 1}},
        "targetSelectionRange": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 7}},
    });
    let found = definitions(json!([link]));
    let expected = LocationLink {
        origin_selection_range: Some(range(9, 0, 3)),
        target_uri: uri("file:///b.c"),
        target_range: Range {
            start: Position {
                line: 2,
                character: 0,
            },
            end: Position {
                line: 5,
                character: 1,
            },
        },
        target_selection_range: range(2, 4, 7),
    };
    assert_eq!(found, Definitions::Links(vec![expected]));
    assert!(!found.is_empty());
}

#[test]
fn a_reply_of_another_shape_isn_t_a_definition() {
    assert!(serde_json::from_value::<Definitions>(json!(42)).is_err());
    assert!(serde_json::from_value::<Definitions>(json!([{"uri": "file:///a"}])).is_err());
}

#[test]
fn each_language_has_its_lsp_identifier() {
    assert_eq!(
        [Language::C, Language::Python, Language::Rust].map(Language::id),
        ["c", "python", "rust"]
    );
}

#[test]
fn diagnostics_read_with_their_severity() {
    let read: Diagnostics = serde_json::from_value(json!({
        "uri": "file:///a.c",
        "version": 3,
        "diagnostics": [
            {"range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 19}},
             "severity": 1, "source": "clang", "message": "'Python.h' file not found", "code": "pp_file_not_found"},
            {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}}, "message": "m"},
        ],
    }))
    .unwrap();
    assert_eq!(
        read,
        Diagnostics {
            uri: uri("file:///a.c"),
            version: Some(3),
            diagnostics: vec![
                Diagnostic {
                    range: range(0, 9, 19),
                    severity: Some(Severity::Error),
                    source: Some("clang".into()),
                    message: "'Python.h' file not found".into(),
                },
                Diagnostic {
                    range: range(1, 0, 1),
                    severity: None,
                    source: None,
                    message: "m".into()
                },
            ],
        }
    );
}

#[test]
fn each_severity_reads_from_its_number_and_no_other_does() {
    assert_eq!(
        [1_u8, 2, 3, 4].map(|n| Severity::try_from(n).unwrap()),
        [
            Severity::Error,
            Severity::Warning,
            Severity::Information,
            Severity::Hint
        ]
    );
    assert_eq!(
        Severity::try_from(5).unwrap_err(),
        "5 isn't a diagnostic's severity"
    );
}

#[test]
fn progress_reads_each_kind_and_either_token() {
    let read = |value: Value| serde_json::from_value::<Progress>(value).unwrap();
    assert_eq!(
        read(
            json!({"token": "backgroundIndexProgress", "value": {"kind": "begin", "title": "indexing", "percentage": 0}})
        ),
        Progress {
            token: ProgressToken::Text("backgroundIndexProgress".into()),
            value: WorkDone::Begin {
                title: "indexing".into(),
                message: None,
                percentage: Some(0)
            },
        }
    );
    assert_eq!(
        read(json!({"token": 7, "value": {"kind": "report", "message": "3/452"}})).value,
        WorkDone::Report {
            message: Some("3/452".into()),
            percentage: None
        }
    );
    assert_eq!(
        read(json!({"token": 7, "value": {"kind": "end"}})),
        Progress {
            token: ProgressToken::Number(7),
            value: WorkDone::End { message: None }
        }
    );
}
