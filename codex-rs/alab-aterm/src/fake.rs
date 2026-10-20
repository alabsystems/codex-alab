// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An in-process FAKE aterm for tests: a control socket in a scratch directory that
//! speaks the wire this crate speaks, with CONTRACT §1-§3's rules.
//!
//! It lays the directory out as aterm does (`aterm-<pid>.sock`, `aterm-<pid>.token`,
//! `graph/<sid>`), so [`crate::locate::locate_in`] finds it. It serves `version`,
//! `whoami`, `@<sid> status|meta|harness policy`, `meta set|unset supervisor|attention|
//! description|harness_act`, `story`, `appnotice`, `sessions`, `subscribe @<sid>
//! events[,mail]`, the terminal-context reads `panes`, `blocks`, `blocktext` and
//! `selection` (this session as `@<sid>` or its local id `@0`, and the split panes
//! [`FakeAterm::add_pane`] adds), and the inbox: `inbox [--peek] [--meta]`, `inbox get
//! <id>`, `inbox seen <id> [<verdict>]` and a length-framed `post … len=<n>` (mail arrives
//! by [`FakeAterm::deliver`], pushed as a `MAIL` frame).
//!
//! - **Proven claim** ([`Mode::Current`]): a `meta set supervisor codex-alab@<pid>` with
//!   no `ttl=` is proven only when the connection's kernel peer pid equals `<pid>` and
//!   `<pid>` is the foreground leader ([`FakeAterm::set_foreground_leader`]; by default
//!   this test process). A proven claim replaces an `aterm-harness@…` claim, or a proven
//!   claim whose pid is gone; any other live holder is `ERR busy supervisor=<holder>`.
//! - **Connection-bound**: a claim without `ttl=` ends when its connection closes or turns
//!   into a subscribe stream; a proven claim ends when its pid is gone.
//! - **Old aterm** ([`Mode::Old`]): no `harness policy` (`ERR unknown verb`), no
//!   `supervisor_proven=`, no `harness_act`; every claim is unproven.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::Endpoint;
use crate::conn::read_line;
use crate::wire::pct_encode;

/// Which aterm the fake plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// An aterm with CONTRACT §1-§3 (proven self-claim, `harness policy`, `harness_act`).
    Current,
    /// An aterm before them: everything codex-alab must treat as "never claim".
    Old,
}

/// The claim the fake holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeClaim {
    /// Its holder.
    pub holder: String,
    /// The connection it is bound to (`None` for a `ttl=` or test-made claim).
    pub conn: Option<u64>,
    /// The pid it was proven for.
    pub proven_pid: Option<u32>,
}

const META_FIELDS: &str =
    "ERR unknown meta field (title|description|icon|role|attention|questions)\n";
/// This session's local id (`status` prints `sid=0`).
const SELF_LOCAL: &str = "0";

fn lines_reply(lines: Option<&Vec<String>>) -> String {
    let lines = lines.map(Vec::as_slice).unwrap_or_default();
    let mut out = format!("OK {}\n", lines.len());
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// One message in the fake's inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeMail {
    pub id: u64,
    pub off: u64,
    pub from: String,
    pub kind: String,
    pub trust: String,
    pub body: String,
}

/// One `post` the fake took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakePost {
    /// The options before the body (`to=… kind=… re=… len=…`).
    pub options: Vec<String>,
    pub body: String,
    /// Whether aterm's bridge would route its `to=` (`aterm-link` `resolve_to`). The local
    /// endpoint answers `OK <id>` either way; an unroutable post is retired on the bridge.
    pub routed: bool,
}

