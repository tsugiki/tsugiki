//! The PR view's Files tab: the PR's diff from its Repository's mirror, or why it isn't there yet (ADR
//! 0008). The window asks for the rows in chunks (ADR 0001).

use std::ops::Range;

use serde::Serialize;
use specta::Type;

use crate::diff::{self, DiffFile, DiffRow, Diffs, Held, PrAt};
use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::CommitSha;
use crate::github::pull_requests::{PrHead, PullRequestFacts};
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
        files: Vec<DiffFile>,
        rows: u32,
    },
}

/// Where the Files tab reads from: the database, the mirrors and the diff on show. It asks the mirrors'
/// worker for a head the mirror lacks.
pub struct DiffSources<'a> {
    pub reader: &'a StoreReader,
    pub folders: &'a Folders,
    pub diffs: &'a Diffs,
    pub mirrors: &'a MirrorRequests,
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
                let at = PrAt {
                    repository: repository.clone(),
                    number,
                    head: head.clone(),
                };
                let target = &pr.target_branch;
                let open = sources
                    .diffs
                    .get_or_read(&at, || diff::read(&mirror, target, &head))?;
                Ok(Some(Self::Ready {
                    head,
                    files: open.diff.files().to_vec(),
                    rows: open.diff.row_count(),
                }))
            }
        }
    }

    /// The rows in `range` of `pr`'s diff, cut to its end. They come from the diff on show, unless the
    /// user has opened another PR since.
    pub fn rows(
        sources: &DiffSources<'_>,
        pr: &PrAt,
        range: Range<u32>,
    ) -> Result<Vec<DiffRow>, Error> {
        let open = sources.diffs.get_or_read(pr, || {
            let facts = facts(sources.reader, &pr.repository, pr.number)?;
            let Some(facts) = facts else {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    &format!("Tsugiki doesn't hold PR #{}.", pr.number),
                ));
            };
            let mirror = sources.folders.mirror(&pr.repository);
            diff::read(&mirror, &facts.target_branch, &pr.head)
        })?;
        Ok(open.diff.rows(range).to_vec())
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
    use crate::diff::tests::Mirrored;
    use crate::mirror::HeardRequests;
    use crate::workbench::testing::{ViewWorld, mine};

    /// A database and a mirror, where the PR's head and Target branch are those of `mirrored`.
    struct World {
        view: ViewWorld,
        folders: Folders,
        diffs: Diffs,
        mirrors: MirrorRequests,
        heard: HeardRequests,
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
            }
        }

        fn sources(&self) -> DiffSources<'_> {
            DiffSources {
                reader: &self.view.reader,
                folders: &self.folders,
                diffs: &self.diffs,
                mirrors: &self.mirrors,
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
        let whole = mirrored.diff();
        world.mirrors(&mirrored);
        world.has_pr(Some(head.clone()), "main");

        let files = world.files().unwrap();
        let pr = PrAt {
            repository: world.repository(),
            number: 7,
            head: head.clone(),
        };
        let rows = PrFiles::rows(&world.sources(), &pr, 0..3).unwrap();

        let expected = PrFiles::Ready {
            head,
            files: whole.files().to_vec(),
            rows: whole.row_count(),
        };
        assert_eq!(files, expected);
        assert_eq!(rows, whole.rows(0..3));
        mirrored.finish();
        world.finish();
    }

    #[test]
    fn rows_of_a_diff_no_longer_on_show_are_read_again() {
        let world = World::new("read-again");
        let mirrored = Mirrored::new("pr-files-read-again");
        let head = mirrored.head.clone();
        let whole = mirrored.diff();
        world.mirrors(&mirrored);
        world.has_pr(Some(head.clone()), "main");
        let pr = PrAt {
            repository: world.repository(),
            number: 7,
            head,
        };
        let elsewhere = PrAt {
            number: 8,
            ..pr.clone()
        };

        let rows = PrFiles::rows(&world.sources(), &pr, 3..5).unwrap();
        let unknown = PrFiles::rows(&world.sources(), &elsewhere, 0..5).unwrap_err();

        assert_eq!(rows, whole.rows(3..5));
        assert_eq!(unknown.message, "Tsugiki doesn't hold PR #8.");
        mirrored.finish();
        world.finish();
    }
}
