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
use crate::facts;
use crate::facts::FactAnswer;
use crate::facts::ToldOutcome;
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

/// The attention owner a worker's NEEDS YOU lines go under (FOREVER CRUISE / FOREVER LAND,
/// aterm owner direction 2026-10-19: "if you need something from the user, be LOUDER ABOUT
/// IT ... but NEVER let that block you"): aterm's own loop raises a worker's needs under the
/// same owner, `kind=ask`, so the border, pill, menu bar and a manager's `await task` see a
/// codex-alab worker's need as any other. Kept apart from [`ATTENTION_OWNER`]: codex-alab's
/// own attention (a box, a wall) is a point it left to a person; a need never is.
pub const NEEDS_OWNER: &str = "needs";

/// The attention owner codex-alab's automatic waits go under, always `kind=wait` (amended
/// 2026-10-27 after the parity sweep, gap 2: a codex tab waiting out a usage limit or an
/// outage showed nothing). aterm draws a `kind=wait` entry as a Waiting row (the menu bar's ⧗
/// row, the Overview's ⧗ line), never a need, a border or a pill. An owner of its own: aterm's
/// `await task` wakes on [`ATTENTION_OWNER`]'s entries and reads that owner as a startup's ask,
/// and [`ATTENTION_OWNER`]'s one slot carries codex-alab's asks; a wait never goes there, and
/// an entry sent with no kind is an ask.
pub const WAIT_OWNER: &str = "codex-alab-wait";

/// aterm's cap on a keyed attention text, in bytes.
const ATTENTION_MAX_BYTES: usize = 200;

/// aterm's cap on an `appnotice` text, the words after its lane (`cmd_appnotice`'s
/// `MAX_TEXT_BYTES`): aterm refuses a longer one (`ERR appnotice: text is longer than 512
/// bytes`) and never shortens it, so the client bounds every notice to it
/// ([`Client::appnotice_harness`]).
pub const NOTICE_MAX_BYTES: usize = 512;

/// aterm's cap on a session's `description`, in bytes.
pub const DESCRIPTION_MAX_BYTES: usize = 1024;

/// The verbs aterm's `story` takes: a CLOSED set, anything else is `ERR usage`. Since aterm's
/// owner surfaces (aterm main `189ec7bc0d`) it is one word, `chose`: the seven others no process
/// sent are refused. A `chose` is one story point on the tab's presence band and one story mark
/// in the session log; the text is checked, not kept.
pub const STORY_VERBS: [&str; 1] = ["chose"];

/// aterm's cap on a story point's text, in bytes.
pub const STORY_MAX_BYTES: usize = 96;

/// The arms aterm's `meta set harness_held` takes: a CLOSED set, aterm's `HeldBy::word`s (what
/// its own loop's `status held=` says), anything else is `ERR usage`.
pub const HELD_ARMS: [&str; 2] = ["same-reply", "no-work"];

/// One leg of Land's hold as aterm's `meta set harness_land legs=` takes it: aterm's `HoldLeg`,
/// a CLOSED set, its [`LandLeg::word`] the one `status land_held=` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LandLeg {
    /// The reply named a need (`NEEDS YOU:`), in its last words or earlier in the same turn.
    Needs,
    /// The session's repository holds work no remote main has.
    Repo,
    /// The repository probe could not read the repository: what it holds is unknown.
    RepoUnread,
    /// Land's strict reading found open work in the reply.
    Open,
    /// No `Nothing left:` defense in the reply.
    Defense,
    /// The reply, or the turn it ends, was taller than the rows aterm's screen read. Codex
    /// hands each message whole, so codex-alab never sends it.
    Cut,
}

impl LandLeg {
    /// Every leg, in aterm's `HoldLeg::ALL` order (the order the owner can act on them), the
    /// order [`LandReport`] names them in.
    pub const ALL: [LandLeg; 6] = [
        LandLeg::Needs,
        LandLeg::Repo,
        LandLeg::RepoUnread,
        LandLeg::Open,
        LandLeg::Defense,
        LandLeg::Cut,
    ];

    /// Its word on the wire and in `status land_held=` (aterm's `HoldLeg::word`).
    pub const fn word(self) -> &'static str {
        match self {
            LandLeg::Needs => "needs",
            LandLeg::Repo => "repo",
            LandLeg::RepoUnread => "repo-unread",
            LandLeg::Open => "open",
            LandLeg::Defense => "defense",
            LandLeg::Cut => "cut",
        }
    }

    /// The leg `word` names ([`Self::word`]'s inverse); `None` for any other word.
    pub fn of_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|leg| leg.word() == word)
    }
}

/// The most open words aterm keeps of a hold (its `LandHeld::open`): [`LandRead::open`] names
/// no more.
pub const LAND_OPEN_WORDS: usize = 4;

/// LAND'S HOLD, as aterm's `meta set harness_land` takes it (CONTRACT §5 visibility, gap 11 of
/// the parity sweep): aterm's `LandHeld`, shown as its own Land hold's "landing" is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandReport {
    /// When the land act that began the hold was typed, unix ms (`since=`).
    pub since_ms: u64,
    /// The nudges (`land-nudge@v1`) typed since that act (`nudges=`).
    pub nudges: u32,
    /// What held the landing at the land act's yield judged last, where one was judged and no
    /// other turn ran since (`read=1`); `None`: landing, with no legs read.
    pub read: Option<LandRead>,
}

