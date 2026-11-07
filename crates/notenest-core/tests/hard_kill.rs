//! Real process-kill tests: a separate process is killed with SIGKILL (no chance to run
//! any cleanup, like a crash or a power cut) and the vault is reopened afterwards.
//!
//! The "child" tests below only do something when the parent test spawns them (they
//! look for an environment variable); in a normal run they return immediately.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use notenest_core::filesystem::atomic::write_atomic;
use notenest_core::services::{SaveMode, Workspace};

const CHILD_ENV: &str = "NN_HARD_KILL_CHILD";
/// Size of the note text saved over and over by the workspace-level test.
const PAYLOAD: usize = 100_000;

fn spawn_child(test_name: &str, vault: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, vault)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn child test process")
}

/// Reads the child's output until a line starting with `prefix` satisfies `ready`,
/// then returns that line. Returns `None` if the child exits first.
fn wait_for_line(child: &mut Child, prefix: &str, ready: impl Fn(&str) -> bool) -> Option<String> {
    let mut reader = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let t = line.trim();
        if t.starts_with(prefix) && ready(t) {
            return Some(t.to_owned());
        }
    }
}

fn kill_dash_nine(child: &mut Child) {
    child.kill().expect("SIGKILL the child");
    child.wait().expect("reap the child");
}

// ---------------------------------------------------------------- child roles

/// Child: open a vault, save a note, stash unsaved typing, then wait to be killed.
#[tokio::test(flavor = "multi_thread")]
async fn child_holds_unsaved_text() {
    let Ok(vault) = std::env::var(CHILD_ENV) else {
        return;
    };
    let (ws, _) = Workspace::open(Path::new(&vault), true).await.unwrap();
    let note = ws.create_note(None, Some("Crash Test")).await.unwrap();
    let saved = ws
        .save_note(note.id, "saved text", SaveMode::Overwrite)
        .await
        .unwrap();
    ws.stash_draft(
        note.id,
        "typed but not yet saved",
        &saved.record.content_hash,
    )
    .await
    .unwrap();
    // Start on a fresh line: the test runner has just printed "test <name> ... ".
    println!("\nREADY");
    tokio::time::sleep(Duration::from_secs(60)).await; // killed long before this ends
}

/// Child: save the same note over and over, reporting each completed save.
#[tokio::test(flavor = "multi_thread")]
async fn child_saves_in_a_loop() {
    let Ok(vault) = std::env::var(CHILD_ENV) else {
        return;
    };
    let (ws, _) = Workspace::open(Path::new(&vault), true).await.unwrap();
    let note = ws.create_note(None, Some("Hammered")).await.unwrap();
    for i in 1..=100_000u32 {
        let text = format!("version {i} {}", "x".repeat(PAYLOAD));
        ws.save_note(note.id, &text, SaveMode::Overwrite)
            .await
            .unwrap();
        println!("SAVED {i}");
    }
}

/// Size of the file rewritten by the atomic-write test. Large, and the child does nothing
/// else, so almost all of its time is spent inside the write: a kill lands there.
const BIG: usize = 16 * 1024 * 1024;

/// Child: rewrite one big file over and over, every byte of version `i` equal to `i % 251`.
#[tokio::test(flavor = "multi_thread")]
async fn child_rewrites_a_big_file() {
    let Ok(vault) = std::env::var(CHILD_ENV) else {
        return;
    };
    let path = Path::new(&vault).join("big.bin");
    for i in 1..=100_000u32 {
        let version = vec![u8::try_from(i % 251).unwrap(); BIG];
        write_atomic(&path, &version).unwrap();
        println!("SAVED {i}");
    }
}

// ----------------------------------------------------------------- the tests

