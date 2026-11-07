//! The PR view's Files tab: the PR's diff from its Repository's mirror, or why it isn't there yet (ADR
//! 0008). The window asks for the rows in chunks (ADR 0001).

use std::ops::Range;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::diff::{
    self, Against, Chunk, DiffFile, DiffOf, DiffRow, Diffs, Held, Highlighter, NewsEntry, OpenDiff,
    PrAt,
};
use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::CommitSha;
use crate::github::pull_requests::{PrHead, PullRequestFacts};
use crate::highlight::Span;
use crate::mirror::MirrorRequests;
use crate::repository::RepositoryName;
use crate::store::StoreReader;

/// What the Files tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PrFiles {
    /// The Repository's mirror is still being cloned, and the clone says how far it is.
    Cloning,
    /// The mirror lacks the PR's commits, and is fetching them.
    Fetching,
    /// The diff, whose rows the window asks for by number.
    Ready {
        /// The commit the diff ends at, which each chunk names.
        #[specta(type = String)]
        head: CommitSha,
        /// The merge base the diff starts from, which each chunk names too. A PR moved to another
        /// Target branch keeps its head and gets another merge base (#211).
        #[specta(type = String)]
        base: CommitSha,
        files: Vec<DiffFile>,
        rows: u32,
        /// The PR's NEWS entries, rendered above the rows.
        news: Vec<NewsEntry>,
    },
}

/// Where the Files tab reads from: the database, the mirrors, the diff on show and the trees of its files.
/// It asks the mirrors' worker for a head the mirror lacks.
pub struct DiffSources<'a> {
    pub reader: &'a StoreReader,
    pub folders: &'a Folders,
    pub diffs: &'a Diffs,
    pub mirrors: &'a MirrorRequests,
    pub highlighter: &'a Highlighter,
}

impl PrFiles {
    /// What PR `number`'s Files tab shows, or nothing if Tsugiki doesn't hold it. A diff that isn't on show
    /// takes as long as git does, and a PR whose head the mirror lacks goes ahead of the heads waiting for
    /// a fetch.
    pub fn read(
        sources: &DiffSources<'_>,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let Some(pr) = facts(sources.reader, repository, number)? else {
            return Ok(None);
        };
        let mirror = sources.folders.mirror(repository);
        if !mirror.exists() {
            return Ok(Some(Self::Cloning));
        }
        // The sync reads a PR's head the next time it reads the PR.
        let Some(head) = pr.head else {
            return Ok(Some(Self::Fetching));
        };
        let at = PrAt {
            repository: repository.clone(),
            number,
            head: head.clone(),
        };
        let target = Against::Branch(&pr.target_branch);
        // The diff on show is in the mirror, so asking git again would only cost a moment.
        if let Some(open) = sources.diffs.shown(&at, target) {
            return Ok(Some(Self::ready(&open)));
        }
        match diff::held(&mirror, &pr.target_branch, &head) {
            Held::NoHead => {
                let head = PrHead {
                    number,
                    commit: head,
                    target_branch: pr.target_branch,
                };
                sources.mirrors.fetch_first(repository.clone(), head);
                Ok(Some(Self::Fetching))
            }
            Held::NoTarget => Err(Error::new(
                ErrorKind::NotFound,
                &format!(
                    "The mirror of {repository} has no branch {}, which this PR targets.",
                    pr.target_branch
                ),
            )),
            Held::Both => {
                let read = || diff::read(&mirror, &pr.target_branch, &head);
                let open = sources.diffs.get_or_read(&at, target, read)?;
                Ok(Some(Self::ready(&open)))
            }
        }
    }

    fn ready(open: &OpenDiff) -> Self {
        Self::Ready {
            head: open.pr.head.clone(),
            base: open.base.clone(),
            files: open.diff.files().to_vec(),
            rows: open.diff.row_count(),
            news: open.news.clone(),
        }
    }

    /// The rows in `range` of the diff `of`, cut to its end. They come from the diff on show, unless the
    /// user has opened another PR since.
    pub fn rows(
        sources: &DiffSources<'_>,
        of: &DiffOf,
        range: Range<u32>,
    ) -> Result<Vec<DiffRow>, Error> {
        Ok(Self::open(sources, of)?.diff.rows(range).to_vec())
    }

    /// The spans of the rows in `range`, which highlight their code (#178). Those of a Generated file come
    /// only once the window opens it, and `opened` names those, by their place among the files (#181).
    pub fn spans(
        sources: &DiffSources<'_>,
        of: &DiffOf,
        range: Range<u32>,
        opened: &[u32],
    ) -> Result<Vec<Vec<Span>>, Error> {
        let mirror = sources.folders.mirror(&of.pr.repository);
        let open = Self::open(sources, of)?;
        let chunk = Chunk {
            rows: range,
            opened,
        };
        Ok(sources.highlighter.spans(&open, &mirror, chunk))
    }

    /// The diff `of`: the one on show, or else read again from its base, as when the user has opened
    /// another PR since. The Target branch only says which files are Generated.
    fn open(sources: &DiffSources<'_>, of: &DiffOf) -> Result<Arc<OpenDiff>, Error> {
        let DiffOf { pr, base } = of;
        sources.diffs.get_or_read(pr, Against::Base(base), || {
            let facts = facts(sources.reader, &pr.repository, pr.number)?;
            let Some(facts) = facts else {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    &format!("Tsugiki doesn't hold PR #{}.", pr.number),
                ));
            };
            let mirror = sources.folders.mirror(&pr.repository);
            diff::read_between(&mirror, &facts.target_branch, base, &pr.head)
        })
    }
}