/// A land act's yield as judged (aterm's `LandHeld` behind its `read`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LandRead {
    /// What held the landing (`legs=`): at least one, named in [`LandLeg::ALL`]'s order once
    /// each.
    pub legs: Vec<LandLeg>,
    /// Up to [`LAND_OPEN_WORDS`] of the words Land's strict reading found open, as it spelled
    /// them (`open=`); aterm keeps those its own reading knows.
    pub open: Vec<String>,
    /// The repository's work at that yield (`changed=`, `untracked=`, `stashes=`, `unpushed=`,
    /// `unlanded=`, each sent where it is not 0).
    pub changed: u16,
    pub untracked: u16,
    pub stashes: u16,
    pub unpushed: u16,
    pub unlanded: u16,
}

impl LandReport {
    /// Its tokens after `meta set harness_land`, or why the client refuses to send it: a read
    /// with no leg (aterm's usage), more than [`LAND_OPEN_WORDS`] open words, or an empty one.
    fn tokens(&self) -> Result<String, String> {
        let mut line = format!("since={} nudges={}", self.since_ms, self.nudges);
        let Some(read) = &self.read else {
            return Ok(line);
        };
        if read.legs.is_empty() {
            return Err("harness_land: a read hold names a leg".to_string());
        }
        if read.open.len() > LAND_OPEN_WORDS || read.open.iter().any(String::is_empty) {
            return Err(format!("harness_land open={:?}", read.open));
        }
        let legs: Vec<&str> = LandLeg::ALL
            .into_iter()
            .filter(|leg| read.legs.contains(leg))
            .map(LandLeg::word)
            .collect();
        line.push_str(&format!(" read=1 legs={}", legs.join(",")));
        if !read.open.is_empty() {
            // Each word escaped as aterm escapes a value, its commas too: they part the words.
            let open: Vec<String> = read
                .open
                .iter()
                .map(|word| wire::pct_encode(word).replace(',', "%2C"))
                .collect();
            line.push_str(&format!(" open={}", open.join(",")));
        }
        for (key, n) in [
            ("changed", read.changed),
            ("untracked", read.untracked),
            ("stashes", read.stashes),
            ("unpushed", read.unpushed),
            ("unlanded", read.unlanded),
        ] {
            if n > 0 {
                line.push_str(&format!(" {key}={n}"));
            }
        }
        Ok(line)
    }
}

/// The most needs aterm's `meta set harness_needs` takes (`need=` one to this many times): a
/// larger set is told by its first this many, in the worker's order.
pub const NEEDS_TOLD: usize = 32;

/// aterm's cap on one need `meta set harness_needs` takes, in bytes decoded: its
/// `needs::NEED_TOLD_MAX`, four bytes for each of a need's `NEED_CHARS` (160) characters, so a
/// need as the reading keeps it always fits.
pub const NEED_TOLD_MAX_BYTES: usize = 160 * 4;

/// The tokens after `meta set harness_needs` for the set `needs` beside the `owner=needs` entry
/// `entry` (gap 16's wire), or why the client refuses to send it: aterm's usage for an empty
/// entry, no need, more than [`NEEDS_TOLD`], or a need empty once folded or past
/// [`NEED_TOLD_MAX_BYTES`] (aterm d749fecec5). The entry is cut to one line of aterm's attention
/// cap as [`Client::set_attention_owned`] cuts it, so it names the entry aterm stores; each value
/// is escaped as aterm escapes one, a bare `-` as `%2D`, the wire's literal `-` ([`wire`]),
/// though aterm's strict reader of this verb (`harness_needs_of`) has no unset value and decodes
/// either to the need `-`.
fn needs_tokens(entry: &str, needs: &[String]) -> Result<String, String> {
    let value = |text: &str| {
        if text == "-" {
            "%2D".to_string()
        } else {
            wire::pct_encode(text)
        }
    };
    let entry = wire::single_line(entry, ATTENTION_MAX_BYTES);
    if entry.is_empty() {
        return Err("harness_needs: an empty entry".to_string());
    }
    if needs.is_empty() || needs.len() > NEEDS_TOLD {
        return Err(format!("harness_needs: {} needs", needs.len()));
    }
    if let Some(need) = needs.iter().find(|need| {
        need.len() > NEED_TOLD_MAX_BYTES || wire::single_line(need, NEED_TOLD_MAX_BYTES).is_empty()
    }) {
        return Err(format!("harness_needs need={need:?}"));
    }
    let mut line = format!("entry={}", value(&entry));
    for need in needs {
        line.push_str(" need=");
        line.push_str(&value(need));
    }
    Ok(line)
}

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

/// aterm's cap on a `notice later` key (it lives as `wire.<key>`), in bytes.
pub const LATER_KEY_MAX: usize = 40;

/// aterm's cap on a `notice later` reason and on its step, in characters.
pub const LATER_FIELD_MAX_CHARS: usize = 90;

/// aterm's cap on a `notice later` detail: this many lines ...
pub const LATER_DETAIL_LINES: usize = 3;

