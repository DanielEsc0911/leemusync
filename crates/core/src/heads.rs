//! Version graph. Spec: docs/specs/sync-model.md (Heads and divergence).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::VersionId;

/// Version → its parents.
pub type Graph = BTreeMap<VersionId, Vec<VersionId>>;

/// Versions that no other version lists as a parent, sorted.
#[must_use]
pub fn heads(graph: &Graph) -> Vec<VersionId> {
    let parents: BTreeSet<VersionId> = graph.values().flatten().copied().collect();
    graph
        .keys()
        .filter(|id| !parents.contains(id))
        .copied()
        .collect()
}

/// True when `ancestor` is `child` or reachable through parents.
#[must_use]
pub fn descends_from(graph: &Graph, child: VersionId, ancestor: VersionId) -> bool {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([child]);
    while let Some(id) = queue.pop_front() {
        if id == ancestor {
            return true;
        }
        if seen.insert(id) {
            queue.extend(graph.get(&id).into_iter().flatten().copied());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn v(n: u64) -> VersionId {
        VersionId::from_parts(n, [0; 10])
    }

    #[test]
    fn a_chain_has_one_head() {
        let g: Graph = [(v(1), vec![]), (v(2), vec![v(1)]), (v(3), vec![v(2)])].into();
        assert_eq!(heads(&g), vec![v(3)]);
        assert!(descends_from(&g, v(3), v(1)));
        assert!(!descends_from(&g, v(1), v(3)));
        assert!(descends_from(&g, v(2), v(2)));
    }

    #[test]
    fn a_fork_has_two_heads_and_a_merge_has_one() {
        let mut g: Graph = [(v(1), vec![]), (v(2), vec![v(1)]), (v(3), vec![v(1)])].into();
        assert_eq!(heads(&g), vec![v(2), v(3)]);
        g.insert(v(4), vec![v(2), v(3)]);
        assert_eq!(heads(&g), vec![v(4)]);
    }

    #[test]
    fn missing_parents_are_tolerated() {
        let g: Graph = [(v(5), vec![v(1)])].into();
        assert_eq!(heads(&g), vec![v(5)]);
        assert!(descends_from(&g, v(5), v(1)));
    }
}
