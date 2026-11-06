// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A codex STARTUP SCREEN that only a person answers, inside aterm (CONTRACT §2 (a)): the
//! sign-in ([`crate::login`]), a folder-trust screen codex-alab's own answer left
//! ([`crate::trust`]), a forced move off a retired model, and the hooks review. Each comes
//! before any thread, so before any claim, and nothing else tells aterm it waits: aterm reads
//! no Codex screen, and its startup deadline names none. So codex-alab raises its keyed
//! attention (`meta set attention owner=codex-alab <kind>: <why>`, at most 200 bytes) while
//! the screen waits, on a connection of its own, and clears it when the screen's step is done
//! ([`during_with`]'s `done`) or the screen ends.
//!
//! [`during`] is inert unless this is the codex-alab binary ([`codex_alab_core::active`]), the
//! process runs in an aterm session ([`Endpoint::from_env`]), and no thread was ever
//! registered in this process ([`codex_alab_core::session::Shared::any_thread_registered`]):
//! a screen after one (a task opened from the agents overview, with the primary detached)
//! comes under the autopilot, whose keyed attention this one would replace and then clear
//! (review of d5e2fc5abf, finding 1: the primary thread alone goes back to none). Outside
//! aterm, an aterm that refuses keyed attention or one that does not answer, nothing is raised
//! and the screen runs exactly as upstream's (fail closed). The raise runs beside the screen,
//! never delaying its first frame; the clear at the screen's end waits at most
//! [`CLEAR_TIMEOUT`] past the raise's own bound.
//!
//! The screen's future goes on the heap as the hook calls in ([`during`], [`migration`],
//! [`hooks`], [`crate::login::during`] are plain functions): an `async fn` keeps a future it
//! takes by value in its state and moves it again, so each layer here carried the whole
//! screen, and the App's own futures (a reconnect's resume, which runs the folder-trust and
//! hooks screens) outgrew a test thread's stack (the gate on 47f4fb5119:
//! `reconnect_daemon_command_center_after_socket_replacement_without_a_conversation`).
//!
//! A screen that never reaches its clear (a SIGTERM, a SIGKILL, a panic) leaves the attention
//! standing: the supervisor's next start past the screens begins one cleanup lifetime,
//! reading [`stale`] on fresh metadata. Failed reads/clears and another owner's masking entry
//! keep it pending on an independent deadline, including while unclaimed. A proven claim,
//! actual publication, foreign holder, own non-screen replacement or explicit empty entry
//! retires it; reconnecting never reopens retired authority. [`clear_stale`] remains the
//! standalone one-read helper. Only codex-alab's visible entry matching this screen kind's
//! fixed footer ([`is_screen_text`]) is cleared.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use codex_alab_aterm::ATTENTION_OWNER;
use codex_alab_aterm::Client;
use codex_alab_aterm::Endpoint;
use codex_alab_aterm::Error as AtermError;
use codex_alab_aterm::Options;
use codex_alab_core::active;
use codex_alab_core::session::shared;

/// How long the raise may take to reach aterm (a local exchange answers in milliseconds).
pub const RAISE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the end of the screen waits for the clear.
pub const CLEAR_TIMEOUT: Duration = Duration::from_secs(2);

/// aterm's cap on an attention's text, in bytes.
pub const TEXT_MAX_BYTES: usize = 200;

/// How every startup screen's `why` ends, and so how a standing one is known ([`is_screen_text`]).
pub const WAITS: &str = "screen waits for a person";

/// Each named screen's fixed footer, shared by the writer and stale-entry reader.
const TRUST_DETAIL: &str = " is not trusted yet; codex's folder-trust ";
const MIGRATION_DETAIL: &str = "; codex's model-migration ";
const HOOKS_DETAIL: &str = " review; codex's hooks-review ";

/// The folder-trust screen's kind.
pub const TRUST: &str = "trust";

/// The forced model migration's kind.
pub const MIGRATION: &str = "migration";

/// The hooks review's kind.
pub const HOOKS: &str = "hooks";

/// One startup screen's attention: aterm shows `<kind>: <why>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub kind: &'static str,
    pub why: String,
}

impl Waiting {
    /// The folder-trust screen for `folder`, which codex-alab's own answer left to a person,
    /// and why it did (`trust::judge`'s reason), cut to what the text has room for.
    pub fn trust(folder: &Path, reason: &str) -> Self {
        let folder = tail_of(&folder.display().to_string(), 72);
        Self::with_reason(TRUST, format!("{folder}{TRUST_DETAIL}{WAITS}"), reason)
    }

    /// A forced move off `from`, a model OpenAI no longer offers, to `to`.
    pub fn migration(from: &str, to: &str) -> Self {
        let (from, to) = (head_of(from, 48), head_of(to, 48));
        Self {
            kind: MIGRATION,
            why: format!("{from} is retired and codex moves to {to}{MIGRATION_DETAIL}{WAITS}"),
        }
    }

