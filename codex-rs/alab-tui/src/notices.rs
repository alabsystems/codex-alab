// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Account notices inside aterm (`ALAB(notices)`), after the owner's ruling (2026-10-21): "for
//! the nag, we need to manage those from a central location, not just interrupt users
//! randomly". OpenAI's security-setup notice (`tui/src/security_setup.rs`; the server owns its
//! eligibility, copy and deadlines) is fetched at every launch, reconnect and account update,
//! and upstream shows it as a banner in each tab. Inside aterm it is filed instead as ONE
//! entry of aterm's exceptions digest (`notice later`: the menu-bar item and Settings ▸
//! Messages), however many tabs and launches say it. Its one button, `Open in browser`, opens
//! the server's chatgpt.com page, and the owner's dismissal holds for every tab (a week, then
//! four weeks, then three months).
//!
//! This routes where the notice shows; it never suppresses it. The fetch, its 3 s deadline
//! and its checks stay upstream's, and [`route`] runs after them. It answers `true` (no
//! banner) only when aterm answered that its digest holds the key, `how=quiet` included: the
//! owner's dismissal governs. Not the codex binary ([`codex_alab_core::active`]), outside
//! aterm, an aterm without `notice later`, a refusal, an error, or no answer within
//! [`ROUTE_TIMEOUT`]: `false`, and upstream's banner shows exactly as before. Notices a
//! person needs now (usage limits, the sign-in, hook consent) are never routed here.
//!
//! When the server answers the account's fetch with no notice (its setup is finished, or the
//! server no longer asks it), [`settle`] ends the account's entry if it is still open, so the
//! digest never keeps asking for a setup already done. An entry the owner dismissed is left
//! dismissed (aterm's `done` would forget that), so a notice that comes and goes does not
//! come back as news.
//!
//! The key is [`key`]: a hash of the account and user ids, so no id reaches aterm's
//! `messages.log`. The title, reason and step are fixed words; the server's own title and
//! description go in as detail lines, cut to aterm's caps ([`later`]).

use std::time::Duration;

use codex_alab_aterm::Client;
use codex_alab_aterm::EndOutcome;
use codex_alab_aterm::Endpoint;
use codex_alab_aterm::LATER_DETAIL_LINE_MAX_CHARS;
use codex_alab_aterm::LATER_DETAIL_LINES;
use codex_alab_aterm::Later;
use codex_alab_aterm::LaterOutcome;
use codex_alab_aterm::ObserveLaterOutcome;
pub use codex_alab_aterm::ObservedLater;
use codex_alab_aterm::Options;
use codex_alab_core::active;

/// The most [`route`] waits for aterm, its connect included (a local exchange answers in
/// milliseconds). It runs after the fetch's own 3 s deadline, so the banner it falls back to
/// is at most this much later.
pub const ROUTE_TIMEOUT: Duration = Duration::from_millis(750);

/// The digest key's prefix; [`key`] adds 8 hex digits.
pub const KEY_PREFIX: &str = "codex.security-setup.";

/// The entry's family on aterm's pages.
pub const TAG: &str = "codex";

/// The entry's title: fixed words, never the server's (aterm's title form: at most 6 words).
pub const TITLE: &str = "Set up Codex account security";

/// Why only the owner can settle it.
pub const REASON: &str = "only you can finish your ChatGPT account's security setup";

/// The one step that settles it (the entry's button opens the page).
pub const STEP: &str = "Finish the setup in your browser";

/// The security-setup notice as codex-tui fetched and checked it (`glue/security_setup.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedNotice {
    /// The ChatGPT account id the notice was fetched for.
    pub account: String,
    /// The ChatGPT user id.
    pub user: String,
    /// The server's title.
    pub title: String,
    /// The server's description.
    pub description: String,
    /// The action's page, already held to `https://chatgpt.com` with no user or port.
    pub url: String,
}

