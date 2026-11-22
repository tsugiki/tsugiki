//! The GitHub thread's judgement of the Repositories the token can't write to, from the scopes GitHub sends
//! with each answer, reads included (#398), and the news for the window when it changes a Repository's
//! mark. The window hears of the connection from the status.

use std::time::{Duration, Instant};

use crate::error::{Error, ErrorKind};
use crate::github::GitHub;
use crate::github::in_flight::Asking;
use crate::read_only::{TokenSeen, token_cant_write};
use crate::repository::{RepositoryName, Visibility};
use crate::status::{Change, StatusBoard};
use crate::store::Store;
use crate::token::{TokenKind, WriteScope};

/// What the GitHub thread has judged and saved, so a turn that brings nothing new asks nothing of GitHub or
/// the store, and the window hears only what changed. Each Repository is judged and saved on its own, so
/// one whose visibility GitHub can't answer now holds up no other.
pub(super) struct WriteJudge {
    /// How long Tsugiki waits to ask GitHub a Repository's visibility again after GitHub couldn't answer.
    retry: Duration,
    /// The kind of token whose marks alone the store keeps, once it has forgotten the others'.
    kind: Option<TokenKind>,
    /// The token, as GitHub's answers name it, whose marks alone the store keeps.
    token: Option<TokenSeen>,
    /// The scope the Repositories in `judged` were judged by.
    scope: Option<WriteScope>,
    /// The Repositories judged for `token` and `scope`, saved or not: a save that failed waits for another
    /// token or scope rather than trying again each turn.
    judged: Vec<RepositoryName>,
    /// The Repositories the settings listed when the store last forgot the marks of the others, for
    /// `scope`. Nothing, until then.
    listed: Option<Vec<RepositoryName>>,
    /// Each Repository's visibility as GitHub answered this run, which a token that writes to public
    /// repositories alone needs.
    visibility: Vec<(RepositoryName, Visibility)>,
    /// When Tsugiki may ask GitHub a visibility again, after a question it couldn't answer.
    ask_after: Option<Instant>,
}

/// The parts of the GitHub thread the judge works with: the window to tell, the store of the marks, and
/// GitHub, whose last answer names the token's scopes.
pub(super) struct ThreadParts<'a> {
    pub(super) board: &'a mut StatusBoard,
    pub(super) store: &'a Store,
    pub(super) github: &'a GitHub,
}

impl WriteJudge {
    /// A judge that asks GitHub a visibility again `retry` after GitHub couldn't answer one.
    pub(super) const fn new(retry: Duration) -> Self {
        Self {
            retry,
            kind: None,
            token: None,
            scope: None,
            judged: Vec::new(),
            listed: None,
            visibility: Vec::new(),
            ask_after: None,
        }
    }

