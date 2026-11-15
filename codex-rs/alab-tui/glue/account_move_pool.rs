// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `/accounts use <label>` and `/accounts auto`: this session moves to another Codex account
//! already signed in on this Mac by asking aterm to restart it there (`restart
//! account=<label>` or `restart account-auto`). aterm ends the process and resumes the same
//! conversation on that account once the account proves it serves, else on the one it ran
//! on. Nobody is signed in by this, and only what aterm's schema 2 snapshot offers is asked:
//! an aterm before it, an account it would refuse, or a session it does not supervise is
//! refused here with nothing sent, and so is a session whose record does not stand at its
//! idle point, which aterm reads last before its signal. What it says goes to the `/accounts`
//! pane; only its last word, a fixed sentence that names no account, reaches history, and only
//! where that pane is gone.
#![cfg(unix)]

use super::ChatWidget;
use super::accounts_pool::CHANGES_UNAVAILABLE;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use codex_alab_core::session::Shared;
use codex_alab_tui::accounts_ui::Update;
use codex_alab_tui::accounts_ui::View;
use codex_alab_tui::aterm::Client;
use codex_alab_tui::aterm::Error;
use codex_alab_tui::aterm::account_move::Outcome;
use codex_alab_tui::aterm::account_move::Target;
use codex_alab_tui::aterm::account_move::Why;
use codex_alab_tui::aterm::accounts::Account;
use codex_alab_tui::aterm::accounts::Condition;
use codex_alab_tui::aterm::accounts::Rotation;
use codex_alab_tui::aterm::accounts::Snapshot;
use codex_alab_tui::session::NotIdle;
use codex_alab_tui::session::ProgramState;
use codex_alab_tui::session::idle_point;
use codex_protocol::ThreadId;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use std::time::Instant;

#[cfg(test)]
#[path = "account_move_pool_tests.rs"]
mod tests;

const USAGE: &str = "Use /accounts use <configured label> or /accounts auto.";
const NO_THREAD: &str = "Start a conversation before selecting its session account.";
const BUSY: &str =
    "Wait for this turn to end, or interrupt it, before changing this session's account.";
const BUSY_SINCE: &str = "This session became busy (a turn, an act, queued input or a draft) \
                          before aterm was asked, so nothing was asked. The session is \
                          unchanged.";
const TERMINALS: &str = "Background terminals run in this session, and the restart would end \
                         them. Let them finish, or stop them, before changing this session's \
                         account. The session is unchanged.";
const WAITING: &str = "This session waits for an answer (an approval, a question or a sign-in). \
                       Answer it before changing this session's account. The session is \
                       unchanged.";
const BETWEEN: &str = "This session is working with no turn running (an active goal between its \
                       turns, or its tools still starting), so it is not at rest. Let it settle \
                       before changing this session's account. The session is unchanged.";
const STARTING: &str = "This session is still starting (its first screen, or aterm's \
                        confirmation of its relaunch, has not come yet), so it is not at rest. \
                        Try again in a moment. The session is unchanged.";
const AUTOPILOT: &str = "Turn Autopilot on to change this session's account: aterm restarts \
                         only a session it supervises.";
const UNVERIFIED: &str =
    "The configured account could not be verified. Refresh /accounts before selecting it.";
const ALREADY: &str = "This session already runs on that account.";
const ALREADY_CHOICE: &str = "This session already runs on the pool's choice.";
const INELIGIBLE: &str = "aterm would refuse a move to that account now. Refresh /accounts to \
                          see its state. The session is unchanged.";
const NOT_WRITTEN: &str = "The session record could not be written, so aterm was not asked to \
                           restart the session. The session is unchanged.";
const NO_ANSWER: &str = "aterm did not answer. If the session restarts it resumes this \
                         conversation; otherwise it is unchanged.";
const UNREAD_ANSWER: &str = "aterm's answer could not be read. If the session restarts it \
                             resumes this conversation; otherwise it is unchanged.";
