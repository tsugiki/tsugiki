use super::*;

fn repository(name: &str) -> RepositoryName {
    RepositoryName::parse(name).unwrap()
}

fn queued(name: &str, waiting: &[Waiting]) -> Queued {
    Queued {
        repository: repository(name),
        waiting: waiting.iter().copied().collect(),
    }
}

/// Every kind of job.
const ALL: &[Waiting] = &[
    Waiting::LookupMove,
    Waiting::DemandedHeads,
    Waiting::Heads,
    Waiting::Reviewed,
    Waiting::PatchIds,
];

/// A queue of `repositories`, with a clone waiting and an upkeep due, that nothing holds back.
fn queue(repositories: Vec<Queued>) -> Queue {
    Queue {
        problem: None,
        pace: Pace::Going,
        reads_patch_ids: true,
        repositories,
        clone: Some(repository("owner/cloned")),
        upkeep: vec![repository("owner/kept")],
    }
}

/// Takes `job` out of `queue`, as running it would.
fn run(queue: &mut Queue, job: &Next) {
    let mut done = |repository: &RepositoryName, kind: Waiting| {
        let queued = (queue.repositories.iter_mut())
            .find(|queued| queued.repository.is_same(repository))
            .unwrap();
        assert!(queued.waiting.remove(&kind), "{kind:?} wasn't waiting");
    };
    match job {
        Next::MoveLookup(repository) => done(repository, Waiting::LookupMove),
        Next::FetchDemanded(repository) => done(repository, Waiting::DemandedHeads),
        Next::FetchWanted(repository) => done(repository, Waiting::Heads),
        Next::KeepReviewed(repository) => done(repository, Waiting::Reviewed),
        Next::ReadPatchIds(repository) => done(repository, Waiting::PatchIds),
        Next::Clone(_) => queue.clone = None,
        Next::Upkeep(_) => drop(queue.upkeep.remove(0)),
    }
}

/// The jobs `queue` gives, in the order they run, each taken out once it has run.
fn order(mut queue: Queue) -> Vec<Next> {
    let mut picked = Vec::new();
    while let Some(job) = next(&queue) {
        run(&mut queue, &job);
        picked.push(job);
    }
    picked
}

#[test]
fn each_kind_of_job_goes_in_its_turn() {
    let picked = order(queue(vec![queued("owner/one", ALL)]));

    let one = repository("owner/one");
    assert_eq!(
        picked,
        [
            Next::MoveLookup(one.clone()),
            Next::FetchDemanded(one.clone()),
            Next::FetchWanted(one.clone()),
            Next::KeepReviewed(one.clone()),
            Next::ReadPatchIds(one),
            Next::Clone(repository("owner/cloned")),
            Next::Upkeep(repository("owner/kept")),
        ]
    );
}

#[test]
fn a_kind_of_job_goes_ahead_of_the_kinds_after_it_in_every_repository() {
    let first = queued("owner/first", &[Waiting::Heads]);
    let second = queued(
        "owner/second",
        &[Waiting::LookupMove, Waiting::DemandedHeads],
    );

    let picked = order(queue(vec![first, second]));

    let (first, second) = (repository("owner/first"), repository("owner/second"));
    assert_eq!(
        picked[..3],
        [
            Next::MoveLookup(second.clone()),
            Next::FetchDemanded(second),
            Next::FetchWanted(first),
        ]
    );
}

#[test]
fn the_repository_whose_turn_comes_first_goes_first() {
    let first = queued("owner/first", &[Waiting::Reviewed]);
    let second = queued("owner/second", &[Waiting::Reviewed]);

    let picked = next(&queue(vec![first, second]));

    assert_eq!(picked, Some(Next::KeepReviewed(repository("owner/first"))));
}

#[test]
fn nothing_starts_without_a_usable_git() {
    let mut queue = queue(vec![queued("owner/one", ALL)]);
    queue.problem = Some(Problem::Git);

    assert_eq!(next(&queue), None);
}

#[test]
fn short_of_disk_only_clones_wait() {
    let mut queue = queue(vec![queued("owner/one", ALL)]);
    queue.problem = Some(Problem::DiskShort);

    let picked = order(queue);

    assert!(!picked.iter().any(|job| matches!(job, Next::Clone(_))));
    assert_eq!(picked.len(), 6);
}

#[test]
fn offline_only_a_move_and_upkeep_start() {
    let mut queue = queue(vec![queued("owner/one", ALL)]);
    queue.pace = Pace::Held;

    let picked = order(queue);

    assert_eq!(
        picked,
        [
            Next::MoveLookup(repository("owner/one")),
            Next::Upkeep(repository("owner/kept")),
        ]
    );
}

#[test]
fn while_fetches_wait_for_news_reads_and_clones_go_on() {
    let mut queue = queue(vec![queued("owner/one", ALL)]);
    queue.pace = Pace::Stalled;

    let picked = order(queue);

    let one = repository("owner/one");
    assert_eq!(
        picked,
        [
            Next::MoveLookup(one.clone()),
            Next::ReadPatchIds(one),
            Next::Clone(repository("owner/cloned")),
            Next::Upkeep(repository("owner/kept")),
        ]
    );
}

#[test]
fn no_patch_ids_are_read_while_the_worker_can_not_read_them() {
    let mut queue = queue(vec![queued("owner/one", &[Waiting::PatchIds])]);
    queue.reads_patch_ids = false;

    assert_eq!(next(&queue), Some(Next::Clone(repository("owner/cloned"))));
}

#[test]
fn an_idle_queue_picks_nothing() {
    let mut queue = queue(vec![queued("owner/one", &[])]);
    queue.clone = None;
    queue.upkeep.clear();

    assert_eq!(next(&queue), None);
}
