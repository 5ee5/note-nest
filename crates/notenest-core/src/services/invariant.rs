//! The knowledge index is derived data that is updated incrementally while the app
//! runs and rebuilt from scratch by `reindex`. These tests make sure both always agree.

use uuid::Uuid;

use super::{SaveMode, Workspace};
use crate::Error;

/// Small deterministic generator, so a failing seed can be replayed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

const TITLES: [&str; 6] = ["Alpha", "Beta", "Gamma", "Delta", "Épsilon", "Zeta Note"];
const FOLDERS: [&str; 4] = ["F1", "F2", "F1/Sub", "Projects"];

fn content(rng: &mut Rng) -> String {
    let mut out = String::new();
    match rng.below(4) {
        0 => out.push_str("---\ntags: [t1, Proj/X]\naliases: [Alt Name, AA]\n---\n"),
        1 => out.push_str("---\ntags:\n  - t2\n---\n"),
        _ => {}
    }
    for _ in 0..rng.below(5) {
        let t = TITLES[rng.below(TITLES.len())];
        out.push_str(&match rng.below(9) {
            0 => format!("See [[{t}]] for more.\n"),
            1 => format!("Also [[{}|shown text]].\n", t.to_lowercase()),
            2 => format!(
                "Path link [[{}/{t}#Heading]].\n",
                FOLDERS[rng.below(FOLDERS.len())]
            ),
            3 => format!("Tagged #t{} and #proj/x.\n", rng.below(3)),
            4 => "Alias link [[AA]] and [[alt name]].\n".to_owned(),
            5 => "```\n[[NotALink]] #notatag\n```\n".to_owned(),
            6 => "Broken [[Does Not Exist]].\n".to_owned(),
            7 => format!("# Heading {t}\n\nText with ünïcode and words.\n"),
            _ => "Plain paragraph.\n\n".to_owned(),
        });
    }
    out
}

fn tolerated(e: &Error) -> bool {
    matches!(
        e,
        Error::AlreadyExists(_)
            | Error::NotFound(_)
            | Error::InvalidPath(_)
            | Error::Conflict { .. }
    )
}

async fn step(ws: &Workspace, rng: &mut Rng) {
    let notes = ws.list_notes().await.unwrap();
    let folders = ws.list_folders().await.unwrap();
    let trash = ws.list_trash().unwrap();
    let id = rng.pick(&notes).map(|n| n.id);
    let result: Result<(), Error> = match rng.below(13) {
        0 | 1 => {
            let folder = (rng.below(3) == 0)
                .then(|| rng.pick(&folders).cloned())
                .flatten();
            let title = rng.below(2) == 0;
            let t = TITLES[rng.below(TITLES.len())];
            ws.create_note(folder.as_deref(), title.then_some(t))
                .await
                .map(|_| ())
        }
        2..=4 => match id {
            Some(id) => ws
                .save_note(id, &content(rng), SaveMode::Overwrite)
                .await
                .map(|_| ()),
            None => Ok(()),
        },
        5 => match id {
            Some(id) => ws
                .rename_note(id, TITLES[rng.below(TITLES.len())])
                .await
                .map(|_| ()),
            None => Ok(()),
        },
        6 => match id {
            Some(id) => {
                let folder = (rng.below(4) != 0)
                    .then(|| rng.pick(&folders).cloned())
                    .flatten();
                ws.move_note(id, folder.as_deref()).await.map(|_| ())
            }
            None => Ok(()),
        },
        7 => match id {
            Some(id) => ws.delete_note(id).await.map(|_| ()),
            None => Ok(()),
        },
        8 => match rng.pick(&trash) {
            Some(t) => ws.restore(t.id).await.map(|_| ()),
            None => Ok(()),
        },
        9 => match id {
            Some(id) if rng.below(2) == 0 => ws.set_pinned(id, rng.below(2) == 0).await.map(|_| ()),
            Some(id) => ws.set_archived(id, rng.below(2) == 0).await.map(|_| ()),
            None => Ok(()),
        },
        10 => ws.create_folder(FOLDERS[rng.below(FOLDERS.len())]).await,
        11 => match rng.pick(&folders) {
            Some(f) if rng.below(2) == 0 => ws.rename_folder(f, FOLDERS[rng.below(3)]).await,
            Some(f) => ws.delete_folder(f).await.map(|_| ()),
            None => Ok(()),
        },
        _ => match id {
            Some(id) => ws.duplicate_note(id).await.map(|_| ()),
            None => Ok(()),
        },
    };
    if let Err(e) = result {
        assert!(tolerated(&e), "unexpected error: {e:?}");
    }
}

#[tokio::test]
async fn incremental_updates_always_match_a_fresh_reindex() {
    let (mut links, mut tags, mut keys, mut fts) = (0usize, 0usize, 0usize, 0usize);
    for seed in 1..=25u64 {
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for op in 0..30 {
            step(&ws, &mut rng).await;
            // Check part-way too, so a drift is caught close to its cause.
            if op % 10 == 9 {
                let incremental = ws.database().derived_snapshot().await.unwrap();
                let count = |p: &str| incremental.iter().filter(|l| l.starts_with(p)).count();
                links += count("link|");
                tags += count("tag|");
                keys += count("key|");
                fts += count("fts|");
                let fresh_dir_snapshot = {
                    ws.reindex().await.unwrap();
                    ws.database().derived_snapshot().await.unwrap()
                };
                assert_eq!(
                    incremental, fresh_dir_snapshot,
                    "seed {seed}, after op {op}: incremental index differs from a rebuild"
                );
            }
        }
    }
    // Guard against a vacuous test: the random runs must really exercise every table.
    assert!(
        links > 100 && tags > 80 && keys > 250 && fts > 150,
        "too little coverage: {links} links, {tags} tags, {keys} keys, {fts} fts rows"
    );
}

#[tokio::test]
async fn deleting_notes_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
    let a = ws.create_note(None, Some("A")).await.unwrap();
    ws.save_note(
        a.id,
        "---\ntags: [solo]\n---\n[[B]] #inline",
        SaveMode::Overwrite,
    )
    .await
    .unwrap();
    assert!(!ws
        .database()
        .derived_snapshot()
        .await
        .unwrap()
        .iter()
        .any(|l| l.starts_with("orphan-fts|1")));
    ws.delete_note(a.id).await.unwrap();
    let snapshot = ws.database().derived_snapshot().await.unwrap();
    assert_eq!(snapshot, vec!["orphan-fts|0".to_owned()], "{snapshot:?}");
    let _ = Uuid::nil();
}