    /// Follows what GitHub's last answer said of the token, after a turn of the GitHub thread, and judges
    /// each of `listed`, the Repositories the settings list, not yet judged for that token and scope. The
    /// marks of another token go first, whether or not GitHub can answer now. Each Repository whose mark
    /// changed hears a change that names no item, so its screens read its state again.
    pub(super) fn follow(&mut self, parts: ThreadParts<'_>, listed: &[RepositoryName]) {
        let ThreadParts {
            board,
            store,
            github,
        } = parts;
        let kind = github.token_kind();
        if self.kind != Some(kind) {
            self.kind = Some(kind);
            tell(board, store.forget_read_only_of_other_kinds(kind));
        }
        let Some(grant) = github.grant() else {
            return;
        };
        let token = TokenSeen {
            kind,
            expires: grant.expires,
        };
        if self.token != Some(token) {
            self.token = Some(token);
            self.judged.clear();
            tell(board, store.forget_read_only_of_other_tokens(token));
        }
        // Only an OAuth or a classic token's scopes say what it may write. GitHub can name scopes, none
        // among them, for a token of another kind, which scopes don't limit.
        let scope = match kind {
            TokenKind::OAuth | TokenKind::Classic => grant.scopes,
            TokenKind::FineGrained | TokenKind::Other => None,
        };
        let Some(scope) = scope else {
            return;
        };
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.judged.clear();
            self.listed = None;
        }
        // The window can ask the state of any Repository, so a mark the scopes no longer judge goes.
        if !self
            .listed
            .as_deref()
            .is_some_and(|known| same_repositories(known, listed))
        {
            self.listed = Some(listed.to_vec());
            tell(board, store.forget_read_only_of_unlisted(listed.to_vec()));
        }
        for repository in listed {
            if self.judged.iter().any(|judged| judged.is_same(repository)) {
                continue;
            }
            let judged = match token_cant_write(scope, self.visibility_of(repository)) {
                Some(cant_write) => Some(cant_write),
                None if self.may_ask(board, github) => self.ask(github, scope, repository),
                None => None,
            };
            let Some(cant_write) = judged else {
                continue;
            };
            let saved = if cant_write {
                store.mark_read_only(token, repository)
            } else {
                store.clear_read_only(repository)
            };
            let changed =
                saved.map(|changed| changed.then(|| repository.clone()).into_iter().collect());
            tell(board, changed);
            self.judged.push(repository.clone());
        }
    }

    /// Whether GitHub may be asked a visibility now: not offline, not while the rate limit holds every
    /// request, and not for `retry` after a question GitHub couldn't answer.
    fn may_ask(&self, board: &StatusBoard, github: &GitHub) -> bool {
        let waiting = self.ask_after.is_some_and(|after| Instant::now() < after);
        !waiting && !board.is_offline() && github.points_back().is_none()
    }

    /// Whether a token whose scopes reach `scope` can't write to `repository`, from the visibility GitHub
    /// names. Nothing, when GitHub can't answer now.
    fn ask(
        &mut self,
        github: &GitHub,
        scope: WriteScope,
        repository: &RepositoryName,
    ) -> Option<bool> {
        let asked = Asking::Visibility {
            repository: repository.clone(),
        };
        let visibility = match github.asking(asked, || github.repository_visibility(repository)) {
            Ok(visibility) => visibility,
            // A public repository shows itself to any token, so one this token can't see is private.
            Err(problem) if matches!(problem.kind, ErrorKind::NotFound) => Visibility::Private,
            // Any other failure, such as a server error that passes, leaves the question open: judging the
            // Repository on it would clear a mark that may be right until the next run.
            Err(problem) => {
                if !problem.kind.speaks_for_all_of_github() {
                    log::warn!(
                        "Tsugiki couldn't ask GitHub whether {repository} is private: {}",
                        problem.message
                    );
                }
                self.ask_after = Some(Instant::now() + self.retry);
                return None;
            }
        };
        self.visibility.push((repository.clone(), visibility));
        token_cant_write(scope, Some(visibility))
    }

    fn visibility_of(&self, repository: &RepositoryName) -> Option<Visibility> {
        self.visibility
            .iter()
            .find(|(known, _)| known.is_same(repository))
            .map(|(_, visibility)| *visibility)
    }
}

/// Whether `one` and `other` name the same Repositories in the same order, whatever their case.
fn same_repositories(one: &[RepositoryName], other: &[RepositoryName]) -> bool {
    one.len() == other.len() && one.iter().zip(other).all(|(one, other)| one.is_same(other))
}

/// Tells the window of each Repository whose mark `saved` changed, with a change that names no item, so its
/// screens read its state again. A save that failed goes to the log: the status says what GitHub answers,
/// and a mark Tsugiki couldn't save takes nothing from what it shows.
fn tell(board: &mut StatusBoard, saved: Result<Vec<RepositoryName>, Error>) {
    match saved {
        Ok(changed) => {
            for repository in changed {
                board.changed(&Change {
                    repository,
                    items: vec![],
                });
            }
        }
        Err(problem) => log::warn!(
            "Tsugiki couldn't save which Repositories the token can't write to: {}",
            problem.message
        ),
    }
}

#[cfg(test)]
mod tests;
