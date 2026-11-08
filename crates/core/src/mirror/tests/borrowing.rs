//! Mirrors that borrow objects from a clone the user has (#172).

use std::collections::BTreeMap;
use std::time::SystemTime;

use super::*;

/// Long enough for a clone that was going to start to have started.
const A_MOMENT: Duration = Duration::from_millis(300);

impl Place {
    /// A clone of `name`'s remote outside Tsugiki's folders, as a user keeps one, whose `origin` is the URL
    /// Tsugiki clones from.
    fn user_clone(&self, name: &RepositoryName) -> PathBuf {
        self.remote(name);
        let folder = self.remotes.with_file_name(format!("user-{}", name.name()));
        let url = self.clone_source().url(name);
        let _ = git(
            &self.remotes,
            &["clone", "-q", &url, &folder.to_string_lossy()],
        );
        folder
    }

    fn prefixes(&self) -> Vec<String> {
        self.clone_source().prefixes
    }
}

/// Each file under `folder` with its size and when it last changed, to show nothing wrote there.
fn snapshot(folder: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(next) = folders.pop() {
        for entry in fs::read_dir(next).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                folders.push(entry.path());
            } else {
                let _ = files.insert(entry.path(), (metadata.len(), metadata.modified().unwrap()));
            }
        }
    }
    files
}

/// The mirrors with the fake git, which announce each Repository asked about, and each start of git.
fn start_asking(place: &Place) -> (Mirrors, Receiver<FakeStart>, Receiver<Option<String>>) {
    let (asking, asked) = channel();
    let announce = Announce {
        cloning: Box::new(|_| {}),
        fetched: Box::new(|_, _| {}),
        problem: Box::new(|_| {}),
        question: Box::new(move |repository| {
            drop(asking.send(repository.map(ToString::to_string)));
        }),
    };
    let (_versions, answers) = channel();
    let (mirrors, starts) = place.start_faked_from(place.clone_source(), announce, answers);
    (mirrors, starts, asked)
}

fn borrow(folder: &Path) -> Option<MirrorSource> {
    Some(MirrorSource::Borrow {
        folder: folder.to_path_buf(),
    })
}

#[test]
fn an_answer_reads_back_as_machine_toml_keeps_it() {
    let folder = MirrorSource::Borrow {
        folder: PathBuf::from(r"D:\cpython"),
    };

    assert_eq!(MirrorSource::from_text(r"D:\cpython"), folder);
    assert_eq!(folder.to_text(), r"D:\cpython");
    assert_eq!(MirrorSource::from_text("download"), MirrorSource::Download);
    assert_eq!(MirrorSource::Download.to_text(), "download");
}

#[test]
fn a_remote_names_the_repository_over_https_or_ssh_with_or_without_git_in_any_case() {
    let github = GITHUB_PREFIXES.map(str::to_owned).to_vec();
    let cpython = named("python/cpython");

    for url in [
        "https://github.com/python/cpython",
        "https://github.com/python/cpython.git",
        "https://github.com/python/cpython/",
        "HTTPS://GitHub.com/Python/CPython.git",
        "git@github.com:python/cpython.git",
        "ssh://git@github.com/python/cpython",
        "https://someone@github.com/python/cpython.git",
        "https://www.github.com/python/cpython",
        "ssh://git@ssh.github.com:443/python/cpython.git",
    ] {
        assert!(source::names(&github, url, &cpython), "{url}");
    }
    for url in [
        "https://github.com/python/cpython-docs.git",
        "ssh://git@gitlab.com/python/cpython.git",
        "https://github.com/python",
        "ssh://github.com@gitlab.com/python/cpython",
        "someone@github.com:python/cpython.git",
        "",
    ] {
        assert!(!source::names(&github, url, &cpython), "{url}");
    }
}

#[test]
fn a_clone_of_the_repository_lends_the_git_folder_that_holds_its_objects() {
    let place = Place::new("lender");
    let hello = named("octo-org/hello");
    let folder = place.user_clone(&hello);

    let common = lender(&LiveGit, &place.prefixes(), &folder, &hello).unwrap();

    assert_eq!(common, folder.join(".git"));
}

