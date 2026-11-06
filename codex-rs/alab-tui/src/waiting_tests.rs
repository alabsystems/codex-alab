// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The startup screens' attentions (`waiting.rs`): their texts within aterm's cap, how a
//! standing one is known, and a folder-trust raise against alab-aterm's in-process FAKE aterm
//! (never a real one).

use std::path::Path;
use std::time::Duration;

use codex_alab_aterm::Client;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;

use crate::waiting::TEXT_MAX_BYTES;
use crate::waiting::Waiting;
use crate::waiting::clear_stale;
use crate::waiting::during_with;
use crate::waiting::is_screen_text;

/// Every startup screen's text fits aterm's 200-byte cap as built, so the wire cuts nothing
/// from it, and reads as a screen's text, for a folder, a model or a reason far past the cap,
/// in any script.
#[test]
fn every_screen_text_fits_whole_and_reads_as_a_screen() {
    let long = format!("/Users/_owner/{}app", "très-long-dossier/".repeat(20));
    let reason = "aterm's harness policy says effective=off why=hold; ".repeat(8);
    let cases = [
        Waiting::trust(Path::new("/Users/_owner/Developer/app"), "why=hold"),
        Waiting::trust(Path::new(&long), &reason),
        Waiting::trust(Path::new(&long), ""),
        Waiting::migration("gpt-5.1-codex", "gpt-5.5"),
        Waiting::migration(&"m".repeat(300), &"ü".repeat(300)),
        Waiting::hooks(1, Path::new("/Users/_owner/Developer/app")),
        Waiting::hooks(12, Path::new(&long)),
        crate::login::waiting(),
    ];
    for waiting in cases {
        let text = waiting.text();
        assert!(text.len() <= TEXT_MAX_BYTES, "{} bytes: {text}", text.len());
        assert_eq!(
            text,
            format!("{}: {}", waiting.kind, waiting.why),
            "the wire cut nothing"
        );
        assert!(is_screen_text(&text), "{text}");
    }
    let shown = Waiting::trust(Path::new(&long), &reason).text();
    assert!(
        shown.starts_with("trust: …") && shown.contains("/app is not trusted yet"),
        "the folder's own name stays, its head cut: {shown}"
    );
    assert!(
        shown.contains("(aterm's harness policy"),
        "the reason's head stays: {shown}"
    );
}

/// A folder or an explanatory reason may itself contain the marker. Generated screen
/// words still round-trip; an earlier occurrence is data, not the ending marker.
#[test]
fn startup_attention_marker_round_trips_when_inputs_contain_the_marker() {
    let folder = Path::new("/work/screen waits for a person/repo");
    for (waiting, markers) in [
        (Waiting::trust(folder, "why=hold"), 2),
        (
            Waiting::trust(folder, "the other screen waits for a person"),
            3,
        ),
        (
            Waiting::trust(
                Path::new("/work/repo"),
                "the other screen waits for a person",
            ),
            2,
        ),
        (Waiting::hooks(2, folder), 2),
    ] {
        let text = waiting.text();
        assert_eq!(
            text.matches(crate::waiting::WAITS).count(),
            markers,
            "the fixture's markers survive truncation: {text}"
        );
        assert!(is_screen_text(&text), "generated startup text: {text}");
    }
}