/// ... of at most this many characters each.
pub const LATER_DETAIL_LINE_MAX_CHARS: usize = 240;

/// aterm's cap on a `notice later` `url=`, in bytes.
const LATER_URL_MAX_BYTES: usize = 512;

/// aterm's cap on a band or digest title, in characters (at most 6 words besides).
const LATER_TITLE_MAX_CHARS: usize = 48;

/// One `notice later` entry ([`Client::notice_later`]): a fact that needs the owner, but not
/// now, filed as one keyed entry of aterm's exceptions digest (the menu-bar item and Settings ▸
/// Messages), never a band row. Every field is whole or the request is refused: aterm cuts
/// nothing, so a caller fits its words first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Later {
    /// The fact's identity, `[a-z0-9._-]{1,40}` (aterm keeps it as `wire.<key>`).
    pub key: String,
    /// The entry's family (`script` when `None`); never one of aterm's own.
    pub tag: Option<String>,
    /// The page the entry's one button, `Open in browser`, opens: `https://<host>[/<path>]`.
    pub url: Option<String>,
    /// The title: at most 6 words and 48 characters, with no clause.
    pub title: String,
    /// Why only the owner can settle it, at most [`LATER_FIELD_MAX_CHARS`].
    pub reason: String,
    /// The one step that settles it, at most [`LATER_FIELD_MAX_CHARS`].
    pub step: String,
    /// At most [`LATER_DETAIL_LINES`] lines of at most [`LATER_DETAIL_LINE_MAX_CHARS`].
    pub detail: Vec<String>,
}

impl Later {
    /// The request line, `notice later <key> [tag=<tag>] [url=<url>] <title> -- <reason> --
    /// <step>[ -- <detail>]`, its detail lines joined by a literal `\n` as aterm splits them.
    /// A field that would not reach aterm whole, or would move another field's boundary, is
    /// [`Error::BadRequest`]; aterm's own word rules (the title's form, a reason that only
    /// says something is pending, a step that is a snooze) are aterm's to answer.
    pub fn line(&self) -> Result<String, Error> {
        let bad = |what: &str| Err(Error::BadRequest(format!("notice later {what}")));
        if !later_key_ok(&self.key) {
            return bad("key");
        }
        let mut line = format!("notice later {}", self.key);
        if let Some(tag) = &self.tag {
            if !later_tag_ok(tag) {
                return bad("tag");
            }
            line.push_str(&format!(" tag={tag}"));
        }
        if let Some(url) = &self.url {
            let url_ok = url.len() <= LATER_URL_MAX_BYTES
                && url.starts_with("https://")
                && url.bytes().all(|b| {
                    b.is_ascii_graphic() && !matches!(b, b'\\' | b'"' | b'<' | b'>' | b'`')
                });
            if !url_ok {
                return bad("url");
            }
            line.push_str(&format!(" url={url}"));
        }
        let title = later_field(&self.title, LATER_TITLE_MAX_CHARS);
        let reason = later_field(&self.reason, LATER_FIELD_MAX_CHARS);
        let step = later_field(&self.step, LATER_FIELD_MAX_CHARS);
        let (Some(title), Some(reason), Some(step)) = (title, reason, step) else {
            return bad("title, reason or step");
        };
        // The first word after the options would be read as one.
        if title.starts_with("tag=") || title.starts_with("url=") {
            return bad("title");
        }
        line.push_str(&format!(" {title} -- {reason} -- {step}"));
        let mut detail = Vec::with_capacity(self.detail.len());
        for text in &self.detail {
            let text = wire::single_line(text, usize::MAX);
            if text.chars().count() > LATER_DETAIL_LINE_MAX_CHARS || text.contains("\\n") {
                return bad("detail");
            }
            if !text.is_empty() {
                detail.push(text);
            }
        }
        if detail.len() > LATER_DETAIL_LINES {
            return bad("detail");
        }
        if !detail.is_empty() {
            line.push_str(&format!(" -- {}", detail.join("\\n")));
        }
        Ok(line)
    }
}

/// A `notice later` key aterm takes: `[a-z0-9._-]{1,40}`.
fn later_key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= LATER_KEY_MAX
        && key.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// A `notice later` tag aterm takes: `[a-z0-9-]{1,24}`.
fn later_tag_ok(tag: &str) -> bool {
    (1..=24).contains(&tag.len())
        && tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One title, reason or step on one line, if it fits in `max_chars` and keeps the ` -- `
/// that ends it the only one.
fn later_field(text: &str, max_chars: usize) -> Option<String> {
    let text = wire::single_line(text, usize::MAX);
    let fits = !text.is_empty() && text.chars().count() <= max_chars;
    let whole = !text.starts_with("--") && !text.ends_with(" --") && !text.contains(" -- ");
    (fits && whole).then_some(text)
}

/// How aterm took a `notice later` (`OK later=<id> how=<word>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaterHow {
    /// A new entry.
    New,
    /// The key's entry, its words changed in place.
    Restated,
    /// The key's entry, said again.
    Duplicate,
    /// The owner dismissed or answered the key's entry, and that still holds.
    Quiet,
}

