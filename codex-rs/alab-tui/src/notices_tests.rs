// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The account notice's routing (`notices.rs`), against alab-aterm's in-process FAKE aterm
//! (never a real one). The switch is on here, as in the codex binary; `tests/inactive.rs`
//! holds the run without it.

use std::time::Duration;

use codex_alab_aterm::Client;
use codex_alab_aterm::Later;
use codex_alab_aterm::LaterOutcome;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use pretty_assertions::assert_eq;

use crate::notices::KEY_PREFIX;
use crate::notices::REASON;
use crate::notices::RoutedNotice;
use crate::notices::STEP;
use crate::notices::TITLE;
use crate::notices::key;
use crate::notices::later;
use crate::notices::route_from;
use crate::notices::settle_from;

async fn fake(mode: Mode) -> (tempfile::TempDir, FakeAterm) {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .expect("fake aterm");
    (dir, fake)
}

/// The server's notice, as the security-setup fixtures carry it.
fn notice() -> RoutedNotice {
    RoutedNotice {
        account: "acct-0123".to_string(),
        user: "user-abcd".to_string(),
        title: "Set up security for Daybreak mode".to_string(),
        description: "Finish setup by October 15. You can keep using Codex while you finish setup."
            .to_string(),
        url: "https://chatgpt.com/cyber".to_string(),
    }
}

/// A one-minute hang detector around a route, never a latency budget.
async fn routed(fake: &FakeAterm, notice: &RoutedNotice) -> bool {
    let endpoint = fake.endpoint();
    tokio::time::timeout(
        Duration::from_secs(60),
        route_from(move || Some(endpoint), notice),
    )
    .await
    .expect("the route hung")
}

/// A one-minute hang detector around a settle, never a latency budget.
async fn settled(fake: &FakeAterm, notice: &RoutedNotice) -> bool {
    let endpoint = fake.endpoint();
    tokio::time::timeout(
        Duration::from_secs(60),
        settle_from(move || Some(endpoint), &notice.account, &notice.user),
    )
    .await
    .expect("the settle hung")
}

/// Inside aterm the notice is one digest entry, not a banner: the fixed words, the page, the
/// server's words as detail, under a key that names no id. A second tab's route folds into
/// the same entry and is taken too.
#[tokio::test]
async fn inside_aterm_the_notice_is_one_digest_entry() {
    let (_dir, fake) = fake(Mode::Current).await;
    assert!(routed(&fake, &notice()).await);
    let want = "notice later codex.security-setup.060524c7 tag=codex url=https://chatgpt.com/cyber \
                Set up Codex account security -- only you can finish your ChatGPT account's \
                security setup -- Finish the setup in your browser -- Set up security for \
                Daybreak mode\\nFinish setup by October 15. You can keep using Codex while you \
                finish setup.";
    assert_eq!(
        fake.requests()
            .iter()
            .filter(|r| r.starts_with("notice"))
            .collect::<Vec<_>>(),
        [want]
    );
    assert!(routed(&fake, &notice()).await, "a second tab");
    let laters = fake.laters();
    let hows: Vec<&str> = laters.iter().map(|l| l.how.as_str()).collect();
    assert_eq!(hows, ["new", "duplicate"]);
    let entry = &laters[0];
    assert_eq!(entry.title, TITLE);
    assert_eq!(entry.reason, REASON);
    assert_eq!(entry.step, STEP);
    assert_eq!(entry.url.as_deref(), Some("https://chatgpt.com/cyber"));
    assert_eq!(
        entry.detail,
        [
            "Set up security for Daybreak mode",
            "Finish setup by October 15. You can keep using Codex while you finish setup."
        ]
    );
}

/// Once the owner dismissed the entry aterm answers `how=quiet`, and that is still taken: the
/// dismissal in the one central place governs every tab, so no tab shows the banner.
#[tokio::test]
async fn a_quiet_answer_still_counts_as_taken() {
    let (_dir, fake) = fake(Mode::Current).await;
    assert!(routed(&fake, &notice()).await);
    assert!(fake.dismiss_later("codex.security-setup.060524c7"));
    assert!(routed(&fake, &notice()).await);
    assert_eq!(fake.laters().last().map(|l| l.how.as_str()), Some("quiet"));
}

