// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The typed client: one request connection owned by this process, and the verbs
//! codex-alab speaks on it (CONTRACT §2, §3, §5).

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::Endpoint;
use crate::Error;
use crate::conn::Conn;
use crate::conn::Framing;
use crate::conn::Reply;
use crate::events::Events;
use crate::policy::HarnessPolicy;
use crate::policy::PolicyAnswer;
use crate::status::Status;
use crate::status::Whoami;
use crate::wire;

/// Every call's deadline (CONTRACT §5): past it the call is unanswered, and passive.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a claim may wait for its answer. aterm holds the `OK` of a claim that replaces
/// its own host's until the writes that host's fence already admitted have landed, bounded
/// by those writes' deadlines: at worst its 30 s main-thread reply limit. A shorter wait
/// would read a slow `OK` as unanswered and drop a claim aterm had granted.
pub const CLAIM_TIMEOUT: Duration = Duration::from_secs(40);

/// The keyed attention owner codex-alab writes (`meta set attention owner=codex-alab`).
pub const ATTENTION_OWNER: &str = "codex-alab";

/// aterm's cap on a keyed attention text, in bytes.
const ATTENTION_MAX_BYTES: usize = 200;

/// A cap on a notice text sent to `appnotice` (aterm sanitizes and bounds it again).
const NOTICE_MAX_BYTES: usize = 1024;

/// aterm's cap on a session's `description`, in bytes.
pub const DESCRIPTION_MAX_BYTES: usize = 1024;

/// The verbs aterm's `story` takes: a CLOSED set, anything else is `ERR usage`.
pub const STORY_VERBS: [&str; 8] = [
    "approved",
    "dismissed",
    "reconnected",
    "timeout",
    "exit",
    "compacted",
    "warned",
    "chose",
];

/// aterm's cap on a story point's text, in bytes.
pub const STORY_MAX_BYTES: usize = 96;

/// The supervisor holder string for `pid`: `codex-alab@<pid>`.
pub fn holder_for(pid: u32) -> String {
    format!("codex-alab@{pid}")
}

/// Client options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Every call's deadline.
    pub timeout: Duration,
    /// The pid the holder names. Always this process's in production: aterm proves a
    /// claim by the connection's kernel peer pid, so naming another proves nothing.
    pub holder_pid: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            timeout: CALL_TIMEOUT,
            holder_pid: std::process::id(),
        }
    }
}

/// What `claim` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The claim is ours and aterm says it is proven (`changed` = newly shown).
    Held {
        /// Whether the shown holder moved.
        changed: bool,
    },
    /// Not attempted: no `harness policy` answered `OK effective=on` on this connection.
    NotPermitted(String),
    /// Another holder's live claim (`ERR busy supervisor=<holder>`).
    Busy {
        /// That holder, decoded.
        holder: String,
    },
    /// aterm took the claim but does not say it is proven; it was given back at once.
    Unproven,
    /// Any other refusal, as sent.
    Refused(String),
}