const GONE: &str = "aterm restarted nothing: this process does not lead its tab. The session \
                    is unchanged.";
const NOT_RESTARTED: &str = "aterm refused to restart this session now. The session is unchanged.";

/// What aterm's refusal `why` means for the person, one fixed sentence per word.
fn refusal(why: Why) -> &'static str {
    match why {
        Why::NotTheHostsAgent | Why::NotRelaunchable => {
            "aterm cannot relaunch this session, so it will not restart it. The session is \
             unchanged."
        }
        Why::NotSupervised => {
            "Turn Autopilot on to change this session's account: aterm restarts only a session it \
             supervises. The session is unchanged."
        }
        Why::Halted => {
            "A hold stands on this session, so aterm will not restart it. The session is unchanged."
        }
        Why::DriverLease | Why::DriverTurn => {
            "Another program is driving this session, so aterm will not restart it now. The session \
             is unchanged."
        }
        Why::StartUnread | Why::NoState => {
            "aterm could not read what it needs to restart this session. The session is unchanged."
        }
        Why::ExitOwnerNotReady => {
            "aterm's harness is not ready yet to keep this session's restored task open through a \
             restart, so it restarted nothing. The session is unchanged."
        }
        Why::Unmanaged => {
            "This session does not run on one of the pool's stored logins, so aterm cannot move it. \
             The session is unchanged."
        }
        Why::NotResumable => {
            "No shell under this session would resume it, so aterm will not restart it. Start \
             codex-alab from a shell to move it between accounts. The session is unchanged."
        }
        Why::RecoveryPending => {
            "An account recovery of this session is under way. The session is unchanged."
        }
        Why::Off => "Account rotation is off in aterm's settings. The session is unchanged.",
        Why::UnknownLabel => {
            "aterm's pool has no account with that label. The session is unchanged."
        }
        Why::NoChoice => {
            "The pool has no account a new session would start on. The session is unchanged."
        }
        Why::Current => "This session already runs on that account. The session is unchanged.",
        Why::Same => {
            "That label names the account this session already runs on. The session is unchanged."
        }
        Why::NotEnrolled => "That account is not signed in on this Mac. The session is unchanged.",
        Why::SignedOut => "That account is signed out on this Mac. The session is unchanged.",
        Why::Held => "That account is held for another Mac. The session is unchanged.",
        Why::Limited => "That account is at a usage limit. The session is unchanged.",
        Why::Identity => {
            "That account's login names another account than its row. The session is unchanged."
        }
        Why::Unread => "aterm could not read the account pool in time. The session is unchanged.",
        Why::RestartPending => {
            "A restart of this session is already under way. The session is unchanged."
        }
        // One word for every way aterm found no rest, so the sentence names them all and
        // claims no cause it cannot know.
        Why::Busy => {
            "aterm did not find this session at rest when it came to restart it (a turn, an act, \
             typing or unread input, or a session record it could not read), so it restarted \
             nothing. The session is unchanged."
        }
    }
}

/// Why the snapshot says a move to `row` would be refused, in its own condition's words.
fn ineligible(row: &Account) -> &'static str {
    if !row.enrolled || row.condition == Condition::NotEnrolled {
        refusal(Why::NotEnrolled)
    } else if row.condition == Condition::SignedOut {
        refusal(Why::SignedOut)
    } else if row.held {
        refusal(Why::Held)
    } else if row.condition == Condition::Limited {
        refusal(Why::Limited)
    } else {
        INELIGIBLE
    }
}