#[tokio::test(flavor = "multi_thread")]
async fn unsaved_typing_survives_a_hard_kill() {
    if std::env::var(CHILD_ENV).is_ok() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn_child("child_holds_unsaved_text", dir.path());
    assert!(
        wait_for_line(&mut child, "READY", |_| true).is_some(),
        "child never became ready"
    );
    kill_dash_nine(&mut child);

    let (ws, report) = Workspace::open(dir.path(), false).await.unwrap();
    assert!(
        !report.index_rebuilt,
        "the index must survive an unclean exit"
    );
    let notes = ws.list_notes().await.unwrap();
    assert_eq!(notes.len(), 1);
    let on_disk = ws.read_note(notes[0].id).await.unwrap();
    assert!(
        on_disk.content.ends_with("saved text"),
        "{:?}",
        on_disk.content
    );

    let drafts = ws.list_drafts().await.unwrap();
    assert_eq!(drafts.len(), 1, "the unsaved typing must be offered back");
    assert_eq!(drafts[0].content, "typed but not yet saved");
    assert!(!drafts[0].disk_changed && !drafts[0].note_missing);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kill_in_the_middle_of_saving_never_leaves_a_broken_note() {
    if std::env::var(CHILD_ENV).is_ok() {
        return;
    }
    // Several rounds, so the kill lands at different points of a save.
    for round in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_child("child_saves_in_a_loop", dir.path());
        let target = 3 + round * 2;
        let last = wait_for_line(&mut child, "SAVED", |l| {
            l.strip_prefix("SAVED ")
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|n| n >= target)
        })
        .expect("the child kept saving");
        let acknowledged: u32 = last.strip_prefix("SAVED ").unwrap().parse().unwrap();
        kill_dash_nine(&mut child);

        // The file is either the last acknowledged version or the next one, whole.
        let text = std::fs::read_to_string(dir.path().join("Hammered.md"))
            .expect("the note file exists and is readable text");
        let (front, body) = text.split_once("\n---\n").expect("frontmatter is intact");
        assert!(front.starts_with("---\nid: "), "{front:?}");
        let version: u32 = body
            .strip_prefix("version ")
            .and_then(|r| r.split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| {
                panic!(
                    "round {round}: body is not a complete version line: {:?}",
                    &body[..body.len().min(40)]
                )
            });
        assert!(
            version == acknowledged || version == acknowledged + 1,
            "round {round}: file has version {version} but {acknowledged} was the last acknowledged save"
        );
        assert_eq!(
            body.len(),
            format!("version {version} ").len() + PAYLOAD,
            "round {round}: the body was cut short"
        );

        // Reopening cleans up and agrees with the file.
        let (ws, report) = Workspace::open(dir.path(), false).await.unwrap();
        assert!(!report.index_rebuilt);
        let notes = ws.list_notes().await.unwrap();
        assert_eq!(notes.len(), 1);
        let reread = ws.read_note(notes[0].id).await.unwrap();
        assert_eq!(reread.content, text);
    }
}

/// The core promise of atomic writes: whenever the process dies, the file is wholly the
/// previous version or wholly the next one, never a mixture or a short file.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_during_an_atomic_write_leaves_a_whole_file() {
    if std::env::var(CHILD_ENV).is_ok() {
        return;
    }
    // Killing right after a `SAVED` line would always hit the child just after a write
    // finished. A growing delay sweeps the kill across the whole write cycle instead.
    for round in 0..16u32 {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_child("child_rewrites_a_big_file", dir.path());
        let last = wait_for_line(&mut child, "SAVED", |l| {
            l.strip_prefix("SAVED ")
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|n| n >= 3 + round)
        })
        .expect("the child kept writing");
        let acknowledged: u32 = last.strip_prefix("SAVED ").unwrap().parse().unwrap();
        std::thread::sleep(Duration::from_micros(u64::from(round) * 900));
        kill_dash_nine(&mut child);
        // The child may have completed a few more writes during the delay.
        let acknowledged_at_least = acknowledged;

        let bytes = std::fs::read(dir.path().join("big.bin")).expect("the file exists");
        assert_eq!(
            bytes.len(),
            BIG,
            "round {round}: the file is {} bytes, not {BIG} (a torn write)",
            bytes.len()
        );
        let first = bytes[0];
        assert!(
            bytes.iter().all(|&b| b == first),
            "round {round}: the file mixes two versions"
        );
        // Whatever version the file holds, it must not be older than the last one seen.
        let newest_seen = acknowledged_at_least % 251;
        let behind = (newest_seen + 251 - u32::from(first)) % 251;
        assert!(
            behind == 0 || behind > 200,
            "round {round}: the file holds version {first}, older than the acknowledged {acknowledged}"
        );
    }
}
