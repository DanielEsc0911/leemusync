//! What to do with one stream. Spec: docs/specs/sync-model.md (Reconciliation).

use crate::VersionId;
use crate::heads::{Graph, descends_from, heads};

/// The stream's local files compared with `base`. With no `base`, existing files count as `Dirty`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Local {
    Missing,
    Clean,
    Dirty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Local files already equal this head: record it as `base`, touch nothing.
    Adopt(VersionId),
    /// Restore this head (backup first).
    FastForward(VersionId),
    Upload {
        parents: Vec<VersionId>,
    },
    /// Upload local files first so they're safe, then report the conflict.
    UploadThenConflict {
        parents: Vec<VersionId>,
    },
    Conflict(Vec<VersionId>),
    /// A mapped account's folder disappeared: ask the player.
    LocalMissing,
    /// The remote lost the base this device saw (rollback): warn, change nothing, even if local files changed.
    RemoteBehind,
}

pub struct Input<'a> {
    pub base: Option<VersionId>,
    pub local: Local,
    pub graph: &'a Graph,
    /// A head whose files equal the local files (computed by the engine).
    pub local_matches: Option<VersionId>,
    /// The player chose to replace this account's files with the remote head.
    pub replace_local: bool,
}

#[must_use]
pub fn reconcile(input: &Input<'_>) -> Action {
    if let Some(base) = input.base
        && input.local != Local::Missing
        && !input.graph.contains_key(&base)
    {
        return Action::RemoteBehind;
    }
    let heads = heads(input.graph);
    match heads.as_slice() {
        [] => match (input.base, input.local) {
            (_, Local::Missing) => Action::Nothing,
            (None, _) => Action::Upload { parents: vec![] },
            (Some(_), Local::Clean) => Action::RemoteBehind,
            (Some(base), Local::Dirty) => Action::Upload {
                parents: vec![base],
            },
        },
        [head] => one_head(input, *head),
        _ => match input.local {
            Local::Dirty => Action::UploadThenConflict {
                parents: input.base.into_iter().collect(),
            },
            _ => Action::Conflict(heads),
        },
    }
}

fn one_head(input: &Input<'_>, head: VersionId) -> Action {
    let Some(base) = input.base else {
        return match input.local {
            Local::Missing => Action::FastForward(head),
            _ if input.local_matches == Some(head) => Action::Adopt(head),
            _ if input.replace_local => Action::FastForward(head),
            _ => Action::UploadThenConflict { parents: vec![] },
        };
    };
    if input.local == Local::Missing {
        return Action::LocalMissing;
    }
    if !descends_from(input.graph, head, base) {
        return match input.local {
            Local::Dirty => Action::UploadThenConflict {
                parents: vec![base],
            },
            _ => Action::RemoteBehind,
        };
    }
    match (head == base, input.local) {
        (true, Local::Dirty) => Action::Upload {
            parents: vec![base],
        },
        (false, Local::Clean) => Action::FastForward(head),
        (false, Local::Dirty) => Action::UploadThenConflict {
            parents: vec![base],
        },
        _ => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heads::Graph;

    fn v(n: u64) -> VersionId {
        VersionId::from_parts(n, [0; 10])
    }

    fn run(
        base: Option<u64>,
        local: Local,
        graph: &Graph,
        matches: Option<u64>,
        replace: bool,
    ) -> Action {
        reconcile(&Input {
            base: base.map(v),
            local,
            graph,
            local_matches: matches.map(v),
            replace_local: replace,
        })
    }

    #[test]
    fn the_spec_table() {
        let one: Graph = [(v(1), vec![])].into();
        let two: Graph = [(v(1), vec![]), (v(2), vec![v(1)])].into();
        assert_eq!(
            run(Some(1), Local::Clean, &one, None, false),
            Action::Nothing
        );
        assert_eq!(
            run(Some(1), Local::Clean, &two, None, false),
            Action::FastForward(v(2))
        );
        assert_eq!(
            run(Some(1), Local::Dirty, &one, None, false),
            Action::Upload {
                parents: vec![v(1)]
            }
        );
        assert_eq!(
            run(Some(1), Local::Dirty, &two, None, false),
            Action::UploadThenConflict {
                parents: vec![v(1)]
            }
        );
        let fork: Graph = [(v(1), vec![]), (v(2), vec![v(1)]), (v(3), vec![v(1)])].into();
        assert_eq!(
            run(Some(1), Local::Clean, &fork, None, false),
            Action::Conflict(vec![v(2), v(3)])
        );
        assert_eq!(
            run(Some(1), Local::Dirty, &fork, None, false),
            Action::UploadThenConflict {
                parents: vec![v(1)]
            }
        );
    }

    #[test]
    fn first_sync_without_a_base() {
        let empty = Graph::new();
        let one: Graph = [(v(1), vec![])].into();
        assert_eq!(
            run(None, Local::Dirty, &empty, None, false),
            Action::Upload { parents: vec![] }
        );
        assert_eq!(
            run(None, Local::Missing, &empty, None, false),
            Action::Nothing
        );
        assert_eq!(
            run(None, Local::Missing, &one, None, false),
            Action::FastForward(v(1))
        );
        assert_eq!(
            run(None, Local::Dirty, &one, Some(1), false),
            Action::Adopt(v(1))
        );
        assert_eq!(
            run(None, Local::Dirty, &one, None, false),
            Action::UploadThenConflict { parents: vec![] },
            "different local files never get overwritten silently"
        );
        assert_eq!(
            run(None, Local::Dirty, &one, None, true),
            Action::FastForward(v(1))
        );
    }

    #[test]
    fn local_folder_gone_and_remote_behind() {
        let one: Graph = [(v(1), vec![])].into();
        let empty = Graph::new();
        assert_eq!(
            run(Some(1), Local::Missing, &one, None, false),
            Action::LocalMissing
        );
        assert_eq!(
            run(Some(7), Local::Clean, &one, None, false),
            Action::RemoteBehind
        );
        assert_eq!(
            run(Some(1), Local::Clean, &empty, None, false),
            Action::RemoteBehind
        );
        assert_eq!(
            run(Some(7), Local::Dirty, &one, None, false),
            Action::RemoteBehind,
            "a base the remote no longer has is a rollback, even with local changes"
        );
        assert_eq!(
            run(Some(1), Local::Dirty, &empty, None, false),
            Action::RemoteBehind
        );
        let fork: Graph = [(v(2), vec![]), (v(3), vec![])].into();
        assert_eq!(
            run(Some(1), Local::Dirty, &fork, None, false),
            Action::RemoteBehind
        );
    }
}