/// The move this command asks of `snapshot`, or the sentence that refuses it with nothing
/// sent. Only a schema 2 snapshot offers a move. A capability it does not offer is refused for
/// rotation being off where the snapshot says so (aterm refuses every move then, Autopilot or
/// not), else is Autopilot's to give where Autopilot is not acting (aterm restarts only a
/// session it supervises).
fn decide(
    snapshot: &Snapshot,
    auto: bool,
    label: &str,
    autopilot_acting: bool,
) -> Result<Target, &'static str> {
    let Some(pool) = snapshot.pool.as_ref() else {
        return Err(CHANGES_UNAVAILABLE);
    };
    let capabilities = &snapshot.capabilities;
    let own = snapshot.assignment.account_id.as_ref();
    let off = pool.rotation == Rotation::Off;
    if auto {
        if capabilities.auto {
            return Ok(Target::Pool);
        }
        return Err(if off {
            refusal(Why::Off)
        } else if !autopilot_acting {
            AUTOPILOT
        } else if capabilities.select && pool.choice_id.is_some() && pool.choice_id.as_ref() == own
        {
            ALREADY_CHOICE
        } else {
            CHANGES_UNAVAILABLE
        });
    }
    if !capabilities.select {
        return Err(if off {
            refusal(Why::Off)
        } else if autopilot_acting {
            CHANGES_UNAVAILABLE
        } else {
            AUTOPILOT
        });
    }
    let mut matching = snapshot.accounts.iter().filter(|row| row.label == label);
    let (Some(row), None) = (matching.next(), matching.next()) else {
        return Err(UNVERIFIED);
    };
    if own == Some(&row.id) {
        return Err(ALREADY);
    }
    if !row.eligible {
        return Err(ineligible(row));
    }
    Ok(Target::Row(row.label.clone()))
}

/// What the pane says while the move is asked.
fn pending(snapshot: &Snapshot, target: &Target) -> String {
    let label_of = |id: &String| {
        snapshot
            .accounts
            .iter()
            .find(|row| &row.id == id)
            .map(|row| row.label.as_str())
    };
    let choice = snapshot
        .pool
        .as_ref()
        .and_then(|pool| pool.choice_id.as_ref())
        .and_then(label_of);
    let name = match target {
        Target::Row(label) => label.as_str(),
        Target::Pool => choice.unwrap_or("the pool's choice"),
    };
    let mut text = format!(
        "Restarting this session on {name}… The conversation resumes there if that account \
         still serves; otherwise on the current one."
    );
    if let Target::Row(label) = target
        && snapshot.pool.as_ref().is_some_and(|pool| pool.pinned)
        && choice != Some(label.as_str())
    {
        text.push_str(&format!(
            " The pool is pinned to {}: this session will not rotate at a limit while the pin \
             holds.",
            choice.unwrap_or("another account")
        ));
    }
    text
}

/// What the pane says once aterm answered the move (`pending` is what it said while asking).
fn answered(answer: Result<Outcome, Error>, pending: String) -> String {
    match answer {
        Ok(Outcome::Restarting) => pending,
        Ok(Outcome::Gone) => GONE.into(),
        Ok(Outcome::Refused(why)) => refusal(why).into(),
        Ok(Outcome::Denied | Outcome::Unsupported) => CHANGES_UNAVAILABLE.into(),
        Ok(Outcome::NotRestarted) => NOT_RESTARTED.into(),
        Ok(Outcome::Unknown) => UNREAD_ANSWER.into(),
        Err(Error::BadRequest(_)) => UNVERIFIED.into(),
        Err(_) => NO_ANSWER.into(),
    }
}

