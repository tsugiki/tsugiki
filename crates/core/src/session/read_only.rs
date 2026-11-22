//! The GitHub thread's judgement of the Repositories the token can't write to, from the scopes GitHub sends
//! with each answer, reads included (#398), and the news for the window when it changes a Repository's
//! mark. The window hears of the connection from the status.

use crate::error::{Error, ErrorKind};
use crate::github::GitHub;
use crate::github::in_flight::Asking;
use crate::read_only::{TokenSeen, token_cant_write};
use crate::repository::{RepositoryName, Visibility};
use crate::status::{Change, Status, StatusBoard};
use crate::store::{Store, TokenJudged};
use crate::token::WriteScope;

/// What the GitHub thread last judged and told, so an answer that changes nothing asks nothing of GitHub or
/// the store, and the window hears only what changed.
#[derive(Default)]
pub(super) struct WriteJudge {
    /// What the store keeps the marks from: the token, its scopes and the Repositories the settings list.
    judged: Option<Judgement>,
    /// Each Repository's visibility as GitHub answered this run, which a token that writes to public
    /// repositories alone needs.
    visibility: Vec<(RepositoryName, Visibility)>,
}

#[derive(Debug, PartialEq, Eq)]
struct Judgement {
    token: TokenSeen,
    scopes: Option<WriteScope>,
    listed: Vec<RepositoryName>,
}

impl WriteJudge {
    /// Follows what GitHub's last answer said of the token, after a turn of the GitHub thread, and gives
    /// each of `listed`, the Repositories the settings list, whose mark changed, for [`tell`]. A question of
    /// a Repository's visibility that GitHub can't answer now waits for the next turn.
    pub(super) fn follow(
        &mut self,
        store: &Store,
        github: &GitHub,
        listed: Vec<RepositoryName>,
    ) -> Result<Vec<RepositoryName>, Error> {
        let Some(grant) = github.grant() else {
            return Ok(vec![]);
        };
        let judgement = Judgement {
            token: TokenSeen {
                kind: github.token_kind(),
                expires: grant.expires,
            },
            scopes: grant.scopes,
            listed,
        };
        if self.judged.as_ref() == Some(&judgement) {
            return Ok(vec![]);
        }
        let judged = match judgement.scopes {
            None => TokenJudged::Silent,
            Some(scope) => {
                let mut cant = Vec::new();
                for repository in &judgement.listed {
                    let Some(cant_write) = self.judge(github, scope, repository) else {
                        return Ok(vec![]);
                    };
                    if cant_write {
                        cant.push(repository.clone());
                    }
                }
                TokenJudged::CantWrite(cant)
            }
        };
        let changed = store.judge_read_only(judgement.token, judgement.listed.clone(), judged)?;
        self.judged = Some(judgement);
        Ok(changed)
    }

    /// Whether a token whose scopes reach `scope` can't write to `repository`, asking GitHub its visibility
    /// when only that can tell. Nothing, when GitHub can't answer now.
    fn judge(
        &mut self,
        github: &GitHub,
        scope: WriteScope,
        repository: &RepositoryName,
    ) -> Option<bool> {
        let known = self.visibility_of(repository);
        if let Some(judged) = token_cant_write(scope, known) {
            return Some(judged);
        }
        let asked = Asking::Visibility {
            repository: repository.clone(),
        };
        let visibility = match github.asking(asked, || github.repository_visibility(repository)) {
            Ok(visibility) => visibility,
            // A public repository shows itself to any token, so one this token can't see is private.
            Err(problem) if matches!(problem.kind, ErrorKind::NotFound) => Visibility::Private,
            Err(problem) if problem.kind.speaks_for_all_of_github() => return None,
            // Asking again would fail the same way, so the token is judged able to write there, and a
            // write GitHub refuses says otherwise.
            Err(problem) => {
                log::warn!(
                    "Tsugiki couldn't ask GitHub whether {repository} is private: {}",
                    problem.message
                );
                Visibility::Public
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

/// Tells the window of each Repository whose mark `followed` changed, with a change that names no item, so
/// its screens read its state again, or that the store failed.
pub(super) fn tell(board: &mut StatusBoard, followed: Result<Vec<RepositoryName>, Error>) {
    match followed {
        Ok(changed) => {
            for repository in changed {
                board.changed(&Change {
                    repository,
                    items: vec![],
                });
            }
        }
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

#[cfg(test)]
mod tests;