/// The digest key for this account and user: [`KEY_PREFIX`] and the 32-bit FNV-1a hash of
/// the account id, a NUL and the user id, in 8 lowercase hex digits. One account's notice is
/// one entry; another account or user is another.
pub fn key(account: &str, user: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for b in account.bytes().chain([0]).chain(user.bytes()) {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{KEY_PREFIX}{hash:08x}")
}

/// The entry filed for `notice`: the fixed words, the page, and the server's title (one line)
/// and description (the lines left) as detail, each cut to aterm's caps at a space, the last
/// one marked `…` where words were left out.
pub fn later(notice: &RoutedNotice) -> Later {
    let mut detail = lines(&notice.title, 1);
    detail.extend(lines(
        &notice.description,
        LATER_DETAIL_LINES - detail.len(),
    ));
    Later {
        key: key(&notice.account, &notice.user),
        tag: Some(TAG.to_string()),
        url: Some(notice.url.clone()),
        title: TITLE.to_string(),
        reason: REASON.to_string(),
        step: STEP.to_string(),
        detail,
    }
}

/// File `notice` in aterm's digest instead of showing its banner (module doc): `true` only
/// when aterm took it.
pub async fn route(notice: &RoutedNotice) -> bool {
    route_from(Endpoint::from_env, notice).await
}

/// [`route`], with the session found by `locate` (asked only for the codex binary).
pub async fn route_from(locate: impl FnOnce() -> Option<Endpoint>, notice: &RoutedNotice) -> bool {
    if !active() {
        return false;
    }
    let Some(endpoint) = locate() else {
        return false;
    };
    let entry = later(notice);
    let outcome = tokio::time::timeout(ROUTE_TIMEOUT, file(endpoint, &entry)).await;
    let taken = matches!(outcome, Ok(Ok(LaterOutcome::Taken { .. })));
    tracing::debug!(key = %entry.key, ?outcome, taken, "security setup notice routed to aterm");
    taken
}

/// The server answered `account`'s fetch with no notice: end its entry in aterm's digest if one
/// is still open (module doc). `true` only when aterm ended one.
pub async fn settle(account: &str, user: &str) -> bool {
    settle_from(Endpoint::from_env, account, user).await
}

/// [`settle`], with the session found by `locate` (asked only for the codex binary).
pub async fn settle_from(
    locate: impl FnOnce() -> Option<Endpoint>,
    account: &str,
    user: &str,
) -> bool {
    if !active() {
        return false;
    }
    let Some(endpoint) = locate() else {
        return false;
    };
    let key = key(account, user);
    let outcome = tokio::time::timeout(ROUTE_TIMEOUT, end(endpoint, &key)).await;
    let ended = matches!(outcome, Ok(Ok(EndOutcome::Ended)));
    tracing::debug!(%key, ?outcome, ended, "security setup notice settled in aterm");
    ended
}

/// Observe the account's open entry before beginning a server fetch. A missing or older
/// native protocol yields no token and cannot authorize a later destructive fallback.
pub async fn observe(account: &str, user: &str) -> Option<ObservedLater> {
    observe_from(Endpoint::from_env, account, user).await
}

/// [`observe`] with an explicit endpoint locator for private protocol fixtures.
pub async fn observe_from(
    locate: impl FnOnce() -> Option<Endpoint>,
    account: &str,
    user: &str,
) -> Option<ObservedLater> {
    if !active() {
        return None;
    }
    let endpoint = locate()?;
    let key = key(account, user);
    let outcome = tokio::time::timeout(ROUTE_TIMEOUT, observe_entry(endpoint, &key)).await;
    match outcome {
        Ok(Ok(ObserveLaterOutcome::Open(observed))) => Some(observed),
        _ => None,
    }
}

/// After accepting the matching server response, settle only the previously observed
/// native entry and opaque token. An entry repeated, restated, replaced or dismissed
/// since that observation keeps its state on native servers with per-entry confirmations.
pub async fn settle_observed(observed: &ObservedLater) -> bool {
    settle_observed_from(Endpoint::from_env, observed).await
}

/// [`settle_observed`] with an explicit endpoint locator for private protocol fixtures.
pub async fn settle_observed_from(
    locate: impl FnOnce() -> Option<Endpoint>,
    observed: &ObservedLater,
) -> bool {
    if !active() {
        return false;
    }
    let Some(endpoint) = locate() else {
        return false;
    };
    matches!(
        tokio::time::timeout(ROUTE_TIMEOUT, end_observed(endpoint, observed)).await,
        Ok(Ok(EndOutcome::Ended))
    )
}

async fn observe_entry(
    endpoint: Endpoint,
    key: &str,
) -> Result<ObserveLaterOutcome, codex_alab_aterm::Error> {
    let client = Client::connect(endpoint, Options::default()).await?;
    let outcome = client.observe_open_later(key, TAG).await;
    client.close();
    outcome
}

async fn end_observed(
    endpoint: Endpoint,
    observed: &ObservedLater,
) -> Result<EndOutcome, codex_alab_aterm::Error> {
    let client = Client::connect(endpoint, Options::default()).await?;
    let outcome = client.settle_observed_later(observed).await;
    client.close();
    outcome
}

/// One `end_open_later` on a connection of its own, closed after it.
async fn end(endpoint: Endpoint, key: &str) -> Result<EndOutcome, codex_alab_aterm::Error> {
    let options = Options {
        timeout: ROUTE_TIMEOUT,
        ..Options::default()
    };
    let client = Client::connect(endpoint, options).await?;
    let outcome = client.end_open_later(key, TAG).await;
    client.close();
    outcome
}

/// One `notice later` on a connection of its own, closed after it.
async fn file(endpoint: Endpoint, later: &Later) -> Result<LaterOutcome, codex_alab_aterm::Error> {
    let options = Options {
        timeout: ROUTE_TIMEOUT,
        ..Options::default()
    };
    let client = Client::connect(endpoint, options).await?;
    let outcome = client.notice_later(later).await;
    client.close();
    outcome
}

/// `text` on at most `most` lines of at most [`LATER_DETAIL_LINE_MAX_CHARS`], cut at the last
/// space that fits (hard where none does); the last line ends `…` when words were left out.
/// A literal `\n`, which aterm reads as a line break, is a space here.
fn lines(text: &str, most: usize) -> Vec<String> {
    let max = LATER_DETAIL_LINE_MAX_CHARS;
    let text = codex_alab_aterm::wire::single_line(&text.replace("\\n", " "), usize::MAX);
    let mut rest = text.as_str();
    let mut out = Vec::new();
    while !rest.is_empty() && out.len() < most {
        // The byte just past the line's `max`th character, if the rest is longer.
        let Some((limit, _)) = rest.char_indices().nth(max) else {
            out.push(rest.to_string());
            break;
        };
        if out.len() + 1 == most {
            let head: String = rest.chars().take(max - 1).collect();
            out.push(format!("{}\u{2026}", head.trim_end()));
            break;
        }
        let cut = if rest[limit..].starts_with(' ') {
            limit
        } else {
            rest[..limit].rfind(' ').unwrap_or(limit)
        };
        out.push(rest[..cut].trim_end().to_string());
        rest = rest[cut..].trim_start();
    }
    out
}
