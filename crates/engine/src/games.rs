//! Installed games: the folders an emulator lists and the games found in them
//! (emulator-profiles §Game discovery). Read-only; everything read here is untrusted data (B2).

use std::path::{Component, Path, PathBuf};

use lemusync_core::profile::{GameDirs, GameScan};

use crate::profiles::read_bounded;

/// Entries looked at per folder; a game folder with more isn't a game folder.
pub const MAX_ENTRIES: usize = 5_000;
/// Characters of a game name kept.
pub const MAX_NAME_CHARS: usize = 200;
/// Game folders read from the emulator's config.
const MAX_DIRS: usize = 256;

/// One game found on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub key: String,
    pub name: Option<String>,
    pub path: PathBuf,
}

/// A game key as `config` accepts it: 1–64 of `[a-z0-9_-]`.
#[must_use]
pub fn valid_key(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

/// Control characters dropped, trimmed, capped. `None` when nothing is left.
#[must_use]
pub fn clean_name(raw: &str) -> Option<String> {
    let clean: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let name: String = clean.trim().chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim_end().to_owned();
    (!name.is_empty()).then_some(name)
}

/// An absolute path without `..` or control characters.
#[must_use]
pub fn usable_dir(path: &Path) -> bool {
    path.is_absolute()
        && !path.to_string_lossy().chars().any(char::is_control)
        && !path.components().any(|c| matches!(c, Component::ParentDir))
}

/// Parses untrusted XML. `roxmltree` rejects a `DOCTYPE` by default, so no entity expansion.
fn parse(text: &str) -> Option<roxmltree::Document<'_>> {
    roxmltree::Document::parse(text).ok()
}

/// The texts of every element at `key` (`root/child/leaf`; the leaf may repeat).
fn texts(doc: &roxmltree::Document<'_>, key: &str) -> Vec<String> {
    let mut parts = key.split('/');
    let root = doc.root_element();
    if parts.next() != Some(root.tag_name().name()) {
        return Vec::new();
    }
    let parts: Vec<&str> = parts.collect();
    let Some((leaf, path)) = parts.split_last() else {
        return root.text().map(str::to_owned).into_iter().collect();
    };
    let mut node = root;
    for part in path {
        let Some(n) = node.children().find(|n| n.has_tag_name(*part)) else {
            return Vec::new();
        };
        node = n;
    }
    node.children()
        .filter(|n| n.has_tag_name(*leaf))
        .map(|n| n.text().unwrap_or("").trim().to_owned())
        .collect()
}

/// A regular file below `dir` at relative `rel`, with no symlink on the way.
fn plain_file(dir: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = dir.to_path_buf();
    for part in rel.split('/') {
        path.push(part);
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
    }
    std::fs::symlink_metadata(&path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|_| path)
}

/// The game folders listed in the emulator's config (`games.dirs`), read only when granted.
pub fn emulator_dirs(
    dirs: &GameDirs,
    config_dir: &Path,
    granted: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let file = config_dir.join(&dirs.file);
    if !granted(&file) {
        return Vec::new();
    }
    let Some(text) = plain_file(config_dir, &dirs.file).and_then(|f| read_bounded(&f)) else {
        return Vec::new();
    };
    let Some(doc) = parse(&text) else {
        return Vec::new();
    };
    texts(&doc, &dirs.key)
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| usable_dir(p))
        .take(MAX_DIRS)
        .collect()
}

/// The game in `dir` itself, if `dir` holds the marker.
fn game_in(scan: &GameScan, dir: &Path) -> Option<Found> {
    let text = read_bounded(&plain_file(dir, &scan.marker)?)?;
    let doc = parse(&text)?;
    let raw = texts(&doc, &scan.key).into_iter().next()?;
    let raw = raw.trim().to_ascii_lowercase();
    let key = match scan.key_last {
        Some(n) if raw.len() >= n => raw.get(raw.len() - n..)?.to_owned(),
        Some(_) => return None,
        None => raw,
    };
    if !valid_key(&key) {
        return None;
    }
    let name = texts(&doc, &scan.name)
        .into_iter()
        .next()
        .and_then(|n| clean_name(&n));
    Some(Found {
        key,
        name,
        path: launch_file(scan, dir).unwrap_or_else(|| dir.to_path_buf()),
    })
}

/// The only `*.<launch_ext>` file in `<dir>/<launch_dir>`, when the profile names one.
fn launch_file(scan: &GameScan, dir: &Path) -> Option<PathBuf> {
    let (sub, ext) = (scan.launch_dir.as_deref()?, scan.launch_ext.as_deref()?);
    let folder = dir.join(sub);
    if !std::fs::symlink_metadata(&folder).ok()?.is_dir() {
        return None;
    }
    let mut hits = std::fs::read_dir(&folder)
        .ok()?
        .take(MAX_ENTRIES)
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        });
    let first = hits.next()?;
    hits.next().is_none().then_some(first)
}

