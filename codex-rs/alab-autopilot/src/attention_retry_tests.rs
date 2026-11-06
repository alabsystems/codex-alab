// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::time::Duration;

use codex_alab_aterm::Client;
use codex_alab_aterm::Options;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_core::session::Shared;

use super::SessionEnd;
use super::Supervisor;
use super::SupervisorOptions;

struct Harness {
    fake: FakeAterm,
    sup: Arc<Supervisor>,
    session: tokio::task::JoinHandle<SessionEnd>,
    _tui: tokio::sync::watch::Sender<u64>,
    _dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("scratch aterm");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake aterm");
        let shared = Box::leak(Box::new(Shared::new()));
        let endpoint = fake.endpoint();
        let sup = Supervisor::new(SupervisorOptions {
            shared,
            endpoint: Arc::new({
                let endpoint = endpoint.clone();
                move || Some(endpoint.clone())
            }),
            client: Options::default(),
        });
        let (tui, mut changes) = tokio::sync::watch::channel(0);
        let session = {
            let sup = Arc::clone(&sup);
            tokio::spawn(async move { sup.session(endpoint, &mut changes, &mut 0, None).await })
        };
        assert!(fake.wait_for(|_| sup.acting()).await, "proven claim");
        // Serialize with the five claim-time publications before injecting a refusal (the
        // first claim's `harness_held` and `harness_land` unsets are round trips of their own).
        sup.attention("test", None).await;
        sup.show_waits().await;
        sup.show_needs().await;
        sup.show_held().await;
        sup.show_land().await;
        Self {
            fake,
            sup,
            session,
            _tui: tui,
            _dir: dir,
        }
    }

    async fn close(self) {
        self.sup
            .closing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.fake.end_subscriptions();
        tokio::time::timeout(Duration::from_secs(60), self.session)
            .await
            .expect("scratch session ends")
            .expect("scratch session task");
        if let Some(client) = self.sup.client() {
            client.close();
            client.closed().await;
        }
        eprintln!(
            "retained alert retry fixture {}",
            self._dir.keep().display()
        );
    }
}

const NEED: &str = "the deploy token";
const NEED_TEXT: &str = "Codex needs you: the deploy token — carrying on";
const STOPPED: &str = "stopped: the command needs a credential";

/// A non-effective owner can free a real slot without changing the effective
/// attention value. Recovery therefore cannot depend on that metadata event.
#[tokio::test(flavor = "current_thread")]
async fn refused_attention_recovers_without_a_new_report_claim_or_effective_change() {
    let h = Harness::new().await;
    let sid = h.fake.sid();
    h.fake.set_attention_as("hidden", "an older request");
    h.fake.set_attention_as("front", "a reviewer is looking");
    let needs = format!("@{sid} meta set attention owner=needs kind=ask {NEED_TEXT}");
    let attention = format!("@{sid} meta set attention owner=codex-alab {STOPPED}");
    // The fake has no owner-count limit: refuse the exact production write once,
    // as a real full keyed-owner table does, without applying it.
    for command in [&needs, &attention] {
        h.fake.reply_once(
            command,
            "ERR attention owners full (max 8): unset one first\n",
        );
    }
    h.sup.needs(&[NEED.to_string()], 7).await;
    h.sup.attention("stop", Some(STOPPED.into())).await;
    assert!(!h.fake.attention().contains_key("needs"));
    assert!(!h.fake.attention().contains_key("codex-alab"));
    let claim = h.fake.claim();
    let client = Client::connect(h.fake.endpoint(), Options::default())
        .await
        .expect("another metadata writer");
    client
        .clear_attention_owned("hidden")
        .await
        .expect("slot freed");
    client.close();
    client.closed().await;
    assert_eq!(
        h.fake.attention().get("front").map(String::as_str),
        Some("a reviewer is looking")
    );
    let recovered = h
        .fake
        .wait_for(|fake| {
            let entries = fake.attention();
            entries.get("needs").map(String::as_str) == Some(NEED_TEXT)
                && entries.get("codex-alab").map(String::as_str) == Some(STOPPED)
        })
        .await;
    let same_claim = h.fake.claim() == claim;
    let same_need = h.sup.needs_key() == 7;
    let requests = h.fake.requests();
    h.close().await;
    assert!(
        recovered,
        "unchanged refused publications stayed absent: {requests:?}"
    );
    assert!(
        same_claim && same_need,
        "retry must not require a claim or needs change"
    );
    for command in [&needs, &attention] {
        assert_eq!(requests.iter().filter(|line| *line == command).count(), 2);
    }
    assert_no_input(&requests);
}

fn assert_no_input(requests: &[String]) {
    assert!(
        !requests.iter().any(|line| {
            matches!(
                line.split_ascii_whitespace().nth(1),
                Some("turn" | "send" | "key")
            )
        }),
        "metadata retries must not type: {requests:?}"
    );
}

