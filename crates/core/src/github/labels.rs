//! The labels of an Issue or a PR as GitHub's fields give them.

use serde::Deserialize;

use super::pull_requests::Nodes;
use crate::facts::labels::Label;

/// One of the labels an Issue's or a PR's fields ask for, as `labels(first: 100) { nodes { name } }`. A
/// CPython item carries a handful, so the first 100 are all of them. GitHub's `first` doesn't change what a
/// query costs here.
#[derive(Deserialize)]
pub(super) struct LabelWire {
    pub(super) name: Label,
}

/// The names of `labels`, in the order the store reads them back, so facts from GitHub and from the store
/// compare equal. GitHub's schema lets the whole list be null, which holds none.
pub(super) fn label_names(labels: Option<Nodes<LabelWire>>) -> Vec<Label> {
    let labels = labels.into_iter().flat_map(|labels| labels.nodes);
    let mut names: Vec<Label> = labels.flatten().map(|label| label.name).collect();
    names.sort_by_cached_key(|label| label.as_str().to_ascii_lowercase());
    names
}
