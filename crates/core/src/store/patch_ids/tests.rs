use time::OffsetDateTime;

use super::*;
use crate::facts::activity::{ItemKey, PutAway};
use crate::facts::patch_ids::PatchId;
use crate::facts::pull_request_content::{PullRequestContent, Review};
use crate::facts::pull_requests::ReviewState;
use crate::login::Login;
use crate::store::testing::{at_second, database_before, migrated_database};
use crate::store::{Archiving, Muting, Seeing, Store};
use crate::testing::{TempPath, cpython, empty_conversation, me, pr_facts, sha_of};

fn file(path: &str, digit: char) -> FilePatch {
    let id = PatchId::parse(&digit.to_string().repeat(40)).unwrap();
    FilePatch {
        path: path.to_owned(),
        verbatim: id.clone(),
        loose: id,
        generated: digit == 'f',
    }
}

fn of(commit: &CommitSha, target_branch: &str) -> PatchesOf {
    PatchesOf {
        commit: commit.clone(),
        target_branch: target_branch.to_owned(),
    }
}

/// A colleague's open PR into main at `head`.
fn pull_request(number: u32, head: &CommitSha) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(Login::new("some-author")),
        head: Some(head.clone()),
        ..pr_facts(number)
    }
}

/// A PR's Conversation with the user's Review of `commit`.
fn reviewed_at(commit: &CommitSha) -> PullRequestContent {
    PullRequestContent {
        conversation: empty_conversation(),
        reviews: vec![Review {
            id: None,
            author: Some(me()),
            state: ReviewState::Approved,
            commit: Some(commit.clone()),
            submitted_at: OffsetDateTime::UNIX_EPOCH,
            body_html: String::new(),
            hidden: None,
            threads: vec![],
        }],
        earlier_reviews: 0,
        head: Some(commit.clone()),
        pending: vec![],
    }
}

/// A store of a database with the user signed in, and PR 1 reviewed at commit 1 and now at commit 2, and
/// the temp folder that holds it. Close the store before the folder drops: bound as `(_folder, store)`, the
/// store drops first on a panic too.
fn seeded(name: &str) -> (TempPath, Store) {
    let (path, folder) = migrated_database(name);
    let store = Store::open(&path).unwrap();
    store.remember_login(&me()).unwrap();
    store
        .save_pull_requests(&cpython(), vec![pull_request(1, &sha_of('2'))])
        .unwrap();
    store
        .save_pull_request_content(
            &cpython(),
            1,
            (reviewed_at(&sha_of('1')), OffsetDateTime::UNIX_EPOCH),
        )
        .unwrap();
    (folder, store)
}

#[test]
fn the_facts_hold_the_reviewed_commit_and_the_patch_ids_of_both_sides_once_read() {
    let (_folder, store) = seeded("patch-ids-both");
    let unread = store.work_facts(&cpython()).unwrap().reviews;
    let reviewed = vec![file("Lib/ast.py", '1'), file("Python/clinic.h", 'f')];
    let head = vec![file("Lib/ast.py", '2')];
    store
        .save_patch_ids(
            &cpython(),
            vec![
                CommitPatches {
                    of: of(&sha_of('1'), "main"),
                    files: reviewed.clone(),
                },
                CommitPatches {
                    of: of(&sha_of('2'), "main"),
                    files: head.clone(),
                },
            ],
        )
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews;
    store.close();

    assert_eq!(
        unread,
        ReviewFacts {
            reviewed: BTreeMap::from([(1, sha_of('1'))]),
            ..ReviewFacts::default()
        }
    );
    assert_eq!(
        read,
        ReviewFacts {
            reviewed: BTreeMap::from([(1, sha_of('1'))]),
            patches: BTreeMap::from([
                (of(&sha_of('1'), "main"), reviewed),
                (of(&sha_of('2'), "main"), head),
            ]),
            ..ReviewFacts::default()
        }
    );
}

