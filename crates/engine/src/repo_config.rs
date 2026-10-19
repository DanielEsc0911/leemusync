//! The repo-config stream: load (merging several heads, D30), write, update.

use std::collections::BTreeMap;

use leemusync_core::config::{CONFIG_FORMAT, ConfigVersion, RepoConfig};
use leemusync_core::heads::{Graph, heads};
use leemusync_core::{DeviceId, VersionId};

use crate::repo::Repo;
use crate::{EngineError, State, new_version_id, now_ms};

pub(crate) struct LoadedConfig {
    pub config: RepoConfig,
    /// Current heads. A version written on top of this config lists them all as parents (D30).
    pub heads: Vec<VersionId>,
    /// Player names the merge had to rename (e.g. `"Ana (2)"`), for the CLI to report (D30).
    #[allow(dead_code)] // used by Task 15
    pub renamed: Vec<String>,
    /// Config versions that failed to decrypt or parse (ignored, reported as a warning).
    #[allow(dead_code)] // used by Task 15
    pub invalid: usize,
}

/// One config version: from the state DB cache, else decrypted from the repo and then cached.
/// `None` when the remote object is invalid.
async fn fetch(
    repo: &Repo,
    state: &State,
    id: &VersionId,
) -> Result<Option<ConfigVersion>, EngineError> {
    if let Some(json) = state.cached_config(id)?
        && let Ok(v) = ConfigVersion::from_json(&json, id)
    {
        return Ok(Some(v));
    }
    let json = match repo.get_config(id).await {
        Ok(j) => j,
        Err(EngineError::Data(_)) => return Ok(None),
        Err(e) => return Err(e),
    };
    let Ok(v) = ConfigVersion::from_json(&json, id) else {
        return Ok(None);
    };
    state.cache_config(id, &json)?;
    Ok(Some(v))
}

/// Reads every config version and merges the heads by union (D30).
pub(crate) async fn load(repo: &Repo, state: &State) -> Result<LoadedConfig, EngineError> {
    let mut graph = Graph::new();
    let mut versions = BTreeMap::new();
    let mut invalid = 0;
    for id in repo.list_configs().await? {
        let Some(v) = fetch(repo, state, &id).await? else {
            invalid += 1;
            continue;
        };
        graph.insert(id, v.parents.clone());
        versions.insert(id, v);
    }
    let heads = heads(&graph);
    let head_configs: Vec<&RepoConfig> = heads
        .iter()
        .filter_map(|h| versions.get(h))
        .map(|v| &v.config)
        .collect();
    let (config, renamed) = match head_configs.as_slice() {
        [] => (RepoConfig::default(), Vec::new()),
        [one] => ((*one).clone(), Vec::new()),
        many => RepoConfig::merge(many),
    };
    Ok(LoadedConfig {
        config,
        heads,
        renamed,
        invalid,
    })
}

/// Writes a new config version on top of `parents`.
pub(crate) async fn write(
    repo: &Repo,
    state: &State,
    device: DeviceId,
    parents: Vec<VersionId>,
    config: RepoConfig,
) -> Result<VersionId, EngineError> {
    let id = new_version_id()?;
    let v = ConfigVersion {
        format: CONFIG_FORMAT,
        id,
        parents,
        device,
        created_at: now_ms(),
        config,
    };
    repo.put_config(&v).await?;
    state.cache_config(
        &id,
        &v.to_json()
            .map_err(|_| EngineError::Data(crate::repo::config_path(&id)))?,
    )?;
    Ok(id)
}