/// aterm's bridge routing of a `to=` (`aterm-link` `Bridge::resolve_to`, `say` aside): a
/// session only as `@s-<sid>[@n-<node>]`, anything else only as one bare principal (a class
/// prefix `s-|n-|h-|a-` and 1..=32 of `[a-z0-9-]`).
fn bridge_routes(to: &str) -> bool {
    fn principal(p: &str) -> bool {
        ["s-", "n-", "h-", "a-"]
            .iter()
            .find_map(|c| p.strip_prefix(c))
            .is_some_and(|name| {
                (1..=32).contains(&name.len())
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
    }
    let to = crate::wire::pct_decode(to);
    match to.strip_prefix('@') {
        Some(rest) => {
            let (sid, node) = match rest.split_once('@') {
                Some((sid, node)) => (sid, Some(node)),
                None => (rest, None),
            };
            principal(sid)
                && sid.starts_with("s-")
                && node.is_none_or(|n| principal(n) && n.starts_with("n-"))
        }
        None => principal(&to),
    }
}

const SUPERVISOR_USAGE: &str = "ERR usage: meta set supervisor <holder> [ttl=<ms>] | meta unset supervisor [holder=<holder>]\n";

struct State {
    mode: Mode,
    sid: String,
    token: String,
    claim: Option<FakeClaim>,
    foreground_leader: Option<u32>,
    gone: HashSet<u32>,
    status: Vec<(String, String)>,
    policy_gen: u64,
    effective: bool,
    why: String,
    policy_keys: Vec<(String, String)>,
    policy_err: Option<String>,
    attention: BTreeMap<String, String>,
    /// The owners of the attention entries, the most recently set last (aterm shows that one).
    attention_order: Vec<String>,
    description: Option<String>,
    descriptions: Vec<String>,
    acts: Vec<String>,
    stories: Vec<(String, String)>,
    notices: Vec<String>,
    /// Split panes beside this session (local ids; this session is `0`).
    panes: Vec<String>,
    blocks: BTreeMap<String, Vec<String>>,
    outputs: BTreeMap<(String, u64), Vec<String>>,
    selections: BTreeMap<String, Vec<String>>,
    inbox: Vec<FakeMail>,
    inbox_seen: u64,
    receipts: Vec<(u64, String)>,
    posts: Vec<FakePost>,
    /// Answer every `post` with this `ERR …` line (its body still read).
    refuse_post: Option<String>,
    next_off: u64,
    requests: Vec<String>,
    stalled: HashSet<String>,
    /// Verbs whose requests wait, read but unanswered, until the gate opens.
    gated: HashSet<String>,
    /// Requests that met a closed gate, in arrival order.
    gated_seen: Vec<String>,
    subscribers: Vec<(u64, mpsc::UnboundedSender<String>)>,
    /// The next claim clear's `value=-` event is held back ([`FakeAterm::delay_clear_event`]).
    delay_clear: bool,
    /// A held-back clear event: pushed right after the next event.
    delayed: Option<String>,
    mail_subscribers: Vec<(u64, mpsc::UnboundedSender<String>)>,
    refuse_mail_stream: bool,
    next_conn: u64,
    changes: watch::Sender<u64>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl State {
    fn changed(&self) {
        self.changes.send_modify(|n| *n += 1);
    }

    fn emit(&mut self, event: &str) {
        let frame = format!("EVENT 1 {event}\n");
        if self.delay_clear && event == "meta field=supervisor value=-" {
            self.delay_clear = false;
            self.delayed = Some(frame);
            return;
        }
        self.subscribers
            .retain(|(_, tx)| tx.send(frame.clone()).is_ok());
        if let Some(late) = self.delayed.take() {
            self.subscribers
                .retain(|(_, tx)| tx.send(late.clone()).is_ok());
        }
    }

    fn set_claim(&mut self, claim: Option<FakeClaim>) {
        let before = self.claim.as_ref().map(|c| c.holder.clone());
        let after = claim.as_ref().map(|c| c.holder.clone());
        self.claim = claim;
        if before != after {
            let value = after.as_deref().map_or_else(|| "-".to_string(), pct_encode);
            self.emit(&format!("meta field=supervisor value={value}"));
        }
        self.changed();
    }

    /// The entry aterm shows (its most recently written one).
    fn shown_attention(&self) -> Option<String> {
        self.attention_order
            .last()
            .and_then(|o| self.attention.get(o).cloned())
    }

    /// Set `owner`'s attention entry, as aterm's `AttentionOwners::put` does: a new text makes it
    /// the newest; the same text again is a no-op (its stamp stays). A move of the shown entry
    /// is announced.
    fn put_attention(&mut self, owner: &str, text: &str) {
        if self.attention.get(owner).map(String::as_str) == Some(text) {
            return;
        }
        let before = self.shown_attention();
        self.attention.insert(owner.to_string(), text.to_string());
        self.attention_order.retain(|o| o != owner);
        self.attention_order.push(owner.to_string());
        self.announce_attention(before);
        self.changed();
    }

    /// Announce the shown entry when it moved from `before` (aterm's `meta-change` is the
    /// effective value's).
    fn announce_attention(&mut self, before: Option<String>) {
        let after = self.shown_attention();
        if after != before {
            let value = after.as_deref().map_or_else(|| "-".to_string(), pct_encode);
            self.emit(&format!("meta field=attention value={value}"));
        }
    }

    fn release_conn(&mut self, conn: u64) {
        if self.claim.as_ref().is_some_and(|c| c.conn == Some(conn)) {
            self.set_claim(None);
        }
    }

    fn handle(&mut self, line: &str, conn: u64, peer_pid: Option<u32>) -> String {
        self.requests.push(line.to_string());
        self.changed();
        let (selector, rest) = match line.strip_prefix('@') {
            Some(r) => match r.split_once(' ') {
                Some((sel, rest)) => (Some(sel), rest),
                None => (Some(r), ""),
            },
            None => (None, line),
        };
        let (verb, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        let local = match selector {
            None => Some(SELF_LOCAL.to_string()),
            Some(sel) if sel == self.sid || sel == SELF_LOCAL => Some(SELF_LOCAL.to_string()),
            Some(sel) => self.panes.iter().find(|p| p.as_str() == sel).cloned(),
        };
        let Some(local) = local else {
            return "ERR no such session\n".to_string();
        };
        match verb {
            "blocks" => return lines_reply(self.blocks.get(&local)),
            "selection" => return lines_reply(self.selections.get(&local)),
            "blocktext" => {
                let id = tail
                    .split_ascii_whitespace()
                    .next()
                    .and_then(|i| i.parse().ok());
                return match id.and_then(|id| self.outputs.get(&(local, id))) {
                    Some(lines) => {
                        let mut out = format!("OK {} trimmed=0\n", lines.len());
                        for line in lines {
                            out.push_str(line);
                            out.push('\n');
                        }
                        out
                    }
                    None => "ERR block output evicted from scrollback\n".to_string(),
                };
            }
            "panes" if local == SELF_LOCAL => {
                let mut rows = vec![format!(
                    "layout tab=0 panes={} zoomed=false terminal=true",
                    self.panes.len() + 1
                )];
                rows.push(format!(
                    "pane session={SELF_LOCAL} rect=0,0,40x80 focused=true"
                ));
                for (i, p) in self.panes.iter().enumerate() {
                    rows.push(format!(
                        "pane session={p} rect=0,{},40x80 focused=false",
                        81 * (i + 1)
                    ));
                }
                return lines_reply(Some(&rows));
            }
            _ if local != SELF_LOCAL => return "ERR no such session\n".to_string(),
            _ => {}
        }
        match verb {
            "version" => "OK version=0.0.0-fake\n".to_string(),
            "whoami" => format!("OK {} 0123456789abcdef0123456789abcdef owner\n", self.sid),
            "status" => self.status_line(),
            "meta" => self.meta(tail, conn, peer_pid),
            "harness" if tail.trim() == "policy" && self.mode == Mode::Current => {
                self.policy_line()
            }
            "appnotice" => match tail.split_once(' ') {
                Some(("harness" | "toolchain" | "update", text)) if !text.trim().is_empty() => {
                    self.notices.push(text.to_string());
                    "OK recorded\n".to_string()
                }
                _ => "ERR usage: appnotice <toolchain|update|harness> <text>\n".to_string(),
            },
            "story" if self.mode == Mode::Current => self.story(tail),
            "inbox" => self.inbox(tail),
            "sessions" => format!("OK 1\n{} program=codex\n", self.sid),
            _ => "ERR unknown verb (try: help)\n".to_string(),
        }
    }

    /// `inbox [--peek] [--meta]`, `inbox get <id>`, `inbox seen <id> [<verdict>]`, as
    /// `aterm-gui-fabric`'s `cmd_inbox_verb` answers them (no limit or `since=` here).
    fn inbox(&mut self, tail: &str) -> String {
        let mut toks = tail.split_ascii_whitespace();
        match toks.next() {
            Some("get") => {
                let id = toks.next().and_then(|i| i.parse::<u64>().ok());
                match id.and_then(|id| self.inbox.iter().find(|m| m.id == id)) {
                    Some(m) => format!("OK {}\n{}", m.body.len(), m.body),
                    None => "ERR no such message\n".to_string(),
                }
            }
            Some("seen") => {
                let Some(id) = toks.next().and_then(|i| i.parse::<u64>().ok()) else {
                    return "ERR usage: inbox seen <id> [handled|refused|deferred]\n".to_string();
                };
                let verdict = toks.next().unwrap_or("-").to_string();
                if !matches!(verdict.as_str(), "-" | "handled" | "refused" | "deferred") {
                    return "ERR usage: inbox seen <id> [handled|refused|deferred]\n".to_string();
                }
                self.inbox_seen = self.inbox_seen.max(id);
                self.receipts.push((id, verdict));
                let off = self
                    .inbox
                    .iter()
                    .find(|m| m.id == id)
                    .map_or_else(|| "-".to_string(), |m| m.off.to_string());
                self.emit(&format!(
                    "inbox-seen {id} off={off} floor=- seen={}",
                    self.inbox_seen
                ));
                self.changed();
                format!("OK seen={}\n", self.inbox_seen)
            }
            _ => {
                let meta = tail.split_ascii_whitespace().any(|t| t == "--meta");
                let mut out = format!(
                    "OK {} hold=0 holder=- seen={} bus_head={} oldest_on_bus=- dropped=0 pending=0\n",
                    self.inbox.len(),
                    self.inbox_seen,
                    self.next_off
                );
                for m in &self.inbox {
                    out.push_str(&format!(
                        "msg {} off={} t=1 from={} kind={} trust={} len={}",
                        m.id,
                        m.off,
                        m.from,
                        m.kind,
                        m.trust,
                        m.body.len()
                    ));
                    if !meta {
                        out.push_str(&format!(" text={}", pct_encode(&m.body)));
                    }
                    out.push('\n');
                }
                out
            }
        }
    }

    /// A length-framed `post <options> len=<n>` and its body.
    fn post(&mut self, options: &str, body: Vec<u8>) -> String {
        if let Some(err) = &self.refuse_post {
            return format!("{err}\n");
        }
        let options: Vec<String> = options
            .split_ascii_whitespace()
            .map(str::to_string)
            .collect();
        if !options.iter().any(|o| o.starts_with("to="))
            || !options.iter().any(|o| o.starts_with("kind="))
        {
            return "ERR usage: post to=<…> kind=<k> [opts] [--] <text>\n".to_string();
        }
        let routed = options
            .iter()
            .find_map(|o| o.strip_prefix("to="))
            .is_some_and(bridge_routes);
        self.posts.push(FakePost {
            options,
            body: String::from_utf8_lossy(&body).into_owned(),
            routed,
        });
        self.changed();
        format!("OK {}\n", self.posts.len())
    }

    /// aterm's `story` grammar (`control_query::parse_story`): one verb of the closed set,
    /// then an optional text of at most 96 bytes with no control character.
    fn story(&mut self, tail: &str) -> String {
        let tail = tail.trim();
        let (verb, text) = tail.split_once(char::is_whitespace).unwrap_or((tail, ""));
        let text = text.trim();
        if !crate::STORY_VERBS.contains(&verb) {
            return format!(
                "ERR usage: story <{}> [<text>]\n",
                crate::STORY_VERBS.join("|")
            );
        }
        if text.len() > crate::STORY_MAX_BYTES {
            return format!(
                "ERR story: text is longer than {} bytes\n",
                crate::STORY_MAX_BYTES
            );
        }
        if text.chars().any(char::is_control) {
            return "ERR story: text carries a control character\n".to_string();
        }
        self.stories.push((verb.to_string(), text.to_string()));
        self.changed();
        format!("OK story={}\n", self.stories.len())
    }

    fn status_line(&self) -> String {
        let mut line = String::from("OK");
        for (k, v) in &self.status {
            if k == "hash" {
                self.push_claim_fields(&mut line);
            }
            line.push_str(&format!(" {k}={v}"));
        }
        line.push('\n');
        line
    }

    fn push_claim_fields(&self, line: &mut String) {
        let holder = self
            .claim
            .as_ref()
            .map_or_else(|| "-".to_string(), |c| pct_encode(&c.holder));
        line.push_str(&format!(" supervisor={holder}"));
        if self.mode == Mode::Current {
            let proven = self.claim.as_ref().is_some_and(|c| c.proven_pid.is_some());
            line.push_str(&format!(" supervisor_proven={}", u8::from(proven)));
        }
    }

    fn policy_line(&self) -> String {
        if let Some(err) = &self.policy_err {
            return format!("{err}\n");
        }
        let effective = if self.effective { "on" } else { "off" };
        let mut line = format!(
            "OK gen={} effective={effective} why={}",
            self.policy_gen,
            pct_encode(&self.why)
        );
        for (k, v) in &self.policy_keys {
            let v = if v.is_empty() {
                "-".to_string()
            } else {
                pct_encode(v)
            };
            line.push_str(&format!(" {k}={v}"));
        }
        line.push('\n');
        line
    }

    fn meta(&mut self, tail: &str, conn: u64, peer_pid: Option<u32>) -> String {
        let mut toks = tail.split_ascii_whitespace();
        let (sub, field) = (toks.next(), toks.next());
        match (sub, field) {
            (None, _) => {
                // aterm shows one entry (its most recently set) and names its owner.
                let newest = self
                    .attention_order
                    .last()
                    .and_then(|o| self.attention.get(o).map(|t| (o, t)));
                let (owner, attention) = newest.map_or_else(
                    || ("-".to_string(), "-".to_string()),
                    |(o, t)| (pct_encode(o), pct_encode(t)),
                );
                let description = self
                    .description
                    .as_deref()
                    .map_or_else(|| "-".to_string(), pct_encode);
                let mut line = format!(
                    "OK title=- description={description} attention={attention} \
                     attention_owner={owner}"
                );
                self.push_claim_fields(&mut line);
                line.push_str(" questions=-\n");
                line
            }
            (Some("set"), Some("supervisor")) => {
                self.set_supervisor(toks.collect(), conn, peer_pid)
            }
            (Some("unset"), Some("supervisor")) => {
                let args: Vec<&str> = toks.collect();
                match args.as_slice() {
                    [] => self.set_claim(None),
                    [h] => match h.strip_prefix("holder=") {
                        Some(h) => {
                            if self.claim.as_ref().is_some_and(|c| c.holder == h) {
                                self.set_claim(None);
                            }
                        }
                        None => return SUPERVISOR_USAGE.to_string(),
                    },
                    _ => return SUPERVISOR_USAGE.to_string(),
                }
                "OK\n".to_string()
            }
            (Some("set"), Some("attention")) => {
                let rest: Vec<&str> = toks.collect();
                let Some((owner, text)) = rest
                    .split_first()
                    .and_then(|(o, t)| Some((o.strip_prefix("owner=")?, t.join(" "))))
                else {
                    return "ERR usage: meta set attention owner=<k> <text...>\n".to_string();
                };
                if text.is_empty() || text.len() > 200 {
                    return "ERR usage: meta set attention owner=<k> <text...>\n".to_string();
                }
                self.put_attention(owner, &text);
                "OK\n".to_string()
            }
            (Some("unset"), Some("attention")) => {
                let owner = toks
                    .next()
                    .and_then(|o| o.strip_prefix("owner="))
                    .unwrap_or("-");
                let before = self.shown_attention();
                if self.attention.remove(owner).is_some() {
                    self.attention_order.retain(|o| o != owner);
                    self.announce_attention(before);
                    self.changed();
                }
                "OK\n".to_string()
            }
            (Some("set"), Some("description")) => {
                let text = toks.collect::<Vec<_>>().join(" ");
                if text.is_empty() || text.len() > 1024 {
                    return "ERR usage: meta set description <text...>\n".to_string();
                }
                self.descriptions.push(text.clone());
                self.description = Some(text.clone());
                self.emit(&format!(
                    "meta field=description value={}",
                    pct_encode(&text)
                ));
                self.changed();
                "OK\n".to_string()
            }
            (Some("unset"), Some("description")) => {
                if self.description.take().is_some() {
                    self.emit("meta field=description value=-");
                    self.changed();
                }
                "OK\n".to_string()
            }
            (Some("set"), Some("harness_act")) if self.mode == Mode::Current => {
                let Some(act) = toks.next() else {
                    return "ERR usage: meta set harness_act <act>\n".to_string();
                };
                let proven_by_caller = self.claim.as_ref().is_some_and(|c| {
                    c.proven_pid.is_some() && (c.conn == Some(conn) || c.proven_pid == peer_pid)
                });
                if !proven_by_caller {
                    return "ERR denied\n".to_string();
                }
                self.acts.push(act.to_string());
                self.changed();
                "OK\n".to_string()
            }
            _ => META_FIELDS.to_string(),
        }
    }

    fn set_supervisor(&mut self, args: Vec<&str>, conn: u64, peer_pid: Option<u32>) -> String {
        let (holder, ttl) = match args.as_slice() {
            [h] => (*h, None),
            [h, t] => match t.strip_prefix("ttl=").and_then(|t| t.parse::<u64>().ok()) {
                Some(ms) if (1..=600_000).contains(&ms) => (*h, Some(ms)),
                _ => return SUPERVISOR_USAGE.to_string(),
            },
            _ => return SUPERVISOR_USAGE.to_string(),
        };
        if !crate::wire::valid_owner_token(holder) {
            return SUPERVISOR_USAGE.to_string();
        }
        let named = holder
            .strip_prefix("codex-alab@")
            .and_then(|p| p.parse::<u32>().ok());
        let proven_pid = match (self.mode, ttl, named) {
            (Mode::Current, None, Some(pid))
                if peer_pid == Some(pid) && self.foreground_leader == Some(pid) =>
            {
                Some(pid)
            }
            _ => None,
        };
        if let Some(other) = &self.claim
            && other.holder != holder
        {
            let replaceable = proven_pid.is_some()
                && (other.holder.starts_with("aterm-harness@")
                    || other.proven_pid.is_some_and(|p| self.gone.contains(&p)));
            if !replaceable {
                return format!("ERR busy supervisor={}\n", pct_encode(&other.holder));
            }
        }
        self.set_claim(Some(FakeClaim {
            holder: holder.to_string(),
            conn: ttl.is_none().then_some(conn),
            proven_pid,
        }));
        "OK\n".to_string()
    }
}

/// A running fake aterm. Dropping it stops accepting (served connections end with the
/// test's runtime).
pub struct FakeAterm {
    endpoint: Endpoint,
    dir: PathBuf,
    state: Arc<Mutex<State>>,
    changes: watch::Receiver<u64>,
    accept: tokio::task::JoinHandle<()>,
}

impl Drop for FakeAterm {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

static NEXT_FAKE: AtomicU32 = AtomicU32::new(1);

fn default_status() -> Vec<(String, String)> {
    [
        ("schema", "1"),
        ("sid", "0"),
        ("subject", "fake%20session"),
        ("phase", "running"),
        ("hold", "0"),
        ("hand", "-"),
        ("program", "codex"),
        ("agent", "idle"),
        ("input", "clear"),
        ("human_ms", "-"),
        ("human_seq", "0"),
        ("copy", "foreign"),
        ("hash", "-"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn default_policy() -> Vec<(String, String)> {
    [
        ("enabled", "true"),
        ("approve", "all"),
        ("trust_roots", "~/aterm*"),
        ("answer_questions", "true"),
        ("answer_text", "Pick the recommended option."),
        ("questions", "recommended"),
        ("continue", "true"),
        ("continue_text", "keep going"),
        ("continue_per_hour", "0"),
        ("retry_api_errors", "true"),
        ("resume_limits", "true"),
        ("compact_on_context_wall", "true"),
        ("relaunch", "true"),
        ("upgrade", "true"),
        ("upgrade_ask", "false"),
        ("human_grace_s", "120"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

impl FakeAterm {
    /// Start a fake aterm in `dir` (a scratch directory the caller owns).
    pub async fn start(dir: &Path, mode: Mode) -> std::io::Result<FakeAterm> {
        let n = NEXT_FAKE.fetch_add(1, Ordering::Relaxed);
        let instance = 4_000_000 + n;
        let sid = format!("s-{:08x}{:012x}", std::process::id(), u64::from(n));
        let token = format!("{:032x}", u128::from(n) * 0x9e37_79b9_7f4a_7c15 + 7);
        let sock = dir.join(format!("aterm-{instance}.sock"));
        write_private(
            &dir.join(format!("aterm-{instance}.token")),
            &format!("{token}\n"),
        )?;
        std::fs::create_dir_all(dir.join("graph"))?;
        let entry = format!("sock {}\npid {instance}\n", sock.display());
        write_private(&dir.join("graph").join(&sid), &entry)?;
        let listener = UnixListener::bind(&sock)?;
        let (changes_tx, changes) = watch::channel(0);
        let state = Arc::new(Mutex::new(State {
            mode,
            sid: sid.clone(),
            token,
            claim: None,
            foreground_leader: Some(std::process::id()),
            gone: HashSet::new(),
            status: default_status(),
            policy_gen: 1,
            effective: true,
            why: "enabled".to_string(),
            policy_keys: default_policy(),
            policy_err: None,
            attention: BTreeMap::new(),
            attention_order: Vec::new(),
            description: None,
            descriptions: Vec::new(),
            acts: Vec::new(),
            stories: Vec::new(),
            notices: Vec::new(),
            panes: Vec::new(),
            blocks: BTreeMap::new(),
            outputs: BTreeMap::new(),
            selections: BTreeMap::new(),
            inbox: Vec::new(),
            inbox_seen: 0,
            receipts: Vec::new(),
            posts: Vec::new(),
            refuse_post: None,
            next_off: 1000,
            requests: Vec::new(),
            stalled: HashSet::new(),
            gated: HashSet::new(),
            gated_seen: Vec::new(),
            subscribers: Vec::new(),
            delay_clear: false,
            delayed: None,
            mail_subscribers: Vec::new(),
            refuse_mail_stream: false,
            next_conn: 0,
            changes: changes_tx,
        }));
        let accept_state = Arc::clone(&state);
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, Arc::clone(&accept_state)));
            }
        });
        Ok(FakeAterm {
            endpoint: Endpoint::explicit(sock, sid),
            dir: dir.to_path_buf(),
            state,
            changes,
            accept,
        })
    }

    /// The endpoint a client connects to.
    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
    }

    /// The rendezvous directory (for [`crate::locate::locate_in`]).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The session id.
    pub fn sid(&self) -> String {
        self.endpoint.sid.clone()
    }

    /// The claim now held, if any.
    pub fn claim(&self) -> Option<FakeClaim> {
        lock(&self.state).claim.clone()
    }

    /// Every request line served, in order.
    pub fn requests(&self) -> Vec<String> {
        lock(&self.state).requests.clone()
    }

    /// Raise `owner`'s keyed attention, as another client would (`meta set attention
    /// owner=<owner> <text>`), announced.
    pub fn set_attention_as(&self, owner: &str, text: &str) {
        lock(&self.state).put_attention(owner, text);
    }

    /// Keyed attention entries.
    pub fn attention(&self) -> BTreeMap<String, String> {
        lock(&self.state).attention.clone()
    }

    /// `harness_act` records.
    pub fn acts(&self) -> Vec<String> {
        lock(&self.state).acts.clone()
    }

    /// The session's `description` now.
    pub fn description(&self) -> Option<String> {
        lock(&self.state).description.clone()
    }

    /// Every `meta set description` written, in order.
    pub fn descriptions(&self) -> Vec<String> {
        lock(&self.state).descriptions.clone()
    }

    /// Set the description as a person's own `meta set description` does.
    pub fn describe_as_person(&self, text: &str) {
        let mut s = lock(&self.state);
        s.description = Some(text.to_string());
        s.changed();
    }

    /// Deliver a message into this session's inbox (as aterm's bridge does), pushed to
    /// `mail` subscribers as `MAIL` and on `events` as `inbox`; answers its `(id, off)`.
    pub fn deliver(&self, from: &str, kind: &str, trust: &str, body: &str) -> (u64, u64) {
        let mut s = lock(&self.state);
        let id = s.inbox.last().map_or(1, |m| m.id + 1);
        s.next_off += 1;
        let off = s.next_off;
        s.inbox.push(FakeMail {
            id,
            off,
            from: from.to_string(),
            kind: kind.to_string(),
            trust: trust.to_string(),
            body: body.to_string(),
        });
        let frame = format!("MAIL 1 id={id} off={off} from={from} kind={kind}\n");
        s.mail_subscribers
            .retain(|(_, tx)| tx.send(frame.clone()).is_ok());
        s.emit(&format!("inbox {id} off={off} from={from} kind={kind}"));
        s.changed();
        (id, off)
    }

    /// `inbox seen` receipts taken, `(id, verdict)` (`-` for none).
    pub fn receipts(&self) -> Vec<(u64, String)> {
        lock(&self.state).receipts.clone()
    }

    /// The posts taken.
    pub fn posts(&self) -> Vec<FakePost> {
        lock(&self.state).posts.clone()
    }

    /// Answer every `post` with this `ERR …` line (`None` takes posts again), as aterm refuses
    /// one it will not carry.
    pub fn refuse_posts(&self, err: Option<&str>) {
        lock(&self.state).refuse_post = err.map(str::to_string);
    }

    /// Refuse `subscribe … events,mail` as an aterm without the `mail` stream would.
    pub fn refuse_mail_stream(&self) {
        lock(&self.state).refuse_mail_stream = true;
    }

    /// `story` points told, `(verb, text)`.
    pub fn stories(&self) -> Vec<(String, String)> {
        lock(&self.state).stories.clone()
    }

    /// Add a split pane beside this session, with local id `local`.
    pub fn add_pane(&self, local: &str) {
        lock(&self.state).panes.push(local.to_string());
    }

    /// Set a session's `blocks` rows (`local` is `0` for this session).
    pub fn set_blocks(&self, local: &str, rows: &[&str]) {
        lock(&self.state).blocks.insert(
            local.to_string(),
            rows.iter().map(ToString::to_string).collect(),
        );
    }

    /// Set one block's output (`blocktext <id>`); a block without one is evicted.
    pub fn set_output(&self, local: &str, id: u64, lines: &[&str]) {
        lock(&self.state).outputs.insert(
            (local.to_string(), id),
            lines.iter().map(ToString::to_string).collect(),
        );
    }

    /// Set a session's selection.
    pub fn set_selection(&self, local: &str, lines: &[&str]) {
        lock(&self.state).selections.insert(
            local.to_string(),
            lines.iter().map(ToString::to_string).collect(),
        );
    }

    /// `appnotice` texts.
    pub fn notices(&self) -> Vec<String> {
        lock(&self.state).notices.clone()
    }

    /// Set the policy's `effective`/`why` (a new generation, announced).
    pub fn set_effective(&self, effective: bool, why: &str) {
        let mut s = lock(&self.state);
        s.effective = effective;
        s.why = why.to_string();
        bump_policy(&mut s);
    }

    /// Move the policy's `effective`/`why` with no `harness-policy` event and no new
    /// generation, as aterm folds a hold, an Autopilot word or a suspension into it.
    pub fn set_effective_quietly(&self, effective: bool, why: &str) {
        let mut s = lock(&self.state);
        s.effective = effective;
        s.why = why.to_string();
        s.changed();
    }

    /// Push one raw `EVENT 1 <event>` frame to every subscriber (`hold 1 reason=x origin=y`,
    /// `meta field=autopilot value=off`).
    pub fn emit(&self, event: &str) {
        let mut s = lock(&self.state);
        s.emit(event);
        s.changed();
    }

    /// Push one raw frame (a line, without its newline) to every subscriber: `GAP 1 events`.
    pub fn emit_frame(&self, frame: &str) {
        let mut s = lock(&self.state);
        let frame = format!("{frame}\n");
        s.subscribers
            .retain(|(_, tx)| tx.send(frame.clone()).is_ok());
        s.changed();
    }

    /// Set one policy key (a new generation, announced).
    pub fn set_policy_key(&self, key: &str, value: &str) {
        let mut s = lock(&self.state);
        match s.policy_keys.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value.to_string(),
            None => s.policy_keys.push((key.to_string(), value.to_string())),
        }
        bump_policy(&mut s);
    }

    /// Answer `harness policy` with this `ERR …` line instead (`None` restores it).
    pub fn answer_policy_with(&self, err: Option<&str>) {
        lock(&self.state).policy_err = err.map(str::to_string);
    }

    /// Set a raw (already escaped) `status` field.
    pub fn set_status(&self, key: &str, raw: &str) {
        let mut s = lock(&self.state);
        match s.status.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = raw.to_string(),
            None => {
                let at = s.status.len().saturating_sub(1);
                s.status.insert(at, (key.to_string(), raw.to_string()));
            }
        }
    }

    /// Who leads the session's foreground process group.
    pub fn set_foreground_leader(&self, pid: Option<u32>) {
        lock(&self.state).foreground_leader = pid;
    }

    /// Put a claim in place as another supervisor would (aterm's own host, a driver).
    pub fn claim_as(&self, holder: &str) {
        let mut s = lock(&self.state);
        s.set_claim(Some(FakeClaim {
            holder: holder.to_string(),
            conn: None,
            proven_pid: None,
        }));
    }

    /// `pid` is gone: a proven claim for it is cleared, as aterm does at its exit.
    pub fn kill_pid(&self, pid: u32) {
        let mut s = lock(&self.state);
        s.gone.insert(pid);
        if s.claim.as_ref().is_some_and(|c| c.proven_pid == Some(pid)) {
            s.set_claim(None);
        }
    }

    /// Never answer requests whose verb is `verb` (`status`, `harness`, `meta`, …).
    pub fn stall(&self, verb: &str) {
        lock(&self.state).stalled.insert(verb.to_string());
    }

    /// Hold every `verb` request read from now on, unanswered, until [`FakeAterm::open_gate`]
    /// (then it is answered as usual). Unlike a stall, nothing times out at aterm's side: the
    /// client's own deadline still runs.
    pub fn gate(&self, verb: &str) {
        lock(&self.state).gated.insert(verb.to_string());
    }

    /// Answer `verb` again, the held requests first, in order.
    pub fn open_gate(&self, verb: &str) {
        let mut s = lock(&self.state);
        s.gated.remove(verb);
        s.changed();
    }

    /// The requests that met a closed gate.
    pub fn gated_requests(&self) -> Vec<String> {
        lock(&self.state).gated_seen.clone()
    }

    /// Answer `verb` again (undoes [`FakeAterm::stall`] for new requests).
    pub fn unstall(&self, verb: &str) {
        lock(&self.state).stalled.remove(verb);
    }

    /// Hold back the next claim clear's `meta field=supervisor value=-` event and push it right
    /// after the event that follows it: an echo that reaches a subscriber late, behind a newer
    /// event (a word turned back on before aterm took the release).
    pub fn delay_clear_event(&self) {
        lock(&self.state).delay_clear = true;
    }

    /// Clear the claim as a person's bare `meta unset supervisor` does.
    pub fn clear_claim(&self) {
        lock(&self.state).set_claim(None);
    }

    /// How many subscribe streams are attached.
    pub fn subscribers(&self) -> usize {
        lock(&self.state).subscribers.len()
    }

    /// How many connections have been opened to the fake so far.
    pub fn connections(&self) -> u64 {
        lock(&self.state).next_conn
    }

    /// Wait until `done` holds of the fake's claim, at most a minute (a hang detector,
    /// never a latency budget). Returns whether it held.
    pub async fn wait_for(&self, done: impl Fn(&FakeAterm) -> bool) -> bool {
        let mut changes = self.changes.clone();
        let wait = async {
            loop {
                if done(self) {
                    return;
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(60), wait)
            .await
            .is_ok()
            && done(self)
    }
}

fn bump_policy(s: &mut State) {
    s.policy_gen += 1;
    if s.mode == Mode::Current {
        let event = format!("harness-policy gen={}", s.policy_gen);
        s.emit(&event);
    }
    s.changed();
}

fn write_private(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(body.as_bytes())
}

/// The verb a request line names, past its selector.
fn verb_of(line: &str) -> &str {
    let rest = match line.strip_prefix('@') {
        Some(r) => r.split_once(' ').map_or("", |(_, rest)| rest),
        None => line,
    };
    rest.split_ascii_whitespace().next().unwrap_or("")
}

async fn serve(stream: UnixStream, state: Arc<Mutex<State>>) {
    let peer_pid = stream
        .peer_cred()
        .ok()
        .and_then(|c| c.pid())
        .and_then(|p| u32::try_from(p).ok());
    let (conn, token) = {
        let mut s = lock(&state);
        s.next_conn += 1;
        (s.next_conn, s.token.clone())
    };
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);
    match read_line(&mut reader).await {
        Ok(Some(first)) if first == format!("AUTH {token}") => {}
        _ => {
            let _ = writer.write_all(b"ERR auth\n").await;
            return;
        }
    }
    while let Ok(Some(line)) = read_line(&mut reader).await {
        let verb = verb_of(&line).to_string();
        let mut gated = None;
        loop {
            let mut changes = {
                let mut s = lock(&state);
                if !s.gated.contains(&verb) {
                    break;
                }
                if gated.is_none() {
                    s.gated_seen.push(line.clone());
                    s.changed();
                }
                s.changes.subscribe()
            };
            gated = Some(());
            if changes.changed().await.is_err() {
                break;
            }
        }
        if lock(&state).stalled.contains(&verb) {
            lock(&state).requests.push(line);
            // Never answer; read on until the client hangs up.
            while let Ok(Some(_)) = read_line(&mut reader).await {}
            break;
        }
        if verb == "subscribe" {
            lock(&state).release_conn(conn);
            subscribe(&line, conn, &state, &mut reader, &mut writer).await;
            break;
        }
        if verb == "post" {
            // The body follows the line (`len=<n>`), as aterm's `post_frame_len` reads it.
            let options = line
                .split_once(" post ")
                .map_or("", |(_, rest)| rest)
                .to_string();
            let len = options
                .split_ascii_whitespace()
                .find_map(|t| t.strip_prefix("len="))
                .and_then(|n| n.parse::<usize>().ok());
            let Some(len) = len.filter(|n| *n <= 256 * 1024) else {
                let _ = writer
                    .write_all(b"ERR usage: post len=<n> must be a byte count\n")
                    .await;
                break;
            };
            let mut body = vec![0; len];
            if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body)
                .await
                .is_err()
            {
                break;
            }
            let reply = {
                let mut s = lock(&state);
                s.requests.push(line.clone());
                s.post(&options, body)
            };
            if writer.write_all(reply.as_bytes()).await.is_err() {
                break;
            }
            continue;
        }
        let reply = lock(&state).handle(&line, conn, peer_pid);
        if writer.write_all(reply.as_bytes()).await.is_err() {
            break;
        }
    }
    lock(&state).release_conn(conn);
}

async fn subscribe(
    line: &str,
    conn: u64,
    state: &Arc<Mutex<State>>,
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) {
    let (sid, ok, mail) = {
        let s = lock(state);
        let events = format!("subscribe @{} events", s.sid);
        let with_mail = format!("subscribe @{} events,mail", s.sid);
        let mail = line == with_mail && !s.refuse_mail_stream;
        (s.sid.clone(), line == events || mail, mail)
    };
    if !ok {
        lock(state).requests.push(line.to_string());
        let _ = writer.write_all(b"ERR no such session\n").await;
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    {
        let mut s = lock(state);
        s.requests.push(line.to_string());
        s.subscribers.push((conn, tx.clone()));
        if mail {
            s.mail_subscribers.push((conn, tx));
        }
        s.changed();
    }
    let ack = format!("OK subscribe 1\nsub 1 {sid}\n");
    if writer.write_all(ack.as_bytes()).await.is_ok() {
        loop {
            tokio::select! {
                frame = rx.recv() => match frame {
                    Some(frame) => {
                        if writer.write_all(frame.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                read = read_line(reader) => {
                    if !matches!(read, Ok(Some(_))) {
                        break;
                    }
                }
            }
        }
    }
    let mut s = lock(state);
    s.subscribers.retain(|(c, _)| *c != conn);
    s.mail_subscribers.retain(|(c, _)| *c != conn);
    s.changed();
}