#[test]
fn a_commit_with_nothing_to_diff_is_held_with_no_files() {
    let (_folder, store) = seeded("patch-ids-empty");
    let empty = CommitPatches {
        of: of(&sha_of('2'), "main"),
        files: vec![],
    };
    store.save_patch_ids(&cpython(), vec![empty]).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(read, BTreeMap::from([(of(&sha_of('2'), "main"), vec![])]));
}

#[test]
fn reading_a_commit_again_replaces_what_was_held() {
    let (_folder, store) = seeded("patch-ids-again");
    let first = CommitPatches {
        of: of(&sha_of('2'), "main"),
        files: vec![file("Lib/ast.py", '1'), file("Lib/os.py", '3')],
    };
    let again = CommitPatches {
        files: vec![file("Lib/ast.py", '2')],
        ..first.clone()
    };
    store.save_patch_ids(&cpython(), vec![first]).unwrap();
    store.save_patch_ids(&cpython(), vec![again]).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(
        read,
        BTreeMap::from([(of(&sha_of('2'), "main"), vec![file("Lib/ast.py", '2')])])
    );
}

#[test]
fn patch_ids_against_another_branch_or_of_a_commit_no_check_compares_stay_out() {
    let (_folder, store) = seeded("patch-ids-other");
    let elsewhere = [of(&sha_of('2'), "3.14"), of(&sha_of('3'), "main")].map(|of| CommitPatches {
        of,
        files: vec![file("Lib/ast.py", '1')],
    });
    store
        .save_patch_ids(&cpython(), elsewhere.to_vec())
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_save_of_unchecked_commits_or_last_checks_that_fails_says_so() {
    let store = Store::stopped();
    let last = LastCheck {
        reviewed: sha_of('1'),
        head: sha_of('2'),
    };

    let unchecked = store.save_unchecked(&cpython(), vec![]);
    let checks = store.save_last_checks(&cpython(), vec![(1, last)]);
    let findings = store.save_findings(&cpython(), Findings::default(), OffsetDateTime::UNIX_EPOCH);

    assert!(unchecked.is_err(), "{unchecked:?}");
    assert!(checks.is_err(), "{checks:?}");
    assert!(findings.is_err(), "{findings:?}");
}

fn unchecked(commit: char, reason: CantCheck) -> UncheckedCommit {
    UncheckedCommit {
        of: of(&sha_of(commit), "main"),
        reason,
    }
}

#[test]
fn the_commits_the_worker_cant_read_replace_those_it_couldnt_before() {
    let (_folder, store) = seeded("patch-ids-unchecked");
    let old_git = CantCheck::OldGit {
        version: "git version 2.38.1".to_owned(),
        needed: "2.39".to_owned(),
    };
    let before = vec![
        unchecked('1', CantCheck::Refused),
        unchecked('2', CantCheck::Cloning),
    ];
    store.save_unchecked(&cpython(), before).unwrap();
    let first = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    let now = vec![unchecked('2', old_git.clone())];
    store.save_unchecked(&cpython(), now).unwrap();

    let then = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    store.close();

    assert_eq!(
        first,
        BTreeMap::from([
            (of(&sha_of('1'), "main"), CantCheck::Refused),
            (of(&sha_of('2'), "main"), CantCheck::Cloning),
        ])
    );
    assert_eq!(then, BTreeMap::from([(of(&sha_of('2'), "main"), old_git)]));
}

#[test]
fn each_reason_comes_back_as_it_was_kept() {
    let (_folder, store) = seeded("patch-ids-reasons");
    let reasons = [
        CantCheck::Broken,
        CantCheck::NoGit,
        CantCheck::OldGit {
            version: "git version 2.29.2".to_owned(),
            needed: "2.30".to_owned(),
        },
    ];
    let mut read = vec![];
    for reason in reasons.clone() {
        store
            .save_unchecked(&cpython(), vec![unchecked('2', reason)])
            .unwrap();
        let facts = store.work_facts(&cpython()).unwrap();
        read.extend(facts.reviews.unchecked.into_values());
    }
    store.close();

    assert_eq!(read, reasons);
}

#[test]
fn a_new_run_forgets_why_the_worker_couldnt_read_patch_ids() {
    let (_folder, store) = seeded("patch-ids-forget");
    store
        .save_unchecked(&cpython(), vec![unchecked('2', CantCheck::Cloning)])
        .unwrap();

    store.forget_unchecked().unwrap();
    let read = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    store.close();

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_reason_the_store_doesnt_know_fails_the_read() {
    let (_folder, store) = seeded("patch-ids-unknown-reason");
    store
        .save_unchecked(&cpython(), vec![unchecked('2', CantCheck::NoGit)])
        .unwrap();
    // Only a file from a newer Tsugiki, or one changed by hand, says this, which our functions refuse.
    let changed = store
        .call(|connection| {
            connection.execute(
                "UPDATE unchecked_commits SET git_version = 'git version 2.0'",
                [],
            )
        })
        .unwrap();
    assert_eq!(changed, Ok(1));

    let read = store.work_facts(&cpython());
    store.close();

    assert!(read.is_err(), "{read:?}");
}

#[test]
fn the_last_check_of_a_pr_brings_the_patch_ids_of_the_head_it_compared() {
    let (_folder, store) = seeded("patch-ids-last-check");
    let earlier = CommitPatches {
        of: of(&sha_of('3'), "main"),
        files: vec![file("Lib/ast.py", '3')],
    };
    store.save_patch_ids(&cpython(), vec![earlier]).unwrap();
    let first = LastCheck {
        reviewed: sha_of('1'),
        head: sha_of('4'),
    };
    let last = LastCheck {
        reviewed: sha_of('1'),
        head: sha_of('3'),
    };
    store
        .save_last_checks(&cpython(), vec![(1, first)])
        .unwrap();
    store
        .save_last_checks(&cpython(), vec![(1, last.clone())])
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews;
    store.close();

    assert_eq!(read.last_checks, BTreeMap::from([(1, last)]));
    assert_eq!(
        read.patches,
        BTreeMap::from([(of(&sha_of('3'), "main"), vec![file("Lib/ast.py", '3')])])
    );
}

#[test]
fn the_last_check_of_another_review_brings_no_patch_ids() {
    let (_folder, store) = seeded("patch-ids-last-other");
    let earlier = CommitPatches {
        of: of(&sha_of('3'), "main"),
        files: vec![file("Lib/ast.py", '3')],
    };
    store.save_patch_ids(&cpython(), vec![earlier]).unwrap();
    let other = LastCheck {
        reviewed: sha_of('9'),
        head: sha_of('3'),
    };
    store
        .save_last_checks(&cpython(), vec![(1, other)])
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(read, BTreeMap::new());
}

/// The finding that the user's Review of commit `reviewed` on PR `number` is stale against commit `head`.
fn found(number: u32, reviewed: char, head: char) -> Found {
    Found {
        number,
        reviewed: sha_of(reviewed),
        head: sha_of(head),
    }
}

fn found_only(found: Vec<Found>) -> Findings {
    Findings {
        found,
        ended: vec![],
    }
}

#[test]
fn a_finding_keeps_its_moment_and_acknowledgement_until_a_new_head_replaces_it_or_it_ends() {
    let (_folder, store) = seeded("patch-ids-findings");
    let stale_found = || store.work_facts(&cpython()).unwrap().reviews.stale_found;

    let both = vec![found(1, '1', '2'), found(2, '1', '2')];
    store
        .save_findings(&cpython(), found_only(both), at_second(10))
        .unwrap();
    let first = stale_found();
    let seeing = Seeing {
        numbers: vec![1, 2],
        up_to: Some(at_second(15)),
        findings: vec![first[&1].on(1), first[&2].on(2)],
    };
    store.mark_seen(&cpython(), seeing).unwrap();
    let acknowledged = stale_found();
    store
        .save_findings(
            &cpython(),
            found_only(vec![found(2, '1', '3')]),
            at_second(20),
        )
        .unwrap();
    let ended = Findings {
        found: vec![],
        ended: vec![1],
    };
    store
        .save_findings(&cpython(), ended, at_second(30))
        .unwrap();
    let after = stale_found();
    store.close();

    let at = |head: char, at: i64, acknowledged: bool| StaleFound {
        reviewed: sha_of('1'),
        head: sha_of(head),
        at: at_second(at),
        acknowledged,
    };
    let fresh = at('2', 10, false);
    assert_eq!(first, BTreeMap::from([(1, fresh.clone()), (2, fresh)]));
    let marked = at('2', 10, true);
    assert_eq!(
        acknowledged,
        BTreeMap::from([(1, marked.clone()), (2, marked)])
    );
    assert_eq!(after, BTreeMap::from([(2, at('3', 20, false))]));
}

#[test]
fn a_mark_acknowledges_only_the_finding_the_window_drew() {
    let (_folder, store) = seeded("patch-ids-findings-drawn");
    store
        .save_findings(
            &cpython(),
            found_only(vec![found(1, '1', '3')]),
            at_second(20),
        )
        .unwrap();
    let kept = store.work_facts(&cpython()).unwrap().reviews.stale_found[&1].on(1);

    let drawn = [
        StaleFinding {
            head: sha_of('2'),
            ..kept.clone()
        },
        StaleFinding {
            reviewed: sha_of('4'),
            ..kept.clone()
        },
        StaleFinding {
            found_at: at_second(10),
            ..kept.clone()
        },
        StaleFinding { number: 2, ..kept },
    ];
    let seeing = Seeing {
        numbers: vec![1, 2],
        up_to: Some(at_second(25)),
        findings: drawn.to_vec(),
    };
    store.mark_seen(&cpython(), seeing).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.stale_found;
    store.close();
    assert!(!read[&1].acknowledged, "{read:?}");
}

#[test]
fn archive_and_mute_acknowledge_the_findings_they_carry() {
    let (_folder, store) = seeded("patch-ids-findings-hidden");
    let both = vec![found(1, '1', '2'), found(2, '1', '2')];
    store
        .save_findings(&cpython(), found_only(both), at_second(10))
        .unwrap();
    let kept = store.work_facts(&cpython()).unwrap().reviews.stale_found;

    let archiving = Archiving {
        key: ItemKey::PullRequest { number: 1 },
        numbers: vec![1],
        up_to: Some(at_second(15)),
        put_away: PutAway::Now,
        findings: vec![kept[&1].on(1)],
    };
    store.archive(&cpython(), archiving, at_second(15)).unwrap();
    let muting = Muting {
        key: ItemKey::PullRequest { number: 2 },
        numbers: vec![2],
        up_to: Some(at_second(15)),
        findings: vec![kept[&2].on(2)],
    };
    store.mute(&cpython(), muting, at_second(15)).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.stale_found;
    store.close();
    assert!(read[&1].acknowledged && read[&2].acknowledged, "{read:?}");
}

#[test]
fn only_a_stale_review_the_window_could_show_before_the_migration_counts_as_acknowledged() {
    // Each PR's head moved on from the user's last Review, but PR 2's. PR 1's last check of that Review
    // compared head a2, before a push to a5. The worker couldn't read PR 3's head, nor PR 4's reviewed
    // commit, which a colleague reviewed again since. PR 5's last check compared an older Review. PR 6's
    // head couldn't be read against another Target branch. No check ran on PR 7. PR 8's last check
    // compared head 82, but the patch-ids of its head 83 were read too, and of PR 9's head alone.
    let (path, _folder) = database_before(
        "patch-ids-before-findings",
        super::STALE_REVIEWS,
        "INSERT INTO viewer (id, login) VALUES (1, 'octocat');
         INSERT INTO pull_requests (repository, number, title, state, is_draft, author, target_branch, ci,
                                    updated_at, head)
         VALUES ('python/cpython', 1, 'PR 1', 'OPEN', 0, 'some-author', 'main', NULL, 1, 'a5000000'),
                ('python/cpython', 2, 'PR 2', 'OPEN', 0, 'some-author', 'main', NULL, 1, 'b1000000'),
                ('python/cpython', 3, 'PR 3', 'OPEN', 0, 'some-author', 'main', NULL, 1, 'c3000000'),
                ('python/cpython', 4, 'PR 4', 'OPEN', 0, 'some-author', 'main', NULL, 1, 'd4000000'),
                ('python/cpython', 5, 'PR 5', 'OPEN', 0, 'some-author', 'main', NULL, 1, 'e2000000'),
                ('python/cpython', 6, 'PR 6', 'OPEN', 0, 'some-author', 'main', NULL, 1, 'f6000000'),
                ('python/cpython', 7, 'PR 7', 'OPEN', 0, 'some-author', 'main', NULL, 1, '77000000'),
                ('python/cpython', 8, 'PR 8', 'OPEN', 0, 'some-author', 'main', NULL, 1, '83000000'),
                ('python/cpython', 9, 'PR 9', 'OPEN', 0, 'some-author', 'main', NULL, 1, '93000000');
         INSERT INTO issue_content (repository, number, body_html, earlier_comments)
         VALUES ('python/cpython', 1, '', 0), ('python/cpython', 2, '', 0), ('python/cpython', 3, '', 0),
                ('python/cpython', 4, '', 0), ('python/cpython', 5, '', 0), ('python/cpython', 6, '', 0),
                ('python/cpython', 7, '', 0), ('python/cpython', 8, '', 0), ('python/cpython', 9, '', 0);
         INSERT INTO pr_reviews (repository, number, position, author, state, commit_sha, submitted_at,
                                 body_html)
         VALUES ('python/cpython', 1, 0, 'octocat', 'APPROVED', 'a1000000', 1, ''),
                ('python/cpython', 2, 0, 'octocat', 'APPROVED', 'b1000000', 1, ''),
                ('python/cpython', 3, 0, 'octocat', 'APPROVED', 'c1000000', 1, ''),
                ('python/cpython', 4, 0, 'octocat', 'APPROVED', 'd1000000', 1, ''),
                ('python/cpython', 4, 1, 'some-reviewer', 'APPROVED', 'd4000000', 2, ''),
                ('python/cpython', 5, 0, 'octocat', 'APPROVED', 'e1000000', 1, ''),
                ('python/cpython', 6, 0, 'octocat', 'APPROVED', 'f1000000', 1, ''),
                ('python/cpython', 7, 0, 'octocat', 'APPROVED', '71000000', 1, ''),
                ('python/cpython', 8, 0, 'octocat', 'APPROVED', '81000000', 1, ''),
                ('python/cpython', 9, 0, 'octocat', 'APPROVED', '91000000', 1, '');
         INSERT INTO last_checks (repository, number, reviewed_commit, head_commit)
         VALUES ('python/cpython', 1, 'a1000000', 'a2000000'),
                ('python/cpython', 3, 'c1000000', 'c2000000'),
                ('python/cpython', 5, 'e9000000', 'e2000000'),
                ('python/cpython', 8, '81000000', '82000000');
         INSERT INTO patched_commits (repository, commit_sha, target_branch)
         VALUES ('python/cpython', '81000000', 'main'), ('python/cpython', '83000000', 'main'),
                ('python/cpython', '93000000', 'main');
         INSERT INTO unchecked_commits (repository, commit_sha, target_branch, reason)
         VALUES ('python/cpython', 'c3000000', 'main', 'no_git'),
                ('python/cpython', 'd1000000', 'main', 'no_git'),
                ('python/cpython', 'f6000000', '3.13', 'no_git');",
    );

    let store = Store::open(&path).unwrap();
    let read = store.work_facts(&cpython()).unwrap().reviews.stale_found;
    store.close();

    let found = |reviewed: &str, head: &str| StaleFound {
        reviewed: CommitSha::parse(reviewed).unwrap(),
        head: CommitSha::parse(head).unwrap(),
        at: OffsetDateTime::UNIX_EPOCH,
        acknowledged: true,
    };
    let expected = BTreeMap::from([
        (1, found("a1000000", "a2000000")),
        (3, found("c1000000", "c3000000")),
        (4, found("d1000000", "d4000000")),
        (8, found("81000000", "83000000")),
    ]);
    assert_eq!(read, expected);
}