/// Why the move may not end the process now, as the sentence that refuses it with nothing
/// sent; `None` where no turn runs on any thread the TUI routes and the session record stands
/// at its idle point ([`idle_point`]: `idle`, with an empty draft). aterm reads that idle point
/// under the act's lock before it stamps and refuses `account-busy` ([`Why::Busy`]) without
/// it, as it does for input left unread or a person's gesture since the ask: asked only at it,
/// a move aterm refuses so met a session that left rest meanwhile. The record leaves rest
/// while a turn runs, shown or not, an act stands or waits for its turn, a box answer is
/// posted, a carry-on is owed, a person's turn is on its way, input is queued or the live
/// upgrade runs; and, with no turn running, while a background terminal keeps the frame
/// `working`, a box keeps it `blocked`, an active goal between its turns or tools still
/// starting keep it `working`, or the session is still starting (no frame yet, or the busy
/// initializer of a relaunch whose launch probe has not answered). Those four have their own
/// sentences, so the person learns them before anything is asked. A turn the TUI only saw
/// ([`Shared::turn_seen_besides`]: a sub-agent's, a side thread's, one under a remote
/// app-server) is no part of the record, so aterm cannot see it, but the restart would end it
/// too.
///
/// `since` is the check made after the command, whose sentence says a turn, an act or a draft
/// began since. At the command itself (`since` false) the draft is not read: the command's
/// own Enter reads as one until the next frame shows the composer empty.
fn unquiet(shared: &Shared, since: bool) -> Option<&'static str> {
    let busy = if since { BUSY_SINCE } else { BUSY };
    if shared.turn_seen_besides(/*displayed*/ None) {
        return Some(busy);
    }
    match idle_point(shared) {
        Ok(()) => None,
        Err(NotIdle::Draft) if !since => None,
        Err(NotIdle::Draft | NotIdle::Unrest(_)) => Some(busy),
        Err(NotIdle::Frame(ProgramState::Blocked(_))) => Some(WAITING),
        Err(NotIdle::Frame(ProgramState::Working))
            if shared.background_terminals().is_some_and(|count| count > 0) =>
        {
            Some(TERMINALS)
        }
        Err(NotIdle::Frame(ProgramState::Working)) => Some(BETWEEN),
        Err(NotIdle::Starting | NotIdle::Frame(_)) => Some(STARTING),
    }
}

/// How long a move waits, before the record's write, for the frame that draws the command's
/// own Enter ([`own_key_drawn`]). Frames come within milliseconds; the bound only keeps a TUI
/// that draws none from holding the move.
const OWN_KEY_FRAME: Duration = Duration::from_millis(500);