/// What `harness_act` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActOutcome {
    /// Recorded on the session.
    Recorded,
    /// This aterm has no `harness_act` field.
    Unsupported(String),
    /// Refused (`ERR denied`: not the proven holder, or another refusal).
    Refused(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

#[derive(Debug, Default)]
struct State {
    policy: Option<HarnessPolicy>,
    claimed: bool,
    /// Same-holder sets ([`Client::reconfirm`]) sent since the claim was last taken.
    reconfirms: u64,
}

/// One aterm session's client. Cheap to clone; every clone shares the one connection.
/// When the last clone drops, the connection closes and aterm releases its claim.
#[derive(Clone)]
pub struct Client {
    endpoint: Endpoint,
    conn: Conn,
    options: Options,
    holder: String,
    version: String,
    server_pid: Option<u32>,
    state: Arc<Mutex<State>>,
}

impl Client {
    /// Connect to the session hosting this terminal ([`Endpoint::from_env`]); `Ok(None)`
    /// outside codex-alab or outside aterm, where codex-alab does nothing.
    pub async fn connect_self() -> Result<Option<Client>, Error> {
        match Endpoint::from_env() {
            Some(endpoint) => Client::connect(endpoint, Options::default())
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    /// Connect and authenticate to `endpoint`, within `options.timeout`.
    pub async fn connect(endpoint: Endpoint, options: Options) -> Result<Client, Error> {
        let opened = crate::conn::open(&endpoint.sock, options.timeout).await?;
        let version = opened.version.clone();
        let server_pid = opened.server_pid;
        let conn = Conn::spawn(opened, options.timeout);
        Ok(Client {
            holder: holder_for(options.holder_pid),
            endpoint,
            conn,
            options,
            version,
            server_pid,
            state: Arc::new(Mutex::new(State::default())),
        })
    }

    /// The endpoint this client speaks to.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The `version` answer the connect phase read.
    pub fn server_version(&self) -> &str {
        &self.version
    }

    /// The pid of the process serving this connection's socket, as the kernel names its
    /// peer at the connect (`None` when it cannot be told): aterm's own process.
    pub fn server_pid(&self) -> Option<u32> {
        self.server_pid
    }

    /// This client's holder string.
    pub fn holder(&self) -> &str {
        &self.holder
    }

    /// Whether the connection still serves requests (false after a timeout or close).
    pub fn is_open(&self) -> bool {
        self.conn.is_open()
    }

    /// Resolves once the request connection stopped serving (an unanswered call closed it,
    /// or aterm went away): the claim bound to it is gone, and a new connection is needed.
    pub async fn closed(&self) {
        self.conn.ended().await;
    }

    /// Whether this client believes it holds the claim (its last claim or reconfirm
    /// answered `OK`, and nothing released it since).
    pub fn holds_claim(&self) -> bool {
        self.lock().claimed && self.is_open()
    }

    /// Close the connection now: aterm then releases a claim bound to it.
    pub fn close(&self) {
        self.lock().claimed = false;
        self.conn.close();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// One request, Status-framed, as written (no `@self` expansion).
    pub async fn request(&self, line: &str) -> Result<Reply, Error> {
        self.conn.request(line, Framing::Status).await
    }

    /// One request whose reply is Lines-framed (`OK <n>` then n lines).
    pub async fn request_lines(&self, line: &str) -> Result<Reply, Error> {
        self.conn.request(line, Framing::Lines).await
    }

    /// One request whose reply is Bytes-framed (`OK <n> [fields…]` then n bytes).
    pub async fn request_bytes(&self, line: &str) -> Result<Reply, Error> {
        self.conn.request(line, Framing::Bytes).await
    }

    /// One Status-framed request whose line announces `body.len()` bytes that follow it
    /// (`post … len=<n>`).
    pub async fn request_with_body(&self, line: &str, body: Vec<u8>) -> Result<Reply, Error> {
        self.conn
            .request_with_body(line, body, Framing::Status)
            .await
    }

    /// A request addressed to this session (`@self` expanded to `@<sid>`).
    async fn on_self(&self, rest: &str) -> Result<Reply, Error> {
        let line = format!("@{} {rest}", self.endpoint.sid);
        self.request(&line).await
    }

    /// `whoami`.
    pub async fn whoami(&self) -> Result<Whoami, Error> {
        let reply = self.request("whoami").await?;
        Whoami::parse(ok_text(&reply)?)
    }

    /// `@self status`.
    pub async fn status(&self) -> Result<Status, Error> {
        let reply = self.on_self("status").await?;
        Status::parse(ok_text(&reply)?)
    }

    /// `@self meta`, raw fields (pct-encoded; read with [`wire::field`]).
    pub async fn meta(&self) -> Result<std::collections::BTreeMap<String, String>, Error> {
        let reply = self.on_self("meta").await?;
        Ok(wire::parse_fields(ok_text(&reply)?))
    }

    /// `@self harness policy` (CONTRACT §3). An `ERR` or an `OK` this reader cannot take
    /// is [`PolicyAnswer::Unsupported`]; a timeout or a closed connection is an error. The
    /// answer is remembered: [`Client::claim`] needs a supported, effective one, and any
    /// other answer forgets the last.
    pub async fn harness_policy(&self) -> Result<PolicyAnswer, Error> {
        let reply = match self.on_self("harness policy").await {
            Ok(reply) => reply,
            Err(e) => {
                self.lock().policy = None;
                return Err(e);
            }
        };
        let answer = PolicyAnswer::parse(&reply.status);
        self.lock().policy = match &answer {
            PolicyAnswer::Supported(p) => Some(p.clone()),
            PolicyAnswer::Unsupported(_) => None,
        };
        Ok(answer)
    }

    /// The last policy `harness_policy` read, if it was supported.
    pub fn policy(&self) -> Option<HarnessPolicy> {
        self.lock().policy.clone()
    }

    /// Forget the remembered policy (a `harness-policy` event moved it): no claim until it
    /// is read again.
    pub fn forget_policy(&self) {
        self.lock().policy = None;
    }

    /// Claim the session (CONTRACT §2): `@self meta set supervisor codex-alab@<pid>`, no
    /// `ttl=`, so the claim lasts while this connection serves requests. Sent only after
    /// `harness policy` answered `OK effective=on` on this connection; then `status` must
    /// show the claim ours AND proven, else it is given back at once.
    pub async fn claim(&self) -> Result<ClaimOutcome, Error> {
        match self.lock().policy.as_ref() {
            None => {
                return Ok(ClaimOutcome::NotPermitted(
                    "no supported harness policy".into(),
                ));
            }
            Some(p) if !p.effective => {
                return Ok(ClaimOutcome::NotPermitted(format!(
                    "effective=off why={}",
                    p.why
                )));
            }
            Some(_) => {}
        }
        let line = format!("@{} meta set supervisor {}", self.endpoint.sid, self.holder);
        let reply = self
            .conn
            .request_within(&line, Framing::Status, CLAIM_TIMEOUT)
            .await?;
        if reply.ok().is_none() {
            return Ok(refused_claim(reply.status));
        }
        let shown = self.status().await;
        match shown {
            Ok(status) if status.proven_holder(&self.holder) => {
                let mut state = self.lock();
                let changed = !state.claimed;
                state.claimed = true;
                state.reconfirms = 0;
                Ok(ClaimOutcome::Held { changed })
            }
            Ok(_) => {
                self.release().await?;
                Ok(ClaimOutcome::Unproven)
            }
            Err(e) => {
                // Cannot see it proven: give it back (a closed connection already did).
                if self.is_open() {
                    let _ = self.release().await;
                }
                Err(e)
            }
        }
    }

    /// Re-confirm the claim in the tick of an act (CONTRACT §2): the same-holder `meta
    /// set` answered `OK`. Any other answer marks the claim lost. Each one sent is counted
    /// ([`Client::reconfirmed_since_claim`]): a same-holder set takes back a claim someone
    /// cleared just before it.
    pub async fn reconfirm(&self) -> Result<bool, Error> {
        {
            let mut state = self.lock();
            if !(state.claimed && self.is_open()) {
                return Ok(false);
            }
            state.reconfirms = state.reconfirms.saturating_add(1);
        }
        let result = self
            .on_self(&format!("meta set supervisor {}", self.holder))
            .await;
        let ok = matches!(&result, Ok(reply) if reply.ok().is_some());
        if !ok {
            self.lock().claimed = false;
        }
        result.map(|_| ok)
    }

    /// Give the claim back: `@self meta unset supervisor holder=codex-alab@<pid>`, which
    /// never clears another holder's.
    pub async fn release(&self) -> Result<(), Error> {
        self.lock().claimed = false;
        let reply = self
            .on_self(&format!("meta unset supervisor holder={}", self.holder))
            .await?;
        ok_text(&reply).map(|_| ())
    }

    /// Whether a re-confirm was sent since the claim was last taken: then `status` showing the
    /// claim ours does not say a clear met since came before the claim (the re-confirm may
    /// have taken it back).
    pub fn reconfirmed_since_claim(&self) -> bool {
        self.lock().reconfirms > 0
    }

    /// Forget the claim locally (the watch saw it go to someone else); nothing is sent.
    pub fn note_claim_lost(&self) {
        self.lock().claimed = false;
    }

    /// Raise codex-alab's keyed attention: `<kind>: <why>`, one line of at most 200 bytes.
    pub async fn set_attention(&self, kind: &str, why: &str) -> Result<(), Error> {
        let text = wire::single_line(&format!("{kind}: {why}"), ATTENTION_MAX_BYTES);
        if text.is_empty() {
            return Err(Error::BadRequest("empty attention".into()));
        }
        let reply = self
            .on_self(&format!(
                "meta set attention owner={ATTENTION_OWNER} {text}"
            ))
            .await?;
        ok_text(&reply).map(|_| ())
    }

    /// Clear codex-alab's keyed attention (only its own entry).
    pub async fn clear_attention(&self) -> Result<(), Error> {
        let reply = self
            .on_self(&format!("meta unset attention owner={ATTENTION_OWNER}"))
            .await?;
        ok_text(&reply).map(|_| ())
    }

    /// Say what the session is doing: `@self meta set description <text>`, one line of at
    /// most [`DESCRIPTION_MAX_BYTES`] bytes. aterm shows it in `sessions` and a restore
    /// carries it, so a relaunched tab still says what it was doing. Never the title: that is
    /// the person's field, and it outranks the program's own.
    pub async fn set_description(&self, text: &str) -> Result<(), Error> {
        let text = wire::single_line(text, DESCRIPTION_MAX_BYTES);
        if text.is_empty() {
            return Err(Error::BadRequest("empty description".into()));
        }
        let reply = self
            .on_self(&format!("meta set description {text}"))
            .await?;
        ok_text(&reply).map(|_| ())
    }

    /// Withdraw the description: `@self meta unset description`. The caller first reads that
    /// the standing text is the one it wrote (aterm has no compare-and-set).
    pub async fn unset_description(&self) -> Result<(), Error> {
        let reply = self.on_self("meta unset description").await?;
        ok_text(&reply).map(|_| ())
    }

    /// Record an act on the session (CONTRACT §5 visibility): `@self meta set harness_act
    /// <act>`, sent only while the claim is held. `act` is one owner token.
    pub async fn harness_act(&self, act: &str) -> Result<ActOutcome, Error> {
        if !wire::valid_owner_token(act) {
            return Err(Error::BadRequest(act.to_string()));
        }
        if !self.holds_claim() {
            return Ok(ActOutcome::NotHeld);
        }
        let reply = self.on_self(&format!("meta set harness_act {act}")).await?;
        if reply.ok().is_some() {
            return Ok(ActOutcome::Recorded);
        }
        if reply.status.starts_with("ERR unknown meta field")
            || reply.status.starts_with("ERR usage")
        {
            return Ok(ActOutcome::Unsupported(reply.status));
        }
        Ok(ActOutcome::Refused(reply.status))
    }

    /// Tell the presence band what codex-alab did (`@self story <verb> [<text>]`), as aterm's
    /// own `drive watch` tells it a watcher's decisions: the band flashes the point for three
    /// seconds and keeps it as the tab's story dot until the person looks; `chose` also plays
    /// aterm's quiet choice chime. Sent only while the claim is held, on the claim's
    /// connection. `verb` is one of [`STORY_VERBS`]; `text` becomes one line of at most
    /// [`STORY_MAX_BYTES`] bytes, and is never a command.
    pub async fn story(&self, verb: &str, text: &str) -> Result<ActOutcome, Error> {
        if !STORY_VERBS.contains(&verb) {
            return Err(Error::BadRequest(format!("story {verb}")));
        }
        if !self.holds_claim() {
            return Ok(ActOutcome::NotHeld);
        }
        let text = wire::single_line(text, STORY_MAX_BYTES);
        let line = if text.is_empty() {
            format!("story {verb}")
        } else {
            format!("story {verb} {text}")
        };
        let reply = self.on_self(&line).await?;
        if reply.ok().is_some() {
            return Ok(ActOutcome::Recorded);
        }
        if reply.status.starts_with("ERR unknown verb") || reply.status.starts_with("ERR usage") {
            return Ok(ActOutcome::Unsupported(reply.status));
        }
        Ok(ActOutcome::Refused(reply.status))
    }

    /// `appnotice harness <text>`: an app-wide record in aterm's message log, never a row.
    pub async fn appnotice_harness(&self, text: &str) -> Result<(), Error> {
        let text = wire::single_line(text, NOTICE_MAX_BYTES);
        if text.is_empty() {
            return Err(Error::BadRequest("empty notice".into()));
        }
        let reply = self.request(&format!("appnotice harness {text}")).await?;
        ok_text(&reply).map(|_| ())
    }

    /// Open the `events` push stream for this session on a second connection.
    pub async fn subscribe(&self) -> Result<Events, Error> {
        Events::open(&self.endpoint, self.options.timeout).await
    }
}

/// The text after `OK`, or the `ERR` as an error.
fn ok_text(reply: &Reply) -> Result<&str, Error> {
    match reply.ok() {
        Some(rest) => Ok(rest),
        None => Err(crate::conn::refusal(reply.status.clone())),
    }
}

fn refused_claim(status: String) -> ClaimOutcome {
    if let Some(holder) = status.strip_prefix("ERR busy supervisor=") {
        let holder = holder.split_ascii_whitespace().next().unwrap_or("");
        return ClaimOutcome::Busy {
            holder: wire::pct_decode(holder),
        };
    }
    ClaimOutcome::Refused(status)
}