/// What `notice_later` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaterOutcome {
    /// The digest holds the key ([`LaterHow::Quiet`] included: the owner's dismissal governs).
    Taken {
        /// How.
        how: LaterHow,
    },
    /// This aterm has no `notice later` (`ERR usage: notice <post|…`, or no `notice` at all).
    Unsupported(String),
    /// Refused (a word rule, the cap on open wire entries, a full digest, the mint budget).
    Refused(String),
}

/// What `end_open_later` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndOutcome {
    /// The observed open entry ended (`notice settle`): it leaves the digest, and the
    /// key's next `notice later` is news.
    Ended,
    /// The observation no longer authorizes settlement: the entry is absent, dismissed,
    /// replaced, or its native observation changed. Any current entry or dismissal remains.
    NotOpen,
    /// This aterm cannot read an observation token or settle an entry conditionally.
    Unsupported(String),
    /// Refused, as sent.
    Refused(String),
}

/// An open later entry observed at one endpoint and authenticated listener, with the
/// native row's opaque `rev` token. Only a successful live read constructs it; a caller
/// cannot retarget it to another listener, key, entry or token. Native servers with
/// per-entry confirmations keep the token separate from UI revisions. Its private
/// AUTH identity is always redacted.
/// The caller still owns the identity and freshness of any later server response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedLater {
    endpoint: Endpoint,
    key: String,
    id: u64,
    revision: u64,
    auth_identity: crate::conn::AuthIdentity,
}

/// What `observe_open_later` read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObserveLaterOutcome {
    /// The exact entry and opaque native token a later conditional settle may use.
    Open(ObservedLater),
    /// This key has no open later entry.
    NotOpen,
    /// The server cannot provide the observation needed for a safe settle.
    Unsupported(String),
    /// The read was refused, as sent.
    Refused(String),
}

fn notice_number(value: &str) -> Option<u64> {
    wire::canonical_u64(value)
}