/// Waits, at most [`OWN_KEY_FRAME`], until the frame after the command has drawn: the
/// command's own Enter (`keyed`, the person's last input as the command ran) reads as a draft
/// in the record until a frame shows the composer empty ([`Shared::record_draft_empty`]), and
/// the pool's read can answer before that frame comes. It waits only while that key is the
/// last one: a key, paste or click since the command is a draft the move is refused for.
async fn own_key_drawn(shared: &Shared, keyed: Option<u64>) {
    let until = Instant::now() + OWN_KEY_FRAME;
    while idle_point(shared) == Err(NotIdle::Draft)
        && shared.last_human_input_ms() == keyed
        && Instant::now() < until
    {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// An accepted `/accounts use|auto`: whether it asks for the pool's choice, the label it
/// names otherwise, whether Autopilot acted for this session when it was typed, and the
/// person's last input then (the command's own Enter: [`own_key_drawn`]).
struct Ask {
    auto: bool,
    label: String,
    autopilot_acting: bool,
    keyed: Option<u64>,
}

/// Read the pool on `client` for this process (born at `birth`), decide the move and ask it.
/// The pending sentence is posted and the session record written (through `shared`) before
/// the restart is asked, because aterm ends this process on a carried restart and reads the
/// record's idle point to do so; a record that could not be written asks nothing, and
/// neither does a session that is not at that idle point before the write or after it. The
/// pending sentence goes to the pane only; every last word but a carried restart's (the
/// process ends then) is an [`Update::Outcome`], said in history where the pane is gone.
async fn move_session(
    client: Option<Client>,
    birth: Option<String>,
    ask: Ask,
    view: Weak<Mutex<View>>,
    thread_id: Option<ThreadId>,
    tx: AppEventSender,
    shared: &'static Shared,
) {
    let post = |text: Update| {
        tx.send(AppEvent::AlabAccountsReady {
            view: view.clone(),
            thread_id,
            text,
        });
    };
    let say = |text: String| post(Update::Outcome(text));
    let Some(client) = client else {
        say(CHANGES_UNAVAILABLE.into());
        return;
    };
    let snapshot = match birth {
        Some(birth) => client.accounts_snapshot_for_move(&birth).await.ok(),
        None => None,
    };
    let Some(snapshot) = snapshot else {
        say(UNVERIFIED.into());
        return;
    };
    let target = match decide(&snapshot, ask.auto, &ask.label, ask.autopilot_acting) {
        Ok(target) => target,
        Err(text) => {
            say(text.into());
            return;
        }
    };
    let pending = pending(&snapshot, &target);
    post(Update::Observation(pending.clone()));
    // The command was judged at rest when it was typed, but the pool's read (up to 30 s while
    // aterm reads it) gave a turn, an act, a queued message, a background terminal or a draft
    // time to begin. Judged before the record's write, so the write aterm reads is of a session
    // at its idle point: leaving rest is written through, but coming back to it is coalesced
    // (CONTRACT §4), so a record written busy could still say so when aterm reads it. The
    // command's own Enter is no draft once its frame has drawn.
    own_key_drawn(shared, ask.keyed).await;
    if let Some(text) = unquiet(shared, /*since*/ true) {
        say(text.into());
        return;
    }
    let written = tokio::task::spawn_blocking(move || shared.flush_record())
        .await
        .unwrap_or(false);
    if !written {
        say(NOT_WRITTEN.into());
        return;
    }
    // And again last before the ask, for what began during the write: aterm would refuse it
    // as `account-busy`, but this sentence names it.
    if let Some(text) = unquiet(shared, /*since*/ true) {
        say(text.into());
        return;
    }
    match client.account_move(&target).await {
        Ok(Outcome::Restarting) => post(Update::Observation(pending)),
        answer => say(answered(answer, pending)),
    }
}

impl ChatWidget {
    pub(crate) fn alab_accounts_assignment(&mut self, verb: &str, label: &str) {
        self.alab_accounts_move(verb, label, codex_alab_core::session::shared());
    }

    /// [`Self::alab_accounts_assignment`] for the session whose shared state is `shared`.
    fn alab_accounts_move(&mut self, verb: &str, label: &str, shared: &'static Shared) {
        let view = self.alab_accounts_view();
        let thread_id = self.thread_id();
        let label = label.trim().to_owned();
        let auto = verb == "auto";
        let refused = if (!auto && label.is_empty())
            || (auto && !label.is_empty())
            || label.chars().count() > 128
            || label.chars().any(char::is_control)
        {
            Some(USAGE)
        } else if thread_id.is_none() {
            Some(NO_THREAD)
        } else if self.bottom_pane.is_task_running()
            || self.alab_queued_input()
            || self.alab_pending_user_turn()
        {
            // The restart ends the process: a running turn, a queued message or a submitted
            // one would be lost to it or carried on half done. The widget shows one thread;
            // the record's idle point covers the rest.
            Some(BUSY)
        } else {
            unquiet(shared, /*since*/ false)
        };
        if let Some(text) = refused {
            self.alab_accounts_ready(view, thread_id, Update::Outcome(text.into()));
            return;
        }
        let autopilot_acting = shared.autopilot() == codex_alab_core::session::Autopilot::Active;
        let keyed = shared.last_human_input_ms();
        let tx = self.app_event_tx.clone();
        // The accepted command owns the move, and closing the pane does not stop it. The key
        // that closes the pane reaches the program, so aterm may refuse the move for it
        // (`account-busy`); the move's last word is then said in history.
        tokio::spawn(async move {
            let client = Client::connect_accounts_self().await.ok().flatten();
            let birth = codex_alab_tui::identity::current()
                .ok()
                .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity));
            let ask = Ask {
                auto,
                label,
                autopilot_acting,
                keyed,
            };
            move_session(client, birth, ask, view, thread_id, tx, shared).await;
        });
    }
}
