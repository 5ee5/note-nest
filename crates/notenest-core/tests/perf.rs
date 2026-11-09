//! Performance measurement on a large synthetic vault. Not part of the normal test run:
//!
//! ```sh
//! NN_PERF_NOTES=5000 cargo test -p notenest-core --release --test perf -- --ignored --nocapture
//! ```
//!
//! It prints how long the main operations take and asserts only very generous limits,
//! so it can run anywhere without flaking.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use notenest_core::services::{SaveMode, Workspace};

const TAGS: [&str; 12] = [
    "programming/rust",
    "programming/python",
    "programming/linux",
    "research/security",
    "research/ml",
    "personal/journal",
    "projects/alpha",
    "projects/beta",
    "reading",
    "todo",
    "ideas",
    "health",
];

fn word(i: usize) -> String {
    format!("word{}", i % 400)
}

/// Writes `n` notes spread over folders; each has tags, prose, links and inline tags.
fn build_vault(root: &Path, n: usize) {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let folders = 60usize;
    for f in 0..folders {
        std::fs::create_dir_all(root.join(format!("Folder {f}"))).unwrap();
    }
    for i in 0..n {
        let mut text = String::new();
        let t1 = TAGS[(next() % 12) as usize];
        let t2 = TAGS[(next() % 12) as usize];
        write!(
            text,
            "---\ntags: [{t1}, {t2}]\naliases: [Alias {i}]\n---\n# Note {i}\n\n"
        )
        .unwrap();
        for p in 0..6 {
            for _ in 0..40 {
                write!(text, "{} ", word((next() % 4000) as usize)).unwrap();
            }
            let target = (next() % n as u64) as usize;
            write!(
                text,
                "See [[Note {target}]] and [[Note {}|other]] #{}.\n\n",
                (next() % n as u64),
                TAGS[(next() % 12) as usize]
            )
            .unwrap();
            if p == 2 {
                text.push_str("A broken [[Does Not Exist]] link.\n\n");
            }
        }
        let folder = i % folders;
        std::fs::write(root.join(format!("Folder {folder}/Note {i}.md")), text).unwrap();
    }
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let out = f();
    let d = t.elapsed();
    println!("{label:<44} {:>9.1} ms", d.as_secs_f64() * 1000.0);
    (out, d)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance measurement"]
async fn large_vault_performance() {
    let n: usize = std::env::var("NN_PERF_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let dir = tempfile::tempdir().unwrap();
    println!("\n=== vault of {n} notes ===");
    let (_, build) = timed("build the vault on disk", || build_vault(dir.path(), n));
    let bytes: u64 = walk_size(dir.path());
    println!("{:<44} {:>9.1} MB", "vault size", bytes as f64 / 1e6);
    let _ = build;

    let t = Instant::now();
    let (ws, report) = Workspace::open(dir.path(), false).await.unwrap();
    let first_open = t.elapsed();
    println!(
        "{:<44} {:>9.1} ms  ({} notes)",
        "FIRST open (full index)",
        first_open.as_secs_f64() * 1000.0,
        report.reindex.notes
    );

    ws.database().close().await;
    let t = Instant::now();
    let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
    let second_open = t.elapsed();
    println!(
        "{:<44} {:>9.1} ms",
        "SECOND open (reindex over existing index)",
        second_open.as_secs_f64() * 1000.0
    );

    let notes = ws.list_notes().await.unwrap();
    let mid = notes[notes.len() / 2].id;
    let (_, list) = timed("list notes with tags", || ());
    let _ = list;
    let t = Instant::now();
    let entries = ws.list_entries().await.unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} entries)",
        "list notes with tags",
        t.elapsed().as_secs_f64() * 1000.0,
        entries.len()
    );

    let mut search_total = Duration::ZERO;
    for q in [
        "word17",
        "word17 word99",
        "note 42",
        "\"word1 word2\"",
        "#programming/rust word5",
        "alias 99",
        "doesnotexistanywhere",
    ] {
        let t = Instant::now();
        let hits = ws.search(q, 50).await.unwrap();
        let d = t.elapsed();
        search_total += d;
        println!(
            "{:<44} {:>9.1} ms  ({} hits)",
            format!("search {q:?}"),
            d.as_secs_f64() * 1000.0,
            hits.len()
        );
    }
    let t = Instant::now();
    let tags = ws.list_tags().await.unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} tags)",
        "list tags",
        t.elapsed().as_secs_f64() * 1000.0,
        tags.len()
    );
    let t = Instant::now();
    let links = ws.note_links(mid).await.unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} backlinks)",
        "note links (backlinks+outgoing+related)",
        t.elapsed().as_secs_f64() * 1000.0,
        links.backlinks.len()
    );
    let t = Instant::now();
    let g = ws.graph(None, 0).await.unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} nodes, {} edges)",
        "global graph",
        t.elapsed().as_secs_f64() * 1000.0,
        g.nodes.len(),
        g.edges.len()
    );
    for depth in [1, 2] {
        let t = Instant::now();
        let g = ws.graph(Some(mid), depth).await.unwrap();
        println!(
            "{:<44} {:>9.1} ms  ({} nodes, {} edges)",
            format!("local graph (depth {depth})"),
            t.elapsed().as_secs_f64() * 1000.0,
            g.nodes.len(),
            g.edges.len()
        );
    }
    let t = Instant::now();
    let o = ws.orphans().await.unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} orphans)",
        "orphans",
        t.elapsed().as_secs_f64() * 1000.0,
        o.len()
    );
    let t = Instant::now();
    let b = ws.broken_links().await.unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} broken)",
        "broken links",
        t.elapsed().as_secs_f64() * 1000.0,
        b.len()
    );
    let t = Instant::now();
    let r = ws
        .render_markdown(Some(mid), ws.read_note(mid).await.unwrap().content)
        .await
        .unwrap();
    println!(
        "{:<44} {:>9.1} ms  ({} bytes html)",
        "read + render one note",
        t.elapsed().as_secs_f64() * 1000.0,
        r.html.len()
    );

    // A save updates the index for one note: this is what autosave costs.
    let doc = ws.read_note(mid).await.unwrap();
    let t = Instant::now();
    for i in 0..20 {
        ws.save_note(
            mid,
            &format!("{} edit {i}", doc.content),
            SaveMode::Overwrite,
        )
        .await
        .unwrap();
    }
    let per_save = t.elapsed() / 20;
    println!(
        "{:<44} {:>9.1} ms",
        "autosave of one note (avg of 20)",
        per_save.as_secs_f64() * 1000.0
    );

    // Very generous limits: this test exists to print numbers, not to gate CI.
    assert!(
        first_open < Duration::from_secs(120),
        "first open took {first_open:?}"
    );
    assert!(
        second_open < Duration::from_secs(120),
        "second open took {second_open:?}"
    );
    assert!(
        search_total / 7 < Duration::from_secs(2),
        "searches are slow: {search_total:?}"
    );
    assert!(
        per_save < Duration::from_secs(2),
        "autosave is slow: {per_save:?}"
    );
}

fn walk_size(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    let path = e.path();
                    if path.is_dir() {
                        walk_size(&path)
                    } else {
                        e.metadata().map_or(0, |m| m.len())
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}