fn notice_fields(rest: &str) -> Option<std::collections::BTreeMap<&str, &str>> {
    let mut fields = std::collections::BTreeMap::new();
    for word in rest.split_ascii_whitespace() {
        let (key, value) = word.split_once('=')?;
        if fields.insert(key, value).is_some() {
            return None;
        }
    }
    Some(fields)
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
    /// This aterm has no `harness_act` field (or, for [`Client::harness_act_before_turn`], no
    /// `meta prepare harness_act`).
    Unsupported(String),
    /// Refused (`ERR denied`: not the proven holder, or another refusal).
    Refused(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

/// What `harness_held` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldOutcome {
    /// `OK`: aterm's `status held=` says the arm sent (`-` after an unset) while the claim
    /// stands; the hold lapses with the claim.
    Shown,
    /// `ERR denied`: aterm proves no claim of this connection's here.
    Denied(String),
    /// Any other `ERR`: this aterm has no `harness_held` (one before it answers `ERR unknown meta
    /// field (…)`, as for any field it does not know) or refused the line; nothing is shown
    /// either way.
    Unsupported(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

/// What `harness_land` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandOutcome {
    /// `OK`: aterm shows the hold sent as the tab's landing (none after an unset) while the
    /// claim stands; the hold lapses with the claim.
    Shown,
    /// `ERR denied`: aterm proves no claim of this connection's here.
    Denied(String),
    /// Any other `ERR`: this aterm has no `harness_land` (one before it answers `ERR unknown
    /// meta field (…)`, as for any field it does not know) or refused the line; nothing is
    /// shown either way.
    Unsupported(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

/// What `harness_needs` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeedsOutcome {
    /// `OK`: aterm's readers of a worker's needs (the menu bar's row one level in, the hover's
    /// list) read the set sent, each need whole, while the `owner=needs` entry it describes
    /// stands and the claim stands (none after an unset); the set lapses with the claim.
    Shown,
    /// `ERR denied`: aterm proves no claim of this connection's here.
    Denied(String),
    /// Any other `ERR`: this aterm has no `harness_needs` (one before it answers `ERR unknown
    /// meta field (…)`, as for any field it does not know) or refused the line; aterm's readers
    /// read the entry's first need and its count either way.
    Unsupported(String),
    /// Not sent: no claim is held on this connection.
    NotHeld,
}

/// Whether `status` is aterm's `ERR denied` (no claim of this connection's proven here), as
/// the self-claim's fields (`harness_held`, `harness_land`, `harness_needs`) and its fact reads
/// (`harness fact`) answer one.
pub(crate) fn denied(status: &str) -> bool {
    status
        .strip_prefix("ERR denied")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', ':']))
}

#[derive(Debug, Default)]
struct State {
    policy: Option<HarnessPolicy>,
    claimed: bool,
    /// Same-holder sets ([`Client::reconfirm`]) sent since the claim was last taken.
    reconfirms: u64,
}

/// The accepted listener identity never changes with policy or claim state. Keeping it
/// in the existing shared allocation also keeps a cloned Client's inline size bounded.
#[derive(Debug)]
struct SharedState {
    auth_identity: crate::conn::AuthIdentity,
    state: Mutex<State>,
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
    shared: Arc<SharedState>,
}

impl Client {
    pub(crate) fn endpoint_sid(&self) -> &str {
        &self.endpoint.sid
    }

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
        let auth_identity = opened.auth_identity.clone();
        let conn = Conn::spawn(opened, options.timeout);
        Ok(Client {
            holder: holder_for(options.holder_pid),
            endpoint,
            conn,
            options,
            version,
            server_pid,
            shared: Arc::new(SharedState {
                auth_identity,
                state: Mutex::new(State::default()),
            }),
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

    /// The aterm release this connection speaks to, `(major, minor)`, read off the `version=`
    /// field of its `version` answer (`0.109.0`, or a development build's
    /// `0.110.0+g<sha>`); `None` when that field is absent or names no release.
    pub fn server_release(&self) -> Option<(u64, u64)> {
        release_of(&self.version)
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
        self.shared
            .state
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
    pub(crate) async fn on_self(&self, rest: &str) -> Result<Reply, Error> {
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

    /// `@self task`: the state of this session's task record (`working`, `done`, `stalled`,
    /// `exited`; aterm's `task_read`), `None` where none stands (`OK task -`) or the aterm
    /// refuses the verb (one before task records), as aterm's own loop reads a refusal: no
    /// record. A timeout or a closed connection is an error.
    pub async fn task_state(&self) -> Result<Option<String>, Error> {
        let reply = self.on_self("task").await?;
        let Some(rest) = reply.ok().and_then(|rest| rest.strip_prefix("task")) else {
            return Ok(None);
        };
        Ok(rest
            .split_ascii_whitespace()
            .find_map(|field| field.strip_prefix("state="))
            .map(wire::pct_decode))
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
        self.set_attention_owned(ATTENTION_OWNER, None, &format!("{kind}: {why}"))
            .await
    }

    /// `meta set attention owner=<owner> [kind=<ask|wait>] <text>`: an attention entry under
    /// an owner of its own (`owner` an owner token; `text` one line of at most aterm's cap).
    pub async fn set_attention_owned(
        &self,
        owner: &str,
        kind: Option<&str>,
        text: &str,
    ) -> Result<(), Error> {
        if !wire::valid_owner_token(owner) || kind.is_some_and(|k| !matches!(k, "ask" | "wait")) {
            return Err(Error::BadRequest(format!("attention owner {owner}")));
        }
        let text = wire::single_line(text, ATTENTION_MAX_BYTES);
        if text.is_empty() {
            return Err(Error::BadRequest("empty attention".into()));
        }
        let kind = kind.map(|k| format!(" kind={k}")).unwrap_or_default();
        let reply = self
            .on_self(&format!("meta set attention owner={owner}{kind} {text}"))
            .await?;
        ok_text(&reply).map(|_| ())
    }

    /// Clear codex-alab's keyed attention (only its own entry).
    pub async fn clear_attention(&self) -> Result<(), Error> {
        self.clear_attention_owned(ATTENTION_OWNER).await
    }

    /// `meta unset attention owner=<owner>`.
    pub async fn clear_attention_owned(&self, owner: &str) -> Result<(), Error> {
        if !wire::valid_owner_token(owner) {
            return Err(Error::BadRequest(format!("attention owner {owner}")));
        }
        let reply = self
            .on_self(&format!("meta unset attention owner={owner}"))
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

    /// Say why codex-alab stopped typing into this tab in aterm's `status held=` (CONTRACT §5
    /// visibility, ruling F's typing stop): `@self meta set harness_held <arm>`, or `@self meta
    /// unset harness_held` for `None`, sent only while the claim is held, on the claim's
    /// connection. `arm` is one of [`HELD_ARMS`] (any other is refused here, never sent). aterm
    /// takes it from the proven claim only, and drops it when that claim ends; it keys it by the
    /// claim's proof, so a re-claim by this process that reaches aterm first keeps it.
    pub async fn harness_held(&self, arm: Option<&str>) -> Result<HeldOutcome, Error> {
        if arm.is_some_and(|arm| !HELD_ARMS.contains(&arm)) {
            return Err(Error::BadRequest(format!("harness_held {arm:?}")));
        }
        if !self.holds_claim() {
            return Ok(HeldOutcome::NotHeld);
        }
        let line = match arm {
            Some(arm) => format!("meta set harness_held {arm}"),
            None => "meta unset harness_held".to_string(),
        };
        let reply = self.on_self(&line).await?;
        if reply.ok().is_some() {
            return Ok(HeldOutcome::Shown);
        }
        Ok(if denied(&reply.status) {
            HeldOutcome::Denied(reply.status)
        } else {
            HeldOutcome::Unsupported(reply.status)
        })
    }

    /// Say Land's hold in aterm as its own Land hold is said (CONTRACT §5 visibility, gap 11:
    /// `status land_held=`, the menu bar's "landing", the worker row's tip, the Overview's and
    /// Settings ▸ Harness's hold lines): `@self meta set harness_land since=<ms> nudges=<n>
    /// [read=1 legs=… open=… <counts>]`, or `@self meta unset harness_land` for `None`, sent only
    /// while the claim is held, on the claim's connection. A hold aterm would refuse
    /// ([`LandReport::tokens`]) is refused here, never sent. aterm takes it from the proven claim
    /// only, as `harness_held`, and drops it when that claim ends; it keys it by the claim's
    /// proof, so a re-claim by this process that reaches aterm first keeps it.
    pub async fn harness_land(&self, held: Option<&LandReport>) -> Result<LandOutcome, Error> {
        let line = match held {
            Some(held) => format!(
                "meta set harness_land {}",
                held.tokens().map_err(Error::BadRequest)?
            ),
            None => "meta unset harness_land".to_string(),
        };
        if !self.holds_claim() {
            return Ok(LandOutcome::NotHeld);
        }
        let reply = self.on_self(&line).await?;
        if reply.ok().is_some() {
            return Ok(LandOutcome::Shown);
        }
        Ok(if denied(&reply.status) {
            LandOutcome::Denied(reply.status)
        } else {
            LandOutcome::Unsupported(reply.status)
        })
    }

    /// Tell aterm the worker's NEEDS YOU whole (CONTRACT §5 visibility, gap 16: a codex NEEDS
    /// YOU showed only its first need and a count): `@self meta set harness_needs entry=<entry>
    /// need=<need> [need=<need> …]`, the set `needs` beside the `owner=needs kind=ask` entry
    /// `entry` it describes, each need whole in the worker's order, or `@self meta unset
    /// harness_needs` for `None`, sent only while the claim is held, on the claim's connection.
    /// A set aterm would refuse ([`needs_tokens`]: no need, more than [`NEEDS_TOLD`], an empty
    /// one or one past [`NEED_TOLD_MAX_BYTES`]) is refused here, never sent. aterm tells the set
    /// as its own loop's (`ToldNeeds::tell`), bound to the entry standing: a set told before its
    /// entry stands waits for it, the set before keeps describing the entry it was told with,
    /// and a set that describes no standing entry is never read, so the entry and the set reach
    /// it in either order. It takes the set from the proven claim only, as `harness_held`, and
    /// drops it when that claim ends; it keys it by the claim's proof, so a re-claim by this
    /// process that reaches aterm first keeps it.
    pub async fn harness_needs(
        &self,
        told: Option<(&str, &[String])>,
    ) -> Result<NeedsOutcome, Error> {
        let line = match told {
            Some((entry, needs)) => format!(
                "meta set harness_needs {}",
                needs_tokens(entry, needs).map_err(Error::BadRequest)?
            ),
            None => "meta unset harness_needs".to_string(),
        };
        if !self.holds_claim() {
            return Ok(NeedsOutcome::NotHeld);
        }
        let reply = self.on_self(&line).await?;
        if reply.ok().is_some() {
            return Ok(NeedsOutcome::Shown);
        }
        Ok(if denied(&reply.status) {
            NeedsOutcome::Denied(reply.status)
        } else {
            NeedsOutcome::Unsupported(reply.status)
        })
    }

    /// THE SESSION'S UNTOLD ATERM FACT FOR AN ACT UNDER `rule` (CONTRACT §5 "aterm fact", gap 23):
    /// `@self harness fact for=<rule> reader=<reader> kinds=no-progress wall=<wall>`, sent only
    /// while the claim is held, on the claim's connection. `reader` names the thread the act is
    /// typed into (aterm leases an offered episode to it), `wall` its wall track's class word or
    /// `-`. A malformed token is refused here, never sent. The answer is read as
    /// [`FactAnswer::parse`] reads it; a timeout or a closed connection is an error, which has
    /// closed the connection and with it the claim, as for every call on it.
    pub async fn harness_fact(
        &self,
        rule: &str,
        reader: &str,
        wall: &str,
    ) -> Result<FactAnswer, Error> {
        if !facts::valid_rule(rule) || !facts::valid_reader(reader) || !facts::valid_wall(wall) {
            return Err(Error::BadRequest(format!(
                "harness fact for={rule} reader={reader} wall={wall}"
            )));
        }
        if !self.holds_claim() {
            return Ok(FactAnswer::NotHeld);
        }
        let reply = self
            .on_self(&format!(
                "harness fact for={rule} reader={reader} kinds={} wall={wall}",
                facts::FACT_NO_PROGRESS
            ))
            .await?;
        Ok(FactAnswer::parse(&reply.status))
    }

    /// THE ACT THAT CARRIED `episode`'S FACT STARTED AND WAS RECORDED: `@self harness fact told
    /// episode=<016x>`, sent only while the claim is held, on the claim's connection (aterm takes
    /// it only for an episode offered under this claim's proof). Read as [`ToldOutcome::parse`]
    /// reads it.
    pub async fn harness_fact_told(&self, episode: u64) -> Result<ToldOutcome, Error> {
        if !self.holds_claim() {
            return Ok(ToldOutcome::NotHeld);
        }
        let reply = self
            .on_self(&format!("harness fact told episode={episode:016x}"))
            .await?;
        Ok(ToldOutcome::parse(&reply.status))
    }

    /// Record a task-face decision before another native turn may start. The
    /// server's deadline is opaque: on a transport error, wait its full lifetime
    /// from the prepare acknowledgement, so even a queued server handler expires
    /// before the caller releases its native-acceptance gate. The caller must
    /// keep this future alive through cancellation of its own evaluation.
    /// [`ActOutcome::Unsupported`] only where aterm has no `meta prepare harness_act`
    /// ([`prepare_unknown`]: one before 0.110.0), nothing sent after it; whether the unbounded
    /// [`Client::harness_act`] may stand in for that aterm is the caller's to judge.
    pub async fn harness_act_before_turn(&self, act: &str) -> Result<ActOutcome, Error> {
        if !wire::valid_owner_token(act) {
            return Err(Error::BadRequest(act.to_string()));
        }
        if !self.holds_claim() {
            return Ok(ActOutcome::NotHeld);
        }
        let prepared = self.on_self("meta prepare harness_act").await?;
        let Some(fields) = prepared.ok() else {
            // An older aterm cannot fence a delayed write: it answers its plain `meta`
            // usage, and whether the unbounded spelling may stand in is the caller's to
            // decide. Any other answer (`ERR denied`: not the proven holder) is a refusal.
            return Ok(if prepare_unknown(&prepared.status) {
                ActOutcome::Unsupported(prepared.status)
            } else {
                ActOutcome::Refused(prepared.status)
            });
        };
        let mut fields = fields.split_whitespace();
        let until_us = fields
            .next()
            .and_then(|field| field.strip_prefix("until_us="))
            .and_then(wire::canonical_u64)
            .filter(|value| *value > 0);
        let lease_ms = fields
            .next()
            .and_then(|field| field.strip_prefix("lease_ms="))
            .and_then(wire::canonical_u64)
            .filter(|value| (1..=5_000).contains(value));
        let (Some(until_us), Some(lease_ms), None) = (until_us, lease_ms, fields.next()) else {
            return Err(Error::Malformed("invalid harness act lifetime".into()));
        };
        let safe_after = tokio::time::Instant::now() + Duration::from_millis(lease_ms);
        let outcome = self
            .on_self(&format!("meta set harness_act {act} until_us={until_us}"))
            .await
            .map(|reply| {
                if reply.ok().is_some() {
                    ActOutcome::Recorded
                } else {
                    ActOutcome::Refused(reply.status)
                }
            });
        if outcome.is_err() {
            tokio::time::sleep_until(safe_after).await;
        }
        outcome
    }

    /// Tell aterm's session log that codex-alab chose (`@self story <verb> [<text>]`), as
    /// aterm's own approval loop tells a question it answered (`story chose <policy word>`).
    /// Sent only while the claim is held, on the claim's connection. `verb` is one of
    /// [`STORY_VERBS`] (any other is refused here, never sent); `text` becomes one line of at
    /// most [`STORY_MAX_BYTES`] bytes, and is never a command.
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

    /// `appnotice harness <head><tail>`: an app-wide record in aterm's message log, never a
    /// row. One line within aterm's [`NOTICE_MAX_BYTES`] ([`wire::elide_before`]): whole where
    /// it fits, else the end of `head` gives way to `…` and `tail` stays whole (an act's fact
    /// and its why and turn, CONTRACT §5 "aterm fact").
    pub async fn appnotice_harness(&self, head: &str, tail: &str) -> Result<(), Error> {
        let text = wire::elide_before(head, tail, NOTICE_MAX_BYTES);
        if text.is_empty() {
            return Err(Error::BadRequest("empty notice".into()));
        }
        let reply = self.request(&format!("appnotice harness {text}")).await?;
        ok_text(&reply).map(|_| ())
    }

    /// `notice later …` ([`Later`]): file a fact that needs the owner but not now as one entry
    /// of aterm's exceptions digest, however many tabs and launches say it. An `OK` whose
    /// `how=` this reader does not know is [`Error::Malformed`]; a timeout or a closed
    /// connection is an error.
    pub async fn notice_later(&self, later: &Later) -> Result<LaterOutcome, Error> {
        let reply = self.request(&later.line()?).await?;
        if let Some(rest) = reply.ok() {
            let fields = wire::parse_fields(rest);
            let how = match fields.get("how").map(String::as_str) {
                Some("new") => LaterHow::New,
                Some("restated") => LaterHow::Restated,
                Some("duplicate") => LaterHow::Duplicate,
                Some("quiet") => LaterHow::Quiet,
                _ => return Err(Error::Malformed(reply.status)),
            };
            return Ok(LaterOutcome::Taken { how });
        }
        // An aterm before `later` answers its `notice` usage, one before `notice` its verbs';
        // this aterm's own `later` usage is a refusal of the line.
        if reply.status.starts_with("ERR usage: notice <")
            || reply.status.starts_with("ERR unknown verb")
        {
            return Ok(LaterOutcome::Unsupported(reply.status));
        }
        Ok(LaterOutcome::Refused(reply.status))
    }

    /// Observe and conditionally settle this key's open later entry. A dismissal, reopening
    /// or changed native observation after the read leaves it untouched. There is no
    /// key-only `done` fallback: that verb also forgets an owner's dismissal.
    pub async fn end_open_later(&self, key: &str, tag: &str) -> Result<EndOutcome, Error> {
        match self.observe_open_later(key, tag).await? {
            ObserveLaterOutcome::Open(observed) => self.settle_observed_later(&observed).await,
            ObserveLaterOutcome::NotOpen => Ok(EndOutcome::NotOpen),
            ObserveLaterOutcome::Unsupported(why) => Ok(EndOutcome::Unsupported(why)),
            ObserveLaterOutcome::Refused(why) => Ok(EndOutcome::Refused(why)),
        }
    }

    /// Snapshot the exact open entry before asynchronous work that may decide its cause
    /// ended. Missing observation-token support fails closed; the read changes no notice.
    pub async fn observe_open_later(
        &self,
        key: &str,
        tag: &str,
    ) -> Result<ObserveLaterOutcome, Error> {
        if !later_key_ok(key) || !later_tag_ok(tag) {
            return Err(Error::BadRequest("notice settle key or tag".to_string()));
        }
        let reply = self
            .request_lines(&format!("messages live need=later tag={tag}"))
            .await?;
        if reply.ok().is_none() {
            if reply.status.starts_with("ERR usage: messages")
                || reply.status.starts_with("ERR unknown verb")
            {
                return Ok(ObserveLaterOutcome::Unsupported(reply.status));
            }
            return Ok(ObserveLaterOutcome::Refused(reply.status));
        }
        let held = format!("wire.{key}");
        let mut observed = None;
        for row in &reply.lines {
            let mut words = row.splitn(3, ' ');
            let (Some("message"), Some(id), Some(rest)) =
                (words.next(), words.next(), words.next())
            else {
                return Err(Error::Malformed(row.clone()));
            };
            let fields = notice_fields(rest).ok_or_else(|| Error::Malformed(row.clone()))?;
            if fields.get("key").copied() != Some(held.as_str()) {
                continue;
            }
            let id = notice_number(id)
                .filter(|id| *id > 0)
                .ok_or_else(|| Error::Malformed(row.clone()))?;
            if fields.get("need").copied() != Some("later")
                || fields.get("tag").copied() != Some(tag)
                || fields.get("origin").copied() != Some("wire")
                || observed.is_some()
            {
                return Err(Error::Malformed(row.clone()));
            }
            let Some(revision) = fields.get("rev") else {
                return Ok(ObserveLaterOutcome::Unsupported(
                    "messages entry has no digest revision".to_string(),
                ));
            };
            let revision = notice_number(revision).ok_or_else(|| Error::Malformed(row.clone()))?;
            observed = Some(ObservedLater {
                endpoint: self.endpoint.clone(),
                key: key.to_string(),
                id,
                revision,
                auth_identity: self.shared.auth_identity.clone(),
            });
        }
        Ok(observed.map_or(ObserveLaterOutcome::NotOpen, ObserveLaterOutcome::Open))
    }

    /// Settle only the entry and opaque token previously observed at this endpoint's exact
    /// authenticated listener. Native validation is atomic; an older server is never
    /// sent an unconditional `notice done`.
    pub async fn settle_observed_later(
        &self,
        observed: &ObservedLater,
    ) -> Result<EndOutcome, Error> {
        if observed.endpoint != self.endpoint {
            return Err(Error::BadRequest(
                "notice settle endpoint changed".to_string(),
            ));
        }
        if observed.auth_identity != self.shared.auth_identity {
            return Err(Error::BadRequest(
                "notice settle listener changed".to_string(),
            ));
        }
        let reply = self
            .request(&format!(
                "notice settle {} {} {}",
                observed.key, observed.id, observed.revision
            ))
            .await?;
        let Some(rest) = reply.ok() else {
            if reply.status.starts_with("ERR usage: notice")
                || reply.status.starts_with("ERR unknown verb")
            {
                return Ok(EndOutcome::Unsupported(reply.status));
            }
            return Ok(EndOutcome::Refused(reply.status));
        };
        let fields = notice_fields(rest).ok_or_else(|| Error::Malformed(reply.status.clone()))?;
        match (fields.get("settled").copied(), fields.get("how").copied()) {
            (Some("-"), Some("gone")) if fields.len() == 2 => Ok(EndOutcome::NotOpen),
            (Some(id), Some("resolved-ok"))
                if fields.len() == 2 && notice_number(id) == Some(observed.id) =>
            {
                Ok(EndOutcome::Ended)
            }
            _ => Err(Error::Malformed(reply.status)),
        }
    }

    /// Open the `events` push stream for this session on a second connection.
    pub async fn subscribe(&self) -> Result<Events, Error> {
        Events::open(&self.endpoint, self.options.timeout).await
    }
}

/// Whether a refused `meta prepare harness_act` says this aterm has no such form, as one
/// before aterm 0.110.0 (bf9d401e40) answers it: its plain `meta` usage, which names no
/// `prepare` (`ERR usage: meta [set <field> <text...> | unset <field>]`), or a refusal of the
/// field or the verb. This aterm's own usage of the form names `prepare`: a refusal.
pub fn prepare_unknown(status: &str) -> bool {
    (status.starts_with("ERR usage: meta") && !status.contains("prepare"))
        || status.starts_with("ERR unknown meta field")
        || status.starts_with("ERR unknown verb")
}

/// `(major, minor)` of the `version=` field in a `version` answer's fields.
pub(crate) fn release_of(answer: &str) -> Option<(u64, u64)> {
    let fields = wire::parse_fields(answer);
    let version = fields.get("version")?.split(['+', '-']).next()?;
    let mut parts = version.split('.');
    let mut next = || parts.next().and_then(wire::canonical_u64);
    Some((next()?, next()?))
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
