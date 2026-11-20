use super::{DiffSources, FilesRead};
use crate::diff::testing::Mirrored;
use crate::diff::{DiffOf, Diffs, Highlighter, PrAt};
use crate::facts::CommitSha;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::folders::Folders;
use crate::repository::RepositoryName;
use crate::workbench::testing::{ViewWorld, mine};

/// A database and a mirror, where the PR's head and Target branch are those of `mirrored`.
pub(crate) struct World {
    pub(crate) view: ViewWorld,
    pub(crate) folders: Folders,
    pub(crate) diffs: Diffs,
    pub(crate) highlighter: Highlighter,
    /// The data folder, dropped last.
    _data: crate::testing::TempPath,
}

impl World {
    pub(crate) fn new(name: &str) -> Self {
        let view = ViewWorld::new(&format!("pr-files-{name}"));
        let data = crate::testing::temp_path(&format!("pr-files-{name}-data"));
        std::fs::create_dir_all(&data).unwrap();
        Self {
            view,
            folders: crate::testing::one_folder(&data),
            diffs: Diffs::default(),
            highlighter: Highlighter::start(),
            _data: data,
        }
    }

    pub(crate) fn sources(&self) -> DiffSources<'_> {
        DiffSources {
            reader: &self.view.db.reader,
            folders: &self.folders,
            diffs: &self.diffs,
            highlighter: &self.highlighter,
        }
    }

    /// PR 7, at `head`, targeting `target`.
    pub(crate) fn has_pr(&self, head: Option<CommitSha>, target: &str) {
        let pr = PullRequestFacts {
            head,
            target_branch: target.to_owned(),
            ..mine(7)
        };
        self.view.has(vec![pr], vec![]);
    }

    /// PR 7, merged into main as `merge_commit`, from `head`. GitHub compared it with `base` at the
    /// merge, and a PR stored before Tsugiki read that has none.
    pub(crate) fn has_merged(
        &self,
        head: &CommitSha,
        merge_commit: CommitSha,
        base: Option<CommitSha>,
    ) {
        let pr = PullRequestFacts {
            state: PrState::Merged,
            merge_commit: Some(merge_commit),
            base,
            head: Some(head.clone()),
            target_branch: "main".to_owned(),
            ..mine(7)
        };
        self.view.has(vec![pr], vec![]);
    }

    /// PR 7's Conversation, as GitHub had it at `head`.
    pub(crate) fn has_conversation(&self, head: Option<CommitSha>) {
        let mut content = crate::workbench::pr_view::testing::reviewed();
        content.head = head;
        let name = self.repository();
        self.view
            .db
            .store
            .save_pull_request_content(&name, 7, (content, time::OffsetDateTime::UNIX_EPOCH))
            .unwrap();
    }

    /// The mirror of `mirrored`, where Tsugiki keeps CPython's.
    pub(crate) fn mirrors(&self, mirrored: &Mirrored) {
        let mirror = self.folders.mirror(&self.repository());
        std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        std::fs::rename(&mirrored.mirror, mirror).unwrap();
    }

    pub(crate) fn repository(&self) -> RepositoryName {
        self.view.settings().name
    }

    /// PR 7's diff from `base` to `head`.
    pub(crate) fn diff_of(&self, head: &CommitSha, base: &CommitSha) -> DiffOf {
        DiffOf {
            pr: PrAt {
                repository: self.repository(),
                number: 7,
                head: head.clone(),
            },
            base: base.clone(),
        }
    }

    /// What a read of PR 7's Files tab finds, while its clone doesn't wait for an answer.
    pub(crate) fn read(&self) -> Option<FilesRead> {
        self.sources()
            .read(&self.repository(), 7, &|| false)
            .unwrap()
    }

    pub(crate) fn finish(self) {
        self.view.finish();
    }
}