/// NEGATIVE CONTROL: an aterm without `notice later` answers its `notice` usage, and the
/// route says no, so upstream's banner shows.
#[tokio::test]
async fn an_older_aterm_leaves_the_banner() {
    let (_dir, fake) = fake(Mode::Old).await;
    assert!(!routed(&fake, &notice()).await);
    assert!(
        fake.requests()
            .iter()
            .any(|r| r.starts_with("notice later ")),
        "it was asked: {:?}",
        fake.requests()
    );
    assert!(fake.laters().is_empty());
}

/// An aterm that never answers holds the route at most its timeout, then the banner shows.
#[tokio::test]
async fn a_stalled_aterm_times_out_to_the_banner() {
    let (_dir, fake) = fake(Mode::Current).await;
    fake.stall("notice");
    assert!(!routed(&fake, &notice()).await);
    assert!(
        fake.requests()
            .iter()
            .any(|r| r.starts_with("notice later ")),
        "it was asked: {:?}",
        fake.requests()
    );
}

/// A refusal (here the cap on open wire entries, held by other tools) leaves the banner.
#[tokio::test]
async fn a_refusal_leaves_the_banner() {
    let (_dir, fake) = fake(Mode::Current).await;
    let other = Client::connect(fake.endpoint(), codex_alab_aterm::Options::default())
        .await
        .expect("connect");
    for k in ["tool.a", "tool.b", "tool.c", "tool.d"] {
        let entry = Later {
            key: k.to_string(),
            ..later(&notice())
        };
        assert!(matches!(
            other.notice_later(&entry).await.expect("later"),
            LaterOutcome::Taken { .. }
        ));
    }
    assert!(!routed(&fake, &notice()).await);
    assert_eq!(fake.laters().len(), 4);
}

/// Outside aterm (no session found) nothing is routed and the banner shows.
#[tokio::test]
async fn outside_aterm_nothing_is_routed() {
    codex_alab_core::activate();
    assert!(!route_from(|| None, &notice()).await);
}

/// The key is the prefix and the FNV-1a hash of the account and user ids, so the log never
/// holds an id; the ids are hashed apart (a NUL between them), and another account or user
/// is another entry.
#[test]
fn the_key_hashes_the_account_and_user() {
    let k = key("acct-0123", "user-abcd");
    assert_eq!(k, "codex.security-setup.060524c7");
    assert!(k.len() <= codex_alab_aterm::LATER_KEY_MAX);
    assert!(!k.contains("acct") && !k.contains("user"));
    assert_eq!(key("", ""), format!("{KEY_PREFIX}050c5d1f"));
    assert_ne!(key("ab", "c"), key("a", "bc"));
    assert_ne!(key("acct-0123", "user-abcd"), key("acct-0124", "user-abcd"));
    assert_ne!(key("acct-0123", "user-abcd"), key("acct-0123", "user-abce"));
}

/// The server's words fit aterm's detail: its title on one line, its description on the two
/// left, each cut at a space within 240 characters, the last marked `…`; a literal `\n` (which
/// aterm would split on) is a space. The entry's own words never come from the server.
#[test]
fn the_servers_words_are_cut_to_the_detail_caps() {
    let long = RoutedNotice {
        title: format!("Set up\\nsecurity {}", "t".repeat(300)),
        description: "word ".repeat(200),
        ..notice()
    };
    let entry = later(&long);
    assert_eq!(entry.title, TITLE);
    assert_eq!(entry.detail.len(), 3, "{:?}", entry.detail);
    assert!(entry.detail[0].starts_with("Set up security t"));
    assert!(entry.detail.iter().all(|l| l.chars().count() <= 240));
    assert!(entry.detail[0].ends_with('\u{2026}'));
    assert!(entry.detail[1].ends_with("word") && entry.detail[1].len() > 200);
    assert!(entry.detail[2].ends_with('\u{2026}'));
    assert!(entry.line().is_ok(), "the entry reaches aterm whole");
    // Negative control: a short description is whole, on one line, with no mark.
    let short = later(&notice());
    assert_eq!(short.detail.len(), 2);
    assert!(!short.detail[1].ends_with('\u{2026}'));
}