#[test]
fn a_worktree_lends_the_git_folder_of_the_clone_it_belongs_to() {
    let place = Place::new("lender-worktree");
    let hello = named("octo-org/hello");
    let folder = place.user_clone(&hello);
    let worktree = folder.with_file_name("user-hello-feature");
    let _ = git(
        &folder,
        &["worktree", "add", "-q", &worktree.to_string_lossy()],
    );

    let common = lender(&LiveGit, &place.prefixes(), &worktree, &hello).unwrap();

    assert_eq!(
        fs::canonicalize(common).unwrap(),
        fs::canonicalize(folder.join(".git")).unwrap()
    );
}

#[test]
fn a_folder_that_cant_lend_is_refused_with_the_reason() {
    let place = Place::new("not-lenders");
    let [hello, other] = [named("octo-org/hello"), named("octo-org/other")];
    let clone = place.user_clone(&hello);
    // A clone whose `.git` points nowhere, as one moved without its git folder would.
    let broken = place.remotes.with_file_name("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join(".git"), "gitdir: nowhere\n").unwrap();
    let prefixes = place.prefixes();

    let missing = lender(&LiveGit, &prefixes, &broken.join("gone"), &hello);
    let not_a_clone = lender(&LiveGit, &prefixes, &broken, &hello);
    let elsewhere = lender(&LiveGit, &prefixes, &clone, &other);
    fs::write(clone.join(".git/shallow"), "").unwrap();
    let shallow = lender(&LiveGit, &prefixes, &clone, &hello);
    let _ = git(&clone, &["remote", "remove", "origin"]);
    let no_remote = lender(&LiveGit, &prefixes, &clone, &hello);

    assert_eq!(missing, Err(CloneRefusal::NotAFolder));
    assert!(
        matches!(&not_a_clone, Err(CloneRefusal::NotAClone { git_says }) if !git_says.is_empty()),
        "{not_a_clone:?}"
    );
    assert_eq!(elsewhere, Err(CloneRefusal::NoRemote));
    assert_eq!(shallow, Err(CloneRefusal::Shallow));
    assert_eq!(no_remote, Err(CloneRefusal::NoRemote));
}

#[test]
fn requests_check_a_clone_as_the_worker_will() {
    let place = Place::new("lender-request");
    let hello = named("octo-org/hello");
    let folder = place.user_clone(&hello);
    let (_versions, answers) = channel();
    let (mirrors, _starts) = place.start_faked_from(
        place.clone_source(),
        Announce::cloning_only(Box::new(|_| {})),
        answers,
    );

    let lends = mirrors.requests().check_clone(&folder, &hello);
    let refused = mirrors
        .requests()
        .check_clone(&folder, &named("octo-org/other"));
    mirrors.stop(PLENTY);

    assert_eq!(lends, Ok(()));
    assert_eq!(refused, Err(CloneRefusal::NoRemote));
}

#[test]
fn a_mirror_that_borrows_clones_with_the_lenders_objects_and_keeps_none_of_its_links() {
    let place = Place::new("borrow-args");
    let hello = named("octo-org/hello");
    let folder = place.user_clone(&hello);
    let (_versions, answers) = channel();
    let (mirrors, starts) = place.start_faked_from(
        place.clone_source(),
        Announce::cloning_only(Box::new(|_| {})),
        answers,
    );

    mirrors.requests().clone_mirror(hello, borrow(&folder));
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let args = clone.args.clone();
    clone.fail();
    mirrors.stop(PLENTY);

    let reference = args.iter().position(|arg| arg == "--reference");
    let lent = reference.and_then(|at| args.get(at + 1));
    assert_eq!(
        lent.map(PathBuf::from),
        Some(folder.join(".git")),
        "{args:?}"
    );
    assert!(args.contains(&"--dissociate".to_owned()), "{args:?}");
}

#[test]
fn a_lender_gone_by_the_clone_leaves_it_to_download_everything() {
    let place = Place::new("borrow-gone");
    let hello = named("octo-org/hello");
    let (_versions, answers) = channel();
    let (mirrors, starts) = place.start_faked_from(
        place.clone_source(),
        Announce::cloning_only(Box::new(|_| {})),
        answers,
    );

    let gone = place.remotes.with_file_name("gone");
    mirrors.requests().clone_mirror(hello, borrow(&gone));
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let args = clone.args.clone();
    clone.fail();
    mirrors.stop(PLENTY);

    assert!(args.first().is_some_and(|arg| arg == "clone"), "{args:?}");
    assert!(!args.contains(&"--reference".to_owned()), "{args:?}");
}

#[test]
fn a_real_borrowed_mirror_stands_alone_and_the_lender_is_untouched() {
    let place = Place::new("borrow-real");
    let hello = named("octo-org/hello");
    let folder = place.user_clone(&hello);
    let before = snapshot(&folder);
    let (mirrors, announced) = place.start();

    mirrors
        .requests()
        .clone_mirror(hello.clone(), borrow(&folder));
    let clone = one_clone(&announced);
    mirrors.stop(PLENTY);

    let mirror = place.folders.mirror(&hello);
    assert!(!clone.is_empty());
    assert!(mirror.join("HEAD").exists(), "{}", mirror.display());
    assert!(!mirror.join("objects/info/alternates").exists());
    assert_eq!(snapshot(&folder), before);
}

#[test]
fn a_clone_nobody_has_answered_for_asks_and_starts_on_the_answer() {
    let place = Place::new("borrow-ask");
    let hello = named("octo-org/hello");
    let (mirrors, starts, asked) = start_asking(&place);
    let requests = mirrors.requests();

    requests.clone_mirror(hello.clone(), None);
    let question = asked.recv_timeout(PLENTY).unwrap();
    requests.clone_mirror(hello.clone(), None);
    let held = starts.recv_timeout(A_MOMENT).is_err();
    requests.clone_mirror_next(hello, download());
    let answered = asked.recv_timeout(PLENTY).unwrap();
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let cloned = clone.args.first().is_some_and(|arg| arg == "clone");
    clone.fail();
    mirrors.stop(PLENTY);

    assert_eq!(question.as_deref(), Some("octo-org/hello"));
    assert!(held);
    assert_eq!(answered, None);
    assert!(cloned);
    assert!(asked.try_recv().is_err());
}

#[test]
fn the_question_names_the_first_repository_that_waits_and_then_the_next() {
    let place = Place::new("borrow-ask-two");
    let [first, second] = [named("octo-org/first"), named("octo-org/second")];
    let (mirrors, starts, asked) = start_asking(&place);
    let requests = mirrors.requests();

    requests.clone_mirror(first.clone(), None);
    requests.clone_mirror(second, None);
    let named_first = asked.recv_timeout(PLENTY).unwrap();
    requests.clone_mirror(first, download());
    let named_second = asked.recv_timeout(PLENTY).unwrap();
    let clone = starts.recv_timeout(PLENTY).unwrap();
    clone.fail();
    mirrors.stop(PLENTY);

    assert_eq!(named_first.as_deref(), Some("octo-org/first"));
    assert_eq!(named_second.as_deref(), Some("octo-org/second"));
}

#[test]
fn an_app_that_hears_only_of_clones_still_holds_a_clone_nobody_answered_for() {
    let place = Place::new("borrow-ask-quiet");
    let (_versions, answers) = channel();
    let (mirrors, starts) = place.start_faked_from(
        place.clone_source(),
        Announce::cloning_only(Box::new(|_| {})),
        answers,
    );

    mirrors
        .requests()
        .clone_mirror(named("octo-org/hello"), None);
    let held = starts.recv_timeout(A_MOMENT).is_err();
    mirrors.stop(PLENTY);

    assert!(held);
}