/// Games in `dir` and in its direct subfolders (exactly one level), at most `MAX_ENTRIES`
/// entries looked at. Symlinked subfolders are skipped, so the scan never leaves `dir`.
#[must_use]
pub fn scan_dir(scan: &GameScan, dir: &Path) -> Vec<Found> {
    if !usable_dir(dir) {
        return Vec::new();
    }
    let mut out: Vec<Found> = game_in(scan, dir).into_iter().collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.take(MAX_ENTRIES).flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir())
            && let Some(found) = game_in(scan, &entry.path())
        {
            out.push(found);
        }
    }
    out.sort_by(|a, b| (&a.key, &a.path).cmp(&(&b.key, &b.path)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_rule() -> GameScan {
        GameScan {
            marker: "meta/meta.xml".into(),
            key: "menu/title_id".into(),
            key_last: Some(8),
            name: "menu/longname_en".into(),
            launch_dir: Some("code".into()),
            launch_ext: Some("rpx".into()),
        }
    }

    fn meta(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir.join("meta")).unwrap();
        std::fs::write(dir.join("meta/meta.xml"), body).unwrap();
    }

    fn game(dir: &Path, id: &str, name: &str) {
        meta(
            dir,
            &format!(
                "<?xml version=\"1.0\"?><menu><title_id type=\"hexBinary\" length=\"8\">{id}</title_id><longname_en>{name}</longname_en></menu>"
            ),
        );
    }

    #[test]
    fn dirs_come_from_the_emulator_config_only_when_granted_and_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let games = dir.path().join("games");
        std::fs::write(
            dir.path().join("settings.xml"),
            format!(
                "<content><GamePaths><Entry>{}</Entry><Entry>relative</Entry><Entry>/a/../b</Entry><Entry></Entry></GamePaths></content>",
                games.display()
            ),
        )
        .unwrap();
        let rule = GameDirs {
            file: "settings.xml".into(),
            key: "content/GamePaths/Entry".into(),
        };
        assert_eq!(emulator_dirs(&rule, dir.path(), &|_| true), vec![games]);
        assert_eq!(
            emulator_dirs(&rule, dir.path(), &|_| false),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn a_doctype_or_an_oversized_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let rule = GameDirs {
            file: "settings.xml".into(),
            key: "content/GamePaths/Entry".into(),
        };
        let file = dir.path().join("settings.xml");
        std::fs::write(
            &file,
            "<!DOCTYPE c [<!ENTITY x \"/etc\">]><content><GamePaths><Entry>&x;</Entry></GamePaths></content>",
        )
        .unwrap();
        assert_eq!(
            emulator_dirs(&rule, dir.path(), &|_| true),
            Vec::<PathBuf>::new()
        );
        let pad = " ".repeat(1024 * 1024);
        std::fs::write(
            &file,
            format!("<content><GamePaths><Entry>/g</Entry></GamePaths>{pad}</content>"),
        )
        .unwrap();
        assert_eq!(
            emulator_dirs(&rule, dir.path(), &|_| true),
            Vec::<PathBuf>::new()
        );
        let g = dir.path().join("g");
        meta(
            &g,
            "<!DOCTYPE menu><menu><title_id>0005000010101C00</title_id></menu>",
        );
        assert_eq!(scan_dir(&scan_rule(), dir.path()), Vec::<Found>::new());
    }

    #[test]
    fn scan_finds_unpacked_games_one_level_deep_with_name_and_launch_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("Game A");
        game(&a, "0005000010101C00", "  Game\tA\r\n ");
        std::fs::create_dir_all(a.join("code")).unwrap();
        std::fs::write(a.join("code/game.rpx"), b"").unwrap();
        let b = dir.path().join("B");
        game(&b, "00050000101DDD00", "");
        let deep = dir.path().join("C/inner");
        game(&deep, "00050000101EEE00", "Too deep");
        let found = scan_dir(&scan_rule(), dir.path());
        assert_eq!(
            found,
            vec![
                Found {
                    key: "10101c00".into(),
                    name: Some("Game A".into()),
                    path: a.join("code/game.rpx"),
                },
                Found {
                    key: "101ddd00".into(),
                    name: None,
                    path: b,
                },
            ]
        );
        assert_eq!(scan_dir(&scan_rule(), &a).len(), 1, "the dir itself");
        assert_eq!(
            scan_dir(&scan_rule(), Path::new("relative")),
            Vec::<Found>::new()
        );
    }

    #[test]
    fn names_are_capped_and_bad_keys_dropped() {
        let dir = tempfile::tempdir().unwrap();
        game(
            &dir.path().join("a"),
            "0005000010101C00",
            &"n".repeat(1_000_000),
        );
        game(&dir.path().join("b"), "00050000../../x", "Bad");
        game(&dir.path().join("c"), "1C00", "Short");
        let found = scan_dir(&scan_rule(), dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_ref().unwrap().chars().count(), 200);
    }

    #[test]
    fn scan_stops_after_the_entry_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..=MAX_ENTRIES {
            std::fs::create_dir(dir.path().join(format!("{i:05}"))).unwrap();
        }
        game(&dir.path().join("00000"), "0005000010101C00", "A");
        let n = scan_dir(&scan_rule(), dir.path()).len();
        assert!(n <= 1);
        // Every entry is a game: only MAX_ENTRIES are looked at.
        for i in 0..=MAX_ENTRIES {
            game(
                &dir.path().join(format!("{i:05}")),
                &format!("00050000{i:08x}"),
                "G",
            );
        }
        assert_eq!(scan_dir(&scan_rule(), dir.path()).len(), MAX_ENTRIES);
    }

    #[test]
    fn the_bundled_cemu_rule_reads_the_synthetic_fixture() {
        let profile = crate::profiles::bundled().unwrap().remove(0);
        let rule = profile.games.unwrap().scan.unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles/fixtures/cemu/games");
        let root = root.canonicalize().unwrap();
        let found = scan_dir(&rule, &root.join("games"));
        assert_eq!(
            found,
            vec![Found {
                key: "1fffff00".into(),
                name: Some("Synthetic Game".into()),
                path: root.join("games/Synthetic Game/code/synthetic.rpx"),
            }]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        game(&outside.join("g"), "0005000010101C00", "Out");
        let games = dir.path().join("games");
        std::fs::create_dir_all(&games).unwrap();
        std::os::unix::fs::symlink(outside.join("g"), games.join("link")).unwrap();
        let real = games.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(outside.join("g/meta"), real.join("meta")).unwrap();
        assert_eq!(scan_dir(&scan_rule(), &games), Vec::<Found>::new());
    }
}