/// NEGATIVE CONTROLS: what codex-alab's autopilot raises, and what another writer could, is
/// never read as a startup screen's text, so a start past the screens never clears it.
#[test]
fn only_a_screens_own_text_reads_as_one() {
    let waits = "codex's folder-trust screen waits for a person";
    for other in [
        "trust: a folder codex-alab was told to leave alone",
        &format!("trust: /x is not trusted yet; {waits} and more"),
        &format!("trust: /x is not trusted yet; {waits} (why=hold"),
        &format!("question: /x; {waits}"),
        "migration: gpt-5 is retired; the screen waits for a person",
        "login: Codex asks for a sign-in",
        "login: Codex's sign-in was refused or expired; sign in to Codex again (codex login)",
        "trust: a wall says codex's screen waits for a person",
        "trust: /x is retired; codex's model-migration screen waits for a person",
        "migration: /x is not trusted yet; codex's folder-trust screen waits for a person",
        "hooks: /x is not trusted yet; codex's folder-trust screen waits for a person",
        "migration: old is retired and codex moves to new; codex's model-migration screen waits for a person (why=hold)",
        "hooks: 1 hook for /x needs review; codex's hooks-review screen waits for a person (why=hold)",
    ] {
        assert!(!is_screen_text(other), "{other}");
    }
}

/// Complete footers inside user data do not turn them into delimiters. Migration and
/// hooks have exact endings; the trust reason can contain arbitrary parentheses.
#[test]
fn startup_attention_full_footers_round_trip_as_input_data() {
    let footer = " is not trusted yet; codex's folder-trust screen waits for a person";
    let reason = format!("another{footer}");
    let trust = Waiting::trust(Path::new("/x"), &reason);
    let text = trust.text();
    assert_eq!(
        text.matches(footer).count(),
        2,
        "the complete reason survives: {text}"
    );
    assert!(is_screen_text(&text));
    let folder = format!("/x{footer}");
    let text = Waiting::trust(Path::new(&folder), "").text();
    assert_eq!(
        text.matches(footer).count(),
        2,
        "the complete folder footer survives: {text}"
    );
    assert!(is_screen_text(&text));
    for waiting in [
        Waiting::trust(Path::new("/x/(a)b"), "a reason (with) an unmatched )"),
        Waiting::migration("screen waits for a person", "new"),
        Waiting::hooks(2, Path::new("/x/screen waits for a person")),
    ] {
        assert!(is_screen_text(&waiting.text()), "{waiting:?}");
    }
}

async fn fake() -> (tempfile::TempDir, FakeAterm) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    (dir, fake)
}

/// A one-minute hang detector around a screen run, never a latency budget.
async fn bounded<F: std::future::Future>(run: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("the screen run hung")
}

/// A folder-trust screen codex-alab's answer left to a person raises its attention, naming
/// the folder and the reason, while it waits; when it ends, the attention is gone. One a
/// killed screen left standing is cleared by the next start past the screens.
#[tokio::test]
async fn a_trust_screen_left_to_a_person_raises_its_attention_until_it_ends() {
    let (_dir, fake) = fake().await;
    let waiting = Waiting::trust(
        Path::new("/Users/_owner/Developer/app"),
        "aterm's harness policy says effective=off why=hold",
    );
    let want = waiting.text();
    let seen = bounded(during_with(
        Some(fake.endpoint()),
        waiting,
        || false,
        async {
            fake.wait_for(|f| f.attention().get("codex-alab") == Some(&want))
                .await
        },
    ))
    .await;
    assert!(seen, "{:?}", fake.requests());
    assert!(
        !fake.attention().contains_key("codex-alab"),
        "cleared at the end: {:?}",
        fake.attention()
    );

    fake.set_attention_as("codex-alab", &want);
    let client = Client::connect(fake.endpoint(), codex_alab_aterm::Options::default())
        .await
        .expect("connect");
    assert!(clear_stale(&client).await.expect("read"));
    assert!(fake.attention().is_empty(), "{:?}", fake.attention());
}

/// One hook reads as one: `1 hook … needs review`, `2 hooks … need review`.
#[test]
fn the_hooks_text_agrees_with_its_count() {
    let folder = Path::new("/Users/_owner/Developer/app");
    assert!(
        Waiting::hooks(1, folder)
            .why
            .starts_with("1 hook for /Users/_owner/Developer/app needs review;")
    );
    assert!(
        Waiting::hooks(2, folder)
            .why
            .starts_with("2 hooks for /Users/_owner/Developer/app need review;")
    );
}