/// Loads (merging any heads), applies `f`, and writes the result with every head as a parent.
pub(crate) async fn update(
    repo: &Repo,
    state: &State,
    device: DeviceId,
    f: impl FnOnce(&mut RepoConfig) -> Result<(), EngineError>,
) -> Result<RepoConfig, EngineError> {
    let loaded = load(repo, state).await?;
    let mut config = loaded.config;
    f(&mut config)?;
    write(repo, state, device, loaded.heads, config.clone()).await?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::PlayerId;

    async fn setup(dir: &std::path::Path) -> (Repo, crate::State, leemusync_store::Store) {
        let store = leemusync_store::Store::fs(&dir.join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = crate::repo::Repo::create(store.clone(), "p", fast)
            .await
            .unwrap();
        let state = crate::State::open(&dir.join("data")).unwrap();
        (repo, state, store)
    }

    #[tokio::test]
    async fn concurrent_edits_merge_by_union() {
        let dir = tempfile::tempdir().unwrap();
        let store = leemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = crate::repo::Repo::create(store, "p", fast).await.unwrap();
        let state = crate::State::open(&dir.path().join("data")).unwrap();
        let dev = DeviceId(VersionId::from_parts(1, [1; 10]));
        let mut a = RepoConfig::default();
        a.add_player(PlayerId(VersionId::from_parts(2, [2; 10])), "A")
            .unwrap();
        let mut b = RepoConfig::default();
        b.add_player(PlayerId(VersionId::from_parts(3, [3; 10])), "B")
            .unwrap();
        // Two devices both edit on top of "no config yet": two heads.
        write(&repo, &state, dev, vec![], a).await.unwrap();
        write(&repo, &state, dev, vec![], b).await.unwrap();
        let loaded = load(&repo, &state).await.unwrap();
        assert_eq!(loaded.heads.len(), 2);
        assert_eq!(loaded.config.players.len(), 2);
        let after = update(&repo, &state, dev, |_| Ok(())).await.unwrap();
        assert_eq!(after.players.len(), 2);
        assert_eq!(
            load(&repo, &state).await.unwrap().heads.len(),
            1,
            "the update merged both heads"
        );
    }

    #[tokio::test]
    async fn the_merge_version_lists_every_head_and_reports_renames() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, state, _) = setup(dir.path()).await;
        let dev = DeviceId(VersionId::from_parts(1, [1; 10]));
        let mut a = RepoConfig::default();
        a.add_player(PlayerId(VersionId::from_parts(2, [2; 10])), "Ana")
            .unwrap();
        let mut b = RepoConfig::default();
        b.add_player(PlayerId(VersionId::from_parts(3, [3; 10])), "ana")
            .unwrap();
        let ha = write(&repo, &state, dev, vec![], a).await.unwrap();
        let hb = write(&repo, &state, dev, vec![], b).await.unwrap();
        let loaded = load(&repo, &state).await.unwrap();
        assert_eq!(loaded.renamed, vec!["ana (2)".to_string()]);
        update(&repo, &state, dev, |_| Ok(())).await.unwrap();
        let after = load(&repo, &state).await.unwrap();
        assert_eq!(
            after.renamed,
            Vec::<String>::new(),
            "the merge version stores the new name"
        );
        let json = repo.get_config(&after.heads[0]).await.unwrap();
        let merged = ConfigVersion::from_json(&json, &after.heads[0]).unwrap();
        let mut parents = merged.parents.clone();
        parents.sort_unstable();
        let mut expected = vec![ha, hb];
        expected.sort_unstable();
        assert_eq!(parents, expected);
        let names: Vec<&str> = merged
            .config
            .players
            .values()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["Ana", "ana (2)"]);
    }

    #[tokio::test]
    async fn invalid_config_versions_are_counted_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, state, store) = setup(dir.path()).await;
        let dev = DeviceId(VersionId::from_parts(1, [1; 10]));
        let mut a = RepoConfig::default();
        a.add_player(PlayerId(VersionId::from_parts(2, [2; 10])), "Ana")
            .unwrap();
        let good = write(&repo, &state, dev, vec![], a).await.unwrap();
        let bad = VersionId::from_parts(9, [9; 10]);
        store
            .put(
                &crate::repo::config_path(&bad),
                b"LSE1 not really sealed, just junk bytes here....".to_vec(),
                leemusync_store::PutMode::CreateOnly,
            )
            .await
            .unwrap();
        let loaded = load(&repo, &state).await.unwrap();
        assert_eq!(loaded.invalid, 1);
        assert_eq!(loaded.heads, vec![good]);
        assert_eq!(loaded.config.players.len(), 1);
    }
}