/// PR `number`'s facts, as the database last had them.
fn facts(
    reader: &StoreReader,
    repository: &RepositoryName,
    number: u32,
) -> Result<Option<PullRequestFacts>, Error> {
    let (facts, _content) = reader.pull_request_facts(repository, number)?;
    let pr = facts
        .pull_requests
        .into_iter()
        .find(|pr| pr.number == number);
    Ok(pr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::tests::{Mirrored, git_in};
    use crate::mirror::HeardRequests;
    use crate::workbench::testing::{ViewWorld, mine};

    /// A database and a mirror, where the PR's head and Target branch are those of `mirrored`.
    struct World {
        view: ViewWorld,
        folders: Folders,
        diffs: Diffs,
        mirrors: MirrorRequests,
        heard: HeardRequests,
        highlighter: Highlighter,
    }

    impl World {
        fn new(name: &str) -> Self {
            let view = ViewWorld::new(&format!("pr-files-{name}"));
            let data = crate::testing::temp_path(&format!("pr-files-{name}-data"));
            std::fs::create_dir_all(&data).unwrap();
            let (mirrors, heard) = HeardRequests::new();
            Self {
                view,
                folders: Folders {
                    data: data.clone(),
                    settings: data,
                },
                diffs: Diffs::default(),
                mirrors,
                heard,
                highlighter: Highlighter::start(),
            }
        }

        fn sources(&self) -> DiffSources<'_> {
            DiffSources {
                reader: &self.view.reader,
                folders: &self.folders,
                diffs: &self.diffs,
                mirrors: &self.mirrors,
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

        fn files(&self) -> Option<PrFiles> {
            PrFiles::read(&self.sources(), &self.repository(), 7).unwrap()
        }

        fn finish(self) {
            std::fs::remove_dir_all(&self.folders.data).unwrap();
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
        world.finish();
    }

    #[test]
    fn a_pr_whose_head_tsugiki_hasnt_read_waits_for_the_sync() {
        let world = World::new("no-head");
        let mirrored = Mirrored::new("pr-files-no-head");
        world.mirrors(&mirrored);
        world.has_pr(None, "main");

        assert_eq!(world.files(), Some(PrFiles::Fetching));
        assert_eq!(world.heard.firsts(), []);
        mirrored.finish();
        world.finish();
    }

    #[test]
    fn a_head_the_mirror_lacks_is_fetched_first() {
        let world = World::new("fetching");
        let mirrored = Mirrored::new("pr-files-fetching");
        world.mirrors(&mirrored);
        let elsewhere = CommitSha::parse(&"3".repeat(40)).unwrap();
        world.has_pr(Some(elsewhere.clone()), "main");

        assert_eq!(world.files(), Some(PrFiles::Fetching));
        let head = PrHead {
            number: 7,
            commit: elsewhere,
            target_branch: "main".to_owned(),
        };
        assert_eq!(world.heard.firsts(), [(world.repository(), head)]);
        mirrored.finish();
        world.finish();
    }

    #[test]
    fn a_target_branch_the_mirror_lacks_says_so() {
        let world = World::new("no-target");
        let mirrored = Mirrored::new("pr-files-no-target");
        world.mirrors(&mirrored);
        world.has_pr(Some(mirrored.head.clone()), "3.14");

        let problem = PrFiles::read(&world.sources(), &world.repository(), 7).unwrap_err();

        assert_eq!(
            problem.message,
            "The mirror of python/cpython has no branch 3.14, which this PR targets."
        );
        mirrored.finish();
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
        // main marks Lib/ast.py generated, and the window has opened each file.
        let opened: Vec<u32> = (0..).take(whole.files().len()).collect();
        let all = 0..whole.row_count();
        let spans = PrFiles::spans(&world.sources(), &of, all, &opened).unwrap();

        let expected = PrFiles::Ready {
            head,
            base: read.base,
            files: whole.files().to_vec(),
            rows: whole.row_count(),
            news: read.news,
        };
        assert_eq!(files, expected);
        assert_eq!(rows, whole.rows(0..3));
        assert_eq!(spans.len(), whole.rows(0..whole.row_count()).len());
        assert_eq!(spans.iter().filter(|spans| !spans.is_empty()).count(), 13);
        mirrored.finish();
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
        let _ = git_in(&mirror, &["update-ref", "-d", "refs/heads/main"]);

        assert_eq!(world.files(), first);
        mirrored.finish();
        world.finish();
    }

    #[test]
    fn rows_of_a_diff_no_longer_on_show_are_read_again_from_its_base() {
        let world = World::new("read-again");
        let mirrored = Mirrored::new("pr-files-read-again");
        let head = mirrored.head.clone();
        let whole = mirrored.read();
        let _ = git_in(&mirrored.mirror, &["branch", "3.14", head.as_str()]);
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
        mirrored.finish();
        world.finish();
    }

    #[test]
    fn a_pr_moved_to_another_branch_shows_its_diff_against_that_branch() {
        let world = World::new("retargeted");
        let mirrored = Mirrored::new("pr-files-retargeted");
        let head = mirrored.head.clone();
        // 3.14 already holds the PR's commit, so the diff against it is empty.
        let _ = git_in(&mirrored.mirror, &["branch", "3.14", head.as_str()]);
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
        };
        assert_ne!(before, Some(expected.clone()));
        assert_eq!(world.files(), Some(expected));
        mirrored.finish();
        world.finish();
    }
}
