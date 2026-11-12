//! The labels of an Issue or a PR, which a search of the cache matches with `label:` (#463).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::pull_requests::Nodes;

/// The name of a label, as GitHub spells it. It reaches the UI as a plain string.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Label(String);

/// GitHub's label names ignore case, so a Repository can't hold both `docs` and `Docs`, and `label:Docs`
/// finds either.
impl PartialEq for Label {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Label {}

impl Label {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One of the labels an Issue's or a PR's fields ask for, as `labels(first: 100) { nodes { name } }`. A
/// CPython item carries a handful, so the first 100 are all of them. GitHub's `first` doesn't change what a
/// query costs here.
#[derive(Deserialize)]
pub(super) struct LabelWire {
    name: Label,
}

/// The names of `labels`, in the order the store reads them back, so facts from GitHub and from the store
/// compare equal. GitHub's schema lets the whole list be null, which holds none.
pub(super) fn label_names(labels: Option<Nodes<LabelWire>>) -> Vec<Label> {
    let labels = labels.into_iter().flat_map(|labels| labels.nodes);
    let mut names: Vec<Label> = labels.flatten().map(|label| label.name).collect();
    names.sort_by_cached_key(|label| label.as_str().to_ascii_lowercase());
    names
}

#[cfg(test)]
mod tests;
