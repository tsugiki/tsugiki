use super::*;

/// A logger with its own folder, so tests don't share the process's one logger.
fn temp_logger(name: &str) -> (Logger, PathBuf) {
    let folder = crate::testing::temp_path(&format!("log-{name}"));
    let logger = Logger::start(folder.join("tsugiki.log"));
    (logger, folder)
}

fn say(logger: &Logger, level: Level, target: &str, text: &str) {
    logger.log(
        &Record::builder()
            .args(format_args!("{text}"))
            .level(level)
            .target(target)
            .build(),
    );
}

/// What reached the file, once the thread has written everything, however long that takes.
fn written(logger: &Logger, folder: &Path) -> String {
    let (done, reached) = mpsc::channel::<()>();
    logger.send(Message::Flush(done));
    assert!(reached.recv().is_err(), "the thread drops `done`");
    fs::read_to_string(folder.join("tsugiki.log")).unwrap()
}

#[test]
fn a_line_says_when_how_bad_where_and_what() {
    let (logger, folder) = temp_logger("line");
    say(
        &logger,
        Level::Warn,
        "tsugiki_core::store",
        "the disk is full",
    );

    let file = written(&logger, &folder);

    assert!(
        file.contains(" WARN tsugiki_core::store: the disk is full\n"),
        "{file}"
    );
    assert!(file.starts_with("20"), "it starts with the year: {file}");
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_token_in_use_never_reaches_the_file_whatever_its_shape() {
    let (logger, folder) = temp_logger("in-use");
    logger.send(Message::Hide("0123456789abcdef".to_owned()));
    say(
        &logger,
        Level::Warn,
        "tsugiki",
        "Bearer 0123456789abcdef refused",
    );

    let file = written(&logger, &folder);

    assert!(!file.contains("0123456789abcdef"), "{file}");
    assert!(file.contains("Bearer [token] refused"), "{file}");
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn text_shaped_like_a_token_never_reaches_the_file() {
    let (logger, folder) = temp_logger("shaped");
    for token in [
        "ghp_abc123",
        "gho_abc123",
        "ghu_abc123",
        "ghs_abc123",
        "ghr_abc123",
        "github_pat_11ABC_def",
    ] {
        say(
            &logger,
            Level::Warn,
            "tsugiki",
            &format!("first {token}, then {token}."),
        );
    }

    let file = written(&logger, &folder);

    assert!(!file.contains("abc123"), "{file}");
    assert!(!file.contains("11ABC"), "{file}");
    assert!(file.contains("first [token], then [token]."), "{file}");
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_prefix_with_nothing_after_it_is_only_words() {
    let (logger, folder) = temp_logger("prefix");
    say(
        &logger,
        Level::Warn,
        "tsugiki",
        "tokens start with ghp_ or gho_",
    );

    let file = written(&logger, &folder);

    assert!(file.contains("tokens start with ghp_ or gho_\n"), "{file}");
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_token_cut_off_by_the_length_limit_still_doesnt_show() {
    let (logger, folder) = temp_logger("cut");
    let token = format!("ghp_{}", "a".repeat(MAX_LINE_CHARS * 2));
    say(&logger, Level::Warn, "tsugiki", &token);

    let file = written(&logger, &folder);

    assert!(!file.contains("aaaa"), "{file}");
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_long_line_is_cut_on_a_character_and_the_next_line_still_starts_fresh() {
    let (logger, folder) = temp_logger("long");
    say(
        &logger,
        Level::Warn,
        "tsugiki",
        &"é".repeat(MAX_LINE_CHARS * 2),
    );
    say(&logger, Level::Warn, "tsugiki", "the next one");

    let file = written(&logger, &folder);

    assert!(
        file.chars().count() < MAX_LINE_CHARS + 100,
        "the long line was cut"
    );
    assert!(
        file.ends_with("tsugiki: the next one\n"),
        "the next line follows"
    );
    fs::remove_dir_all(folder).unwrap();
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).unwrap().len()
}

#[test]
fn the_file_and_the_one_before_it_stay_under_the_limit() {
    let (logger, folder) = temp_logger("bounded");
    let line = "x".repeat(MAX_LINE_CHARS);
    let lines_for_three_files = usize::try_from(MAX_FILE_BYTES).unwrap() * 3 / MAX_LINE_CHARS;
    for number in 0..lines_for_three_files {
        say(&logger, Level::Warn, "tsugiki", &format!("{number} {line}"));
    }
    say(&logger, Level::Warn, "tsugiki", "the last one");

    let file = written(&logger, &folder);

    assert!(file.ends_with("tsugiki: the last one\n"));
    let limit = MAX_FILE_BYTES + u64::try_from(MAX_LINE_CHARS).unwrap();
    assert!(file_size(&folder.join("tsugiki.log")) <= limit);
    assert!(file_size(&folder.join("tsugiki.log.1")) <= limit);
    assert_eq!(
        fs::read_dir(&folder).unwrap().count(),
        2,
        "and no third file"
    );
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_file_that_cant_be_moved_aside_makes_way_for_new_lines() {
    let (logger, folder) = temp_logger("cant-move");
    fs::create_dir_all(folder.join("tsugiki.log.1")).unwrap();
    let full = "x".repeat(usize::try_from(MAX_FILE_BYTES).unwrap());
    fs::write(folder.join("tsugiki.log"), &full).unwrap();

    say(&logger, Level::Warn, "tsugiki", "the newest line");

    let file = written(&logger, &folder);
    assert!(file.ends_with("tsugiki: the newest line\n"), "{file}");
    assert!(file.len() < 100, "the old lines made way");
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_line_the_same_as_the_last_isnt_written_again() {
    let (logger, folder) = temp_logger("repeats");
    for text in ["offline", "offline", "offline", "a bad answer", "offline"] {
        say(&logger, Level::Warn, "tsugiki", text);
    }

    let file = written(&logger, &folder);

    let said: Vec<_> = file
        .lines()
        .map(|line| line.rsplit(": ").next().unwrap())
        .collect();
    assert_eq!(said, ["offline", "a bad answer", "offline"]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_folder_it_cant_create_loses_the_lines_and_nothing_else() {
    let blocker = crate::testing::temp_path("log-blocker");
    fs::write(&blocker, "a file where the folder should be").unwrap();
    let logger = Logger::start(blocker.join("tsugiki.log"));

    say(&logger, Level::Warn, "tsugiki", "this has nowhere to go");
    logger.flush();

    fs::remove_file(blocker).unwrap();
}

#[test]
fn libraries_only_write_what_is_wrong() {
    let (logger, folder) = temp_logger("libraries");
    say(&logger, Level::Info, "ureq", "connected");
    say(&logger, Level::Warn, "rustls", "a bad certificate");
    say(&logger, Level::Info, "tsugiki_core", "started");
    say(&logger, Level::Debug, "tsugiki_core", "details");

    let file = written(&logger, &folder);

    assert!(!file.contains("connected"), "{file}");
    assert!(file.contains("a bad certificate"), "{file}");
    assert!(file.contains("started"), "{file}");
    assert!(!file.contains("details"), "{file}");
    fs::remove_dir_all(folder).unwrap();
}