/// The server stops sending the notice (the owner finished the setup): the entry still open
/// in the digest ends, read first and then conditionally settled, so the digest stops asking. A
/// notice the server sends again later is news.
#[tokio::test]
async fn a_notice_the_server_no_longer_sends_ends_its_open_entry() {
    let (_dir, fake) = fake(Mode::Current).await;
    assert!(routed(&fake, &notice()).await);
    assert!(settled(&fake, &notice()).await);
    assert_eq!(
        fake.requests()
            .iter()
            .filter(|r| r.starts_with("messages") || r.starts_with("notice settle"))
            .collect::<Vec<_>>(),
        [
            "messages live need=later tag=codex",
            "notice settle codex.security-setup.060524c7 1 1"
        ]
    );
    assert!(!settled(&fake, &notice()).await, "nothing is open any more");
    assert_eq!(fake.settles().len(), 1, "only the open entry was settled");
    assert!(fake.dones().is_empty());
    assert!(routed(&fake, &notice()).await);
    assert_eq!(fake.laters().last().map(|l| l.how.as_str()), Some("new"));
}

/// NEGATIVE CONTROL for the read first: the owner dismissed the entry, and the server's answer
/// has no notice. No `done` is sent (aterm's would forget the dismissal), so when the notice
/// comes back the owner's dismissal still holds and it stays quiet.
#[tokio::test]
async fn a_dismissed_entry_keeps_its_dismissal_when_the_notice_goes() {
    let (_dir, fake) = fake(Mode::Current).await;
    assert!(routed(&fake, &notice()).await);
    assert!(fake.dismiss_later("codex.security-setup.060524c7"));
    assert!(!settled(&fake, &notice()).await);
    assert!(fake.dones().is_empty(), "{:?}", fake.dones());
    assert!(routed(&fake, &notice()).await);
    assert_eq!(fake.laters().last().map(|l| l.how.as_str()), Some("quiet"));
}

/// Another account's open entry is not this account's to end, and with nothing filed for this
/// one nothing is sent.
#[tokio::test]
async fn only_the_accounts_own_entry_is_ended() {
    let (_dir, fake) = fake(Mode::Current).await;
    let other = RoutedNotice {
        account: "acct-4567".to_string(),
        ..notice()
    };
    assert!(routed(&fake, &other).await);
    assert!(!settled(&fake, &notice()).await);
    assert!(fake.dones().is_empty(), "{:?}", fake.dones());
    assert!(routed(&fake, &other).await);
    assert_eq!(
        fake.laters().last().map(|l| l.how.as_str()),
        Some("duplicate")
    );
}

/// An aterm from before `notice later` has no entry of codex's to end: the read finds none and
/// nothing else is sent.
#[tokio::test]
async fn an_older_aterm_is_sent_no_done() {
    let (_dir, fake) = fake(Mode::Old).await;
    assert!(!settled(&fake, &notice()).await);
    assert!(fake.dones().is_empty());
    assert!(
        !fake.requests().iter().any(|r| r.starts_with("notice")),
        "{:?}",
        fake.requests()
    );
}

/// A pre-fetch observation crosses separate client connections: every later affirmative
/// report stays open, including identical words; a fresh observation can still settle it.
#[tokio::test]
async fn observed_account_notice_settlement_keeps_a_newer_fact() {
    let (dir, fake) = fake(Mode::Current).await;
    let _kept = dir.keep();
    let original = notice();
    assert!(routed(&fake, &original).await);
    let endpoint = fake.endpoint();
    let observed =
        crate::notices::observe_from(|| Some(endpoint.clone()), &original.account, &original.user)
            .await
            .expect("observed entry");
    assert!(routed(&fake, &original).await);
    assert!(!crate::notices::settle_observed_from(|| Some(endpoint.clone()), &observed).await);
    let current =
        crate::notices::observe_from(|| Some(endpoint.clone()), &original.account, &original.user)
            .await
            .expect("the newer identical positive stays open");
    assert!(crate::notices::settle_observed_from(|| Some(endpoint.clone()), &current).await);
    assert!(routed(&fake, &original).await);
    assert_eq!(fake.laters().last().map(|l| l.how.as_str()), Some("new"));
    let observed =
        crate::notices::observe_from(|| Some(endpoint.clone()), &original.account, &original.user)
            .await
            .expect("reopened entry");
    let changed = RoutedNotice {
        description: "The server now asks for another setup step.".to_string(),
        ..original
    };
    assert!(routed(&fake, &changed).await);
    assert!(!crate::notices::settle_observed_from(|| Some(endpoint), &observed).await);
    assert!(routed(&fake, &changed).await);
    assert_eq!(
        fake.laters().last().map(|l| l.how.as_str()),
        Some("duplicate")
    );
    assert!(fake.dones().is_empty());
}