fn attention_requests(requests: &[String]) -> Vec<&str> {
    requests
        .iter()
        .filter(|line| {
            let mut words = line.split_ascii_whitespace();
            words
                .next()
                .is_some_and(|selector| selector.starts_with('@'))
                && words.next() == Some("meta")
                && matches!(words.next(), Some("set" | "unset"))
                && words.next() == Some("attention")
                && matches!(words.next(), Some("owner=needs" | "owner=codex-alab"))
        })
        .map(String::as_str)
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn repeat_refusal_arms_a_fresh_future_deadline() {
    let h = Harness::new().await;
    let command = format!(
        "@{} meta set attention owner=needs kind=ask {NEED_TEXT}",
        h.fake.sid()
    );
    h.fake.reply_once(
        &command,
        "ERR attention owners full (max 8): unset one first\n",
    );
    h.sup.needs(&[NEED.to_string()], 7).await;
    let first = h
        .sup
        .lock()
        .needs_retry
        .expect("refused publication remains owed");
    h.fake.reply_once(
        &command,
        "ERR attention owners full (max 8): unset one first\n",
    );
    h.sup.show_needs().await;
    let second = h
        .sup
        .lock()
        .needs_retry
        .expect("second refusal remains owed");
    assert!(
        second > first,
        "each refusal schedules from its own response"
    );
    let requests = h.fake.requests();
    assert_eq!(requests.iter().filter(|line| *line == &command).count(), 2);
    // Consuming a queued wake only recalculates the clock. A future deadline
    // cannot itself authorize a retry, even when the session was already awake.
    tokio::time::pause();
    h.sup.attention_wake.notify_one();
    let remaining = tokio::time::Instant::from_std(second)
        .saturating_duration_since(tokio::time::Instant::now());
    tokio::time::advance(remaining.saturating_sub(Duration::from_nanos(1))).await;
    let after = h.fake.requests();
    assert_eq!(attention_requests(&after), attention_requests(&requests));
    assert_eq!(h.sup.lock().needs_retry, Some(second));
    tokio::time::resume();
    h.close().await;
    assert_no_input(&after);
}

/// Off does its existing immediate cleanup, but a refused clear grants the new
/// timer no unheld authority. A fresh report parks until a new proven claim.
#[tokio::test(flavor = "current_thread")]
async fn refused_off_cleanup_parks_until_a_fresh_claim_reconciles() {
    let h = Harness::new().await;
    h.sup.needs(&[NEED.to_string()], 7).await;
    let command = format!("@{} meta unset attention owner=needs", h.fake.sid());
    h.fake.reply_once(&command, "ERR denied\n");
    let client = h.sup.client().expect("the request client");
    h.sup
        .release_for_off(
            &client,
            "a person's off".into(),
            codex_alab_core::session::HarnessWord::Off,
        )
        .await;
    assert!(!h.sup.held());
    assert!(h.sup.lock().needs_retry.is_some());
    assert!(
        h.sup.attention_retry_at().is_none(),
        "unheld deadline is parked"
    );
    assert!(h.sup.want_needs(&["a new signing key".to_string()], 9));
    let requests = h.fake.requests();
    tokio::time::pause();
    tokio::time::advance(2 * super::ATTENTION_RETRY).await;
    tokio::time::resume();
    let after = h.fake.requests();
    assert_eq!(
        attention_requests(&after),
        attention_requests(&requests),
        "no unheld metadata retry"
    );
    assert_no_input(&after);
    assert_eq!(
        h.fake.attention().get("needs").map(String::as_str),
        Some(NEED_TEXT)
    );
    // The existing policy callback takes a fresh proven claim and reconciles
    // the current report, rather than replaying the old Off clear. It runs on
    // the session's loop at aterm's policy event, as every read of the policy
    // does: read from this task instead, the loop could meet the new claim's own
    // `supervisor` event before the read counted it held, give it back as one
    // taken while passive, and leave aterm nothing to prove (5 runs in 40).
    h.fake.set_policy_key("enabled", "true");
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !(h.sup.acting()
        && h.sup.needs_key() == 9
        && h.sup.lock().needs_retry.is_none()
        && h.fake.attention().get("needs").map(String::as_str)
            == Some("Codex needs you: a new signing key — carrying on"))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(h.sup.acting());
    assert_eq!(h.sup.needs_key(), 9);
    assert_eq!(
        h.fake.attention().get("needs").map(String::as_str),
        Some("Codex needs you: a new signing key — carrying on")
    );
    assert!(h.sup.lock().needs_retry.is_none());
    let requests = h.fake.requests();
    h.close().await;
    assert_no_input(&requests);
}