    /// The hooks review for `count` hooks of `folder`.
    pub fn hooks(count: usize, folder: &Path) -> Self {
        let folder = tail_of(&folder.display().to_string(), 72);
        let (hooks, need) = if count == 1 {
            ("hook", "needs")
        } else {
            ("hooks", "need")
        };
        Self {
            kind: HOOKS,
            why: format!("{count} {hooks} for {folder} {need}{HOOKS_DETAIL}{WAITS}"),
        }
    }

    /// `why`, then ` (<reason>)` where at least a short reason fits within aterm's cap.
    fn with_reason(kind: &'static str, why: String, reason: &str) -> Self {
        let room = TEXT_MAX_BYTES.saturating_sub(kind.len() + ": ".len() + why.len() + " ()".len());
        let reason = reason.trim();
        let why = if room >= 16 && !reason.is_empty() {
            format!("{why} ({})", head_of(reason, room))
        } else {
            why
        };
        Self { kind, why }
    }

    /// The text aterm shows, as the client sends it.
    pub fn text(&self) -> String {
        codex_alab_aterm::wire::single_line(&format!("{}: {}", self.kind, self.why), TEXT_MAX_BYTES)
    }
}

/// `text` within `max` bytes: its tail, led by `…`, where it is longer (a folder's own name
/// and the folders nearest it say most).
fn tail_of(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

/// `text` within `max` bytes: its head, ended by `…`, where it is longer (a reason's start
/// says what it is).
fn head_of(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// Whether `text` is a startup screen's attention as this build writes it: the sign-in's
/// exactly, or a named screen's own fixed footer (and an optional trust reason).
/// The autopilot's own attentions (`login: …` for a sign-in refused at a turn's end, a
/// question box) never end so.
pub fn is_screen_text(text: &str) -> bool {
    if text == crate::login::text() {
        return true;
    }
    [
        (TRUST, TRUST_DETAIL),
        (MIGRATION, MIGRATION_DETAIL),
        (HOOKS, HOOKS_DETAIL),
    ]
    .iter()
    .any(|(kind, detail)| {
        text.strip_prefix(kind)
            .and_then(|rest| rest.strip_prefix(": "))
            .is_some_and(|why| waits_at_end(why, detail, *kind == TRUST))
    })
}

/// A complete fixed footer must end `why`; only trust permits a parenthesized reason.
/// Candidate iteration keeps markers in a folder, model or reason as ordinary data.
fn waits_at_end(why: &str, detail: &str, reason: bool) -> bool {
    let ending = format!("{detail}{WAITS}");
    why.match_indices(&ending).any(|(at, _)| {
        let rest = &why[at + ending.len()..];
        rest.is_empty() || (reason && rest.starts_with(" (") && rest.ends_with(')'))
    })
}

/// Run `screen` with `waiting`'s attention raised while it waits (module doc). Inert unless
/// [`active`], inside aterm, and before any thread.
pub fn during<F: Future>(waiting: Waiting, screen: F) -> impl Future<Output = F::Output> + use<F> {
    during_boxed(Some(waiting), Box::pin(screen))
}

/// [`during`] when `waiting` names an attention; `None`: the screen runs alone.
pub fn during_if<F: Future>(
    waiting: Option<Waiting>,
    screen: F,
) -> impl Future<Output = F::Output> + use<F> {
    during_boxed(waiting, Box::pin(screen))
}

/// [`during`] of a screen already on the heap; `None`: the screen runs alone.
async fn during_boxed<F: Future>(waiting: Option<Waiting>, screen: Pin<Box<F>>) -> F::Output {
    let Some(waiting) = waiting else {
        return screen.await;
    };
    let endpoint = if active() && !shared().any_thread_registered() {
        Endpoint::from_env()
    } else {
        None
    };
    during_with(endpoint, waiting, || false, screen).await
}

/// The model-migration `prompt` (`ALAB(startup-wait)` in `tui/src/app/startup_prompts.rs`):
/// a forced move off `from` (a model OpenAI no longer offers) waits for a person, so it raises
/// the attention ([`during`]); an optional one runs alone (codex-alab skips those anyway:
/// `ALAB(no-upsell)`).
pub fn migration<F: Future>(
    forced: bool,
    from: &str,
    to: &str,
    prompt: F,
) -> impl Future<Output = F::Output> + use<F> {
    during_if(forced.then(|| Waiting::migration(from, to)), prompt)
}

/// The hooks `review` of `count` hooks for `folder` (`ALAB(startup-wait)` in
/// `tui/src/startup_hooks_review.rs`), with the attention raised while it waits ([`during`]).
pub fn hooks<F: Future>(
    count: usize,
    folder: &Path,
    review: F,
) -> impl Future<Output = F::Output> + use<F> {
    during(Waiting::hooks(count, folder), review)
}

/// [`during`] for this `endpoint` (`None`: inert). `done` says the step that waits is done,
/// read at every wake of the screen: the attention goes then, the screen still running.
pub async fn during_with<F: Future>(
    endpoint: Option<Endpoint>,
    waiting: Waiting,
    done: impl Fn() -> bool,
    screen: F,
) -> F::Output {
    let Some(endpoint) = endpoint else {
        return screen.await;
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let mut attention = tokio::spawn(raise_until(endpoint, waiting, done_rx));
    let mut done_tx = Some(done_tx);
    let mut screen = std::pin::pin!(screen);
    let out = std::future::poll_fn(|cx| {
        let polled = screen.as_mut().poll(cx);
        // The screen wakes for every key, frame and answer: a step done shows right after
        // the poll that took it, and the attention goes then, not when the whole screen ends
        // (review of 4cdc96090, finding 6).
        if done_tx.is_some()
            && done()
            && let Some(done) = done_tx.take()
        {
            let _ = done.send(());
        }
        polled
    })
    .await;
    // The screen ended: the attention goes now if it has not (a sender dropped is the word).
    drop(done_tx);
    if tokio::time::timeout(RAISE_TIMEOUT * 2 + CLEAR_TIMEOUT, &mut attention)
        .await
        .is_err()
    {
        attention.abort();
    }
    out
}

/// Raise the attention, then clear it once `done` says so (or its sender is gone).
async fn raise_until(
    endpoint: Endpoint,
    waiting: Waiting,
    done: tokio::sync::oneshot::Receiver<()>,
) {
    // The raise has its own bound: one cut off may have landed, and is cleared behind.
    let raised = tokio::time::timeout(RAISE_TIMEOUT * 2, raise(endpoint.clone(), &waiting))
        .await
        .unwrap_or(Raised::Unknown);
    let _ = done.await;
    let clear = async {
        match raised {
            Raised::On(client) => {
                let _ = client.clear_attention().await;
                client.close();
            }
            // A raise cut off may have landed: clear on a connection of its own. Nothing else
            // of codex-alab's stands yet (the claim, and its attention, come after a thread).
            Raised::Unknown => {
                if let Ok(client) = Client::connect(endpoint, options()).await {
                    let _ = client.clear_attention().await;
                    client.close();
                }
            }
            Raised::No => {}
        }
    };
    let _ = tokio::time::timeout(CLEAR_TIMEOUT, clear).await;
}

/// Whether a `meta` reply's `fields` show a startup screen's attention codex-alab left
/// standing: the entry aterm shows is codex-alab's (`attention_owner=codex-alab`) and reads as
/// one of these screens' texts ([`is_screen_text`]; review of fea034d20, finding 4: a
/// `login: …` prefix also took the autopilot's own sign-in attentions, still true). aterm shows
/// only its newest entry, so one under another owner's newer entry is not seen (and so not
/// cleared: nothing is cleared that cannot be read).
pub fn stale(fields: &BTreeMap<String, String>) -> bool {
    codex_alab_aterm::wire::field(fields, "attention_owner").as_deref() == Some(ATTENTION_OWNER)
        && codex_alab_aterm::wire::field(fields, "attention")
            .as_deref()
            .is_some_and(is_screen_text)
}

/// At a start past the startup screens: clear codex-alab's own startup-screen attention an
/// earlier start left standing ([`stale`]), and nothing else. `Ok(true)` when one was cleared.
pub async fn clear_stale(client: &Client) -> Result<bool, AtermError> {
    if !stale(&client.meta().await?) {
        return Ok(false);
    }
    client.clear_attention().await?;
    Ok(true)
}

/// What the raise did.
enum Raised {
    /// aterm took it, on this connection (the clear goes on it).
    On(Client),
    /// aterm did not take it (unreachable, refused): nothing to clear.
    No,
    /// It was cut off: it may have landed.
    Unknown,
}

fn options() -> Options {
    Options {
        timeout: RAISE_TIMEOUT,
        ..Options::default()
    }
}

/// Raise the attention on a connection of its own.
async fn raise(endpoint: Endpoint, waiting: &Waiting) -> Raised {
    let Ok(client) = Client::connect(endpoint, options()).await else {
        return Raised::No;
    };
    match client.set_attention(waiting.kind, &waiting.why).await {
        Ok(()) => Raised::On(client),
        Err(AtermError::Unanswered) => {
            client.close();
            Raised::Unknown
        }
        Err(_) => {
            client.close();
            Raised::No
        }
    }
}
