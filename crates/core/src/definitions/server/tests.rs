use super::*;

fn counted(done: u32) -> Indexing {
    Indexing::Building(Some(IndexedFiles { done, total: 452 }))
}

#[test]
fn a_new_state_or_the_first_count_is_news_and_a_new_count_only_once_a_second_has_passed() {
    let soon = Duration::from_millis(100);
    let news = |told, now| Indexing::news_since(now, told, soon);

    assert!(news(Indexing::Unreported, Indexing::Building(None)));
    assert!(news(Indexing::Building(None), counted(1)));
    assert!(!news(counted(1), counted(2)));
    assert!(counted(2).news_since(counted(1), NEWS_EVERY));
    assert!(news(counted(451), Indexing::Built));
    assert!(news(Indexing::Built, Indexing::Ended));
    assert!(!news(Indexing::Built, Indexing::Built));
}
