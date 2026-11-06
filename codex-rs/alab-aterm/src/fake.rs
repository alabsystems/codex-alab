// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An in-process FAKE aterm for tests: a control socket in a scratch directory that
//! speaks the wire this crate speaks, with CONTRACT §1-§3's rules.
//!
//! It lays the directory out as aterm does (`aterm-<pid>.sock`, `aterm-<pid>.token`,
//! `graph/<sid>`), so [`crate::locate::locate_in`] finds it. It serves `version`, `whoami`,
//! `@<sid> status|meta|harness policy|harness fact`, `meta set|unset
//! supervisor|attention|description|harness_act|harness_held|harness_land|harness_needs`,
//! `story`, `appnotice`, `notice later|done`, the digest's read `messages live need=later
//! tag=<tag>`, `sessions`, `subscribe @<sid> events[,mail]`, the terminal-context reads `panes`,
//! `blocks`, `blocktext` and `selection` (this session as `@<sid>` or its local id `@0`, and the
//! split panes [`FakeAterm::add_pane`] adds), and the inbox: `inbox [--peek] [--meta]`, `inbox
//! get <id>`, `inbox seen <id> [<verdict>]` and a length-framed `post … len=<n>` (mail arrives
//! by [`FakeAterm::deliver`], pushed as a `MAIL` frame).
//!
//! - **Proven claim** ([`Mode::Current`]): a `meta set supervisor codex-alab@<pid>` with
//!   no `ttl=` is proven only when the connection's kernel peer pid equals `<pid>` and
//!   `<pid>` is the foreground leader ([`FakeAterm::set_foreground_leader`]; by default
//!   this test process). A proven claim replaces an `aterm-harness@…` claim, or a proven
//!   claim whose pid is gone; any other live holder is `ERR busy supervisor=<holder>`.
//! - **Connection-bound**: a claim without `ttl=` ends when its connection closes or turns
//!   into a subscribe stream; a proven claim ends when its pid is gone.
//! - **The typing stop's arm** ([`Mode::Current`]): `meta set harness_held same-reply|no-work`
//!   and `meta unset harness_held` are taken from the proven claim only, as `harness_act` is
//!   (else `ERR denied`), and answered `OK` changed or not; the standing arm
//!   ([`FakeAterm::held`]) is `status held=` (`-` for none). It is the proven pid's, as aterm
//!   keys it by the claim's proof: it lapses when that claim ends (its release, a replacement
//!   by another, its connection's close or turn into a stream, its pid gone), and a re-claim by
//!   the same pid that reaches the fake first keeps it (a reconnect's claim taken before the
//!   old connection's close is read, [`FakeAterm::defer_closes`]; an image that exec'd before
//!   its release, [`FakeAterm::leave_held`]).
//! - **Land's hold** ([`Mode::Current`], gap 11): `meta set harness_land since=<ms> nudges=<n>
//!   [read=1 legs=<leg>[,<leg>…] [open=<w>[,<w>…]] [changed=<n>] [untracked=<n>] [stashes=<n>]
//!   [unpushed=<n>] [unlanded=<n>]]` and `meta unset harness_land`, read as aterm's
//!   `land_held_of` reads them (tokens in any order, each at most once; a leg word aterm's
//!   `HoldLeg` knows, the legs a set; ASCII digits, the counts saturating; `read=1` with legs
//!   and legs, open words and counts only with `read=1`; an empty open word dropped; else its
//!   usage) but for the open words' vocabulary, which it does not hold (it keeps every word,
//!   where aterm keeps only those `OpenWord::of` knows), taken from the proven claim only (else
//!   `ERR denied`) and answered `OK` changed or not. The standing hold ([`FakeAterm::land`]) is
//!   `status land_held=<legs> land_nudges=<n> land_since=<s>` (aterm's `land_fields`: `-` for
//!   legs not read, every field `-` for none) while the session's word reads `land`, the proven
//!   pid's as the typing stop's arm is: it lapses with that claim, and a same-pid re-claim that
//!   reaches the fake first keeps it.
//! - **The worker's needs whole** ([`Mode::Current`], gap 16): `meta set harness_needs
//!   entry=<pct text> need=<pct need> [need=<pct need> …]` and `meta unset harness_needs`, read
//!   on the wire's strict reader ([`parse_needs`]: `entry=` once and not empty, `need=` one to
//!   [`crate::NEEDS_TOLD`] times, each not empty and at most [`crate::NEED_TOLD_MAX_BYTES`]
//!   decoded, every value a whole percent escape of UTF-8, no other token; else its usage, with
//!   or without a claim), taken from the proven claim only (else `ERR denied`) and answered `OK`
//!   changed or not. It is told as aterm's own loop's set is (`ToldNeeds::tell`, beside the
//!   `owner=needs` entry standing then), and read as aterm's readers read it
//!   ([`FakeAterm::needs_told`]: the set only while the entry it describes stands, the set before
//!   it while that one's entry still does); the set told last is [`FakeAterm::needs_told_last`].
//!   It is the proven pid's, as the typing stop's arm is: it lapses with that claim, and a
//!   same-pid re-claim that reaches the fake first keeps it. Two readings it does not hold:
//!   aterm compares the entries through its presentation sanitizer, the fake as sent (the
//!   client sends the text it set); and it says no `status needs_told=`, which only a debug
//!   aterm says.
//! - **Autopilot** (gap 12 of the parity sweep, 2026-10-29): `meta set autopilot
//!   cruise|land|off [until=<when>]`, as aterm's owner-only mode control takes it from the
//!   session's own connection: `status autopilot=` takes the word and `status land_at=` the
//!   timer (`until=` with cruise only; the fake reads no clock, so any `<when>` arms one an
//!   hour from now), each change announced as aterm's `meta field=autopilot` event; answered
//!   `OK` (`OK land_at=<s> local=-` for a timer), else aterm's usage.
//! - **aterm's fact for a self-claim's act** ([`Mode::Current`], gap 23): `harness fact
//!   for=<rule> reader=<id> kinds=<k>[,<k>…] wall=<word|->` and `harness fact told
//!   episode=<016x>`, parsed as aterm's verb parses them (each key once, in any order; else the
//!   new `harness` usage), taken from the standing proven `codex-alab@<pid>` claim only (else
//!   `ERR denied`). A read answers the first `why=` that holds in aterm's order: `rule` (`for`
//!   is no `FACT_RIDES` act), `limit` (a limit's-place rule with `wall=usage|entitlement`),
//!   `kinds` (no `no-progress`), `none` (no call named), then `offered` (every untold episode
//!   leased to another (claim, reader) within the lease, [`FakeAterm::fact_lease`], 30 s by
//!   default) or `told`, and, for a candidate only, `off` (`effective=off`) or `facts-off` (the
//!   policy's `facts` not `true`); a candidate is leased to (the claim's pid, the reader) and
//!   answered `OK fact=<pct> kind=no-progress episode=<016x> as_of_ms=<ms> rider=<pct ' (aterm:
//!   <text>)'>`. A reader takes its own offer again. The ack tells an open, untold episode
//!   offered under the claim's pid by any reader (`OK`), else `ERR stale fact`. The calls are
//!   planted ([`FakeAterm::plant_fact`], [`FakeAterm::plant_fact_as_of`]) and closed
//!   ([`FakeAterm::close_fact`]); the fake keeps no jobs board, so it never answers `stale` of
//!   itself and serves a pinned reading as aterm judged it fresh at its read. A read may be
//!   answered as given ([`FakeAterm::fact_answer_once`]) or never
//!   ([`FakeAterm::stall_fact_reply`]), an ack refused ([`FakeAterm::refuse_fact_told`]); every
//!   read is kept ([`FakeAterm::fact_reads`]) and every told episode ([`FakeAterm::facts_told`]).
//! - **An aterm before the fact** ([`FakeAterm::support_harness_fact`] off): `harness fact …`
//!   answered with its older usage, `ERR usage: harness policy | recovery`.
//! - **Old aterm** ([`Mode::Old`]): no `harness policy` (`ERR unknown verb`), no `harness
//!   fact`, no `supervisor_proven=`, no `harness_act`, no `harness_held`, no `harness_land`, no
//!   `harness_needs`, no `notice later` (its `notice` usage, from before `later`); every claim
//!   is unproven.
//! - **An aterm before act deadlines** ([`FakeAterm::support_act_deadlines`] off): the
//!   unbounded `meta set harness_act <act>` only, `meta prepare harness_act` answered with the
//!   plain `meta` usage, as aterm 0.109.0 answers it. `version` answers `0.0.0-fake` until
//!   [`FakeAterm::set_version`] names a release.
//! - **An aterm before the typing stop's arm** ([`FakeAterm::support_harness_held`] off, or act
//!   deadlines off, or [`Mode::Old`]): `meta set|unset harness_held` answered as any field it
//!   does not know, `ERR unknown meta field (…)`. Its `status` says `held=-` with no verb (aterm
//!   0.143.0 to 0.145.0, whose `held=` reads its own loop's stop only), and no `held=` before act
//!   deadlines or in an old aterm.
//! - **An aterm before Land's hold** ([`FakeAterm::support_harness_land`] off, or act deadlines
//!   off, or [`Mode::Old`]): `meta set|unset harness_land` answered as any field it does not
//!   know, `ERR unknown meta field (…)`; its `status` says `land_held=- land_nudges=-
//!   land_since=-` with no verb (its fields read its own loop's Land only), and none before act
//!   deadlines or in an old aterm.
//! - **An aterm before the needs whole** ([`FakeAterm::support_harness_needs`] off, or act
//!   deadlines off, or [`Mode::Old`]): `meta set|unset harness_needs` answered as any field it
//!   does not know, `ERR unknown meta field (…)`; its readers read the entry's first need and its
//!   count only.
//! - **Digest** (`notice later`, [`Mode::Current`]): one entry per key, as aterm's
//!   `wire::apply_later` keeps it: `new`, then `duplicate` for the same words, `restated` for
//!   new ones, `quiet` once the owner dismissed it ([`FakeAterm::dismiss_later`]); at most
//!   four open entries. Every line taken is kept ([`FakeAterm::laters`]); a stalled `notice`
//!   ([`FakeAterm::stall`]) is never answered. `messages live need=later tag=<tag>` lists the
//!   open (not dismissed) entries of that tag as rows with `key=wire.<key>` and `need=later`.
//!   `notice done <key>` ends an open entry, and of a dismissed one, as aterm's
//!   `wire::apply_done` does, forgets the dismissal: the key's next `later` is `new`. Every
//!   `done` line is kept ([`FakeAterm::dones`]).
//!   Each open row's opaque `rev` confirms that entry: every accepted duplicate or
//!   restatement advances it, while another key does not. A saturated confirmation
//!   cannot settle. The semantic digest revision still changes only for content or
//!   membership changes. The fake retains its simpler changed-words classification;
//!   native text-only quiet coalescing is not modeled here.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::collections::VecDeque;
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

/// aterm's answer to `meta set|unset` of a field it does not know (`meta_unknown_field`, naming
/// its `MetaField::NAMES`), as aterm 0.109.0 through 0.145.0 answer `harness_held`.
const META_FIELDS: &str =
    "ERR unknown meta field (title|description|icon|role|attention|questions|autopilot)\n";
/// aterm's answer to a `meta set autopilot` it cannot take (`AUTOPILOT_USAGE`).
const AUTOPILOT_USAGE: &str =
    "ERR usage: meta set autopilot cruise [until=HH:MM|+DURATION]|land|off\n";
/// The fake rejects `open`: Codex owns its menus and needs no host popup.
const OPEN_USAGE: &str = "ERR usage: open <prefs | about | menu | tab-menu | autopilot-menu | \
                          fact-menu | overview | update> [close] | open fact-menu <fact> | \
                          open app <…>\n";
/// aterm's `meta` usage for a form its `meta` arm does not know (aterm 0.109.0's
/// `cmd_meta`, the last before `meta prepare harness_act`).
const OLD_META_USAGE: &str = "ERR usage: meta [set <field> <text...> | unset <field>]\n";
const HELD_USAGE: &str =
    "ERR usage: meta set harness_held same-reply|no-work | meta unset harness_held\n";
/// aterm's usage for a malformed `harness_land` line (gap 11's wire).
const LAND_USAGE: &str = "ERR usage: meta set harness_land since=<ms> nudges=<n> [read=1 legs=… \
                          open=… changed=… untracked=… stashes=… unpushed=… unlanded=…] | meta \
                          unset harness_land\n";
/// aterm's usage for a malformed `harness_needs` line (gap 16's wire).
const NEEDS_USAGE: &str = "ERR usage: meta set harness_needs entry=<pct text> need=<pct need> \
                           [need=<pct need> \u{2026}] | meta unset harness_needs\n";
/// This session's local id (`status` prints `sid=0`).
const SELF_LOCAL: &str = "0";

/// One set of the worker's needs told whole (aterm's `HarnessNeeds`): every need in the
/// worker's order, and the text of the `owner=needs` entry they were told with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeNeeds {
    /// The entry's text, as sent.
    pub entry: String,
    /// Every need, whole, in the worker's order.
    pub needs: Vec<String>,
}

impl FakeNeeds {
    /// The needs, where `standing` (the `owner=needs` entry stored now, `None` where none
    /// stands) is the text they were told with (aterm's `HarnessNeeds::words_for`, but for its
    /// sanitizer); else `None`.
    fn words_for(&self, standing: Option<&str>) -> Option<&[String]> {
        (standing?.trim() == self.entry).then_some(self.needs.as_slice())
    }
}

/// The worker's needs as aterm's window holds them (aterm's `ToldNeeds`): the set told last,
/// and the set that described the entry standing when that one was told.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FakeToldNeeds {
    current: Option<FakeNeeds>,
    outgoing: Option<FakeNeeds>,
}

impl FakeToldNeeds {
    /// Drop an outgoing set whose entry no longer stands (aterm's `ToldNeeds::settle`).
    fn settle(&mut self, standing: Option<&str>) {
        if self
            .outgoing
            .as_ref()
            .is_some_and(|held| held.words_for(standing).is_none())
        {
            self.outgoing = None;
        }
    }

    /// Tell `told` beside `standing` (aterm's `ToldNeeds::tell`): the set before keeps
    /// describing the entry it was told with, where the set told describes another.
    fn tell(&mut self, told: Option<FakeNeeds>, standing: Option<&str>) {
        self.settle(standing);
        if self.current == told {
            return;
        }
        let describes = |held: &FakeNeeds| held.words_for(standing).is_some();
        let outgoing = if told.as_ref().is_some_and(describes) {
            None
        } else {
            [self.current.take(), self.outgoing.take()]
                .into_iter()
                .flatten()
                .find(describes)
        };
        self.current = told;
        self.outgoing = outgoing;
    }

    /// The needs a reader reads beside `standing` (aterm's `ToldNeeds::words_for`).
    fn words_for(&self, standing: Option<&str>) -> Option<&[String]> {
        [&self.current, &self.outgoing]
            .into_iter()
            .flatten()
            .find_map(|held| held.words_for(standing))
    }
}

/// `meta set harness_needs`'s tokens read as the wire says (gap 16, aterm d749fecec5): `entry=`
/// exactly once and not empty once sanitized, `need=` one to [`crate::NEEDS_TOLD`] times in the
/// worker's order, each not empty once folded and at most [`crate::NEED_TOLD_MAX_BYTES`]
/// decoded, every value a whole percent escape of UTF-8 ([`crate::wire::pct_decode_strict`]),
/// and no other token. `None`: its usage.
fn parse_needs(words: &[&str]) -> Option<FakeNeeds> {
    let mut entry = None;
    let mut needs = Vec::new();
    for word in words {
        let (key, value) = word.split_once('=')?;
        let value = crate::wire::pct_decode_strict(value)
            .filter(|value| !crate::wire::single_line(value, usize::MAX).is_empty())?;
        match key {
            "entry" if entry.is_none() => entry = Some(value),
            "need"
                if value.len() <= crate::NEED_TOLD_MAX_BYTES && needs.len() < crate::NEEDS_TOLD =>
            {
                needs.push(value);
            }
            _ => return None,
        }
    }
    if needs.is_empty() {
        return None;
    }
    Some(FakeNeeds {
        entry: entry?,
        needs,
    })
}

fn lines_reply(lines: Option<&Vec<String>>) -> String {
    let lines = lines.map(Vec::as_slice).unwrap_or_default();
    let mut out = format!("OK {}\n", lines.len());
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `meta set harness_land`'s tokens read as aterm's `land_held_of` reads them (gap 11's wire),
/// on its own reader rather than the client's: in any order, each at most once; `since=` ASCII
/// digits within aterm's `i64`, `nudges=` ASCII digits saturating at `u32`; `read=1` only with
/// `legs=`, whose words are aterm's `HoldLeg` words, a set named in [`crate::LandLeg::ALL`]'s
/// order (aterm keeps a bitset), and `legs=`, `open=` and the counts only with `read=1`; each
/// open word escaped, an empty one dropped, the first [`crate::LAND_OPEN_WORDS`] kept; a count
/// ASCII digits, saturating at `u16`. `None`: its usage. ONE READING IT DOES NOT HOLD: aterm
/// keeps only the open words its strict reading knows (`OpenWord::of`), a vocabulary this fake
/// does not have, so it keeps every word that is not empty, and a word aterm would drop shows
/// here (review of gap 11, finding 6).
fn parse_land(words: &[&str]) -> Option<crate::LandReport> {
    let mut seen = HashSet::new();
    let (mut since, mut nudges, mut read) = (None, None, false);
    let mut counted = false;
    let mut land = crate::LandRead::default();
    let mut legs = None;
    for word in words {
        let (key, value) = word.split_once('=')?;
        if !seen.insert(key) {
            return None;
        }
        let digits = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
        // Past `u64` a count is held at its field's largest value, as aterm's is.
        let number = || digits.then(|| value.parse::<u64>().unwrap_or(u64::MAX));
        let count = || number().map(|n| u16::try_from(n).unwrap_or(u16::MAX));
        match key {
            "since" => {
                since = Some(
                    value
                        .parse::<i64>()
                        .ok()
                        .filter(|_| digits)
                        .and_then(|ms| u64::try_from(ms).ok())?,
                );
            }
            "nudges" => nudges = Some(number().map(|n| u32::try_from(n).unwrap_or(u32::MAX))?),
            "read" if value == "1" => read = true,
            "legs" => {
                let named = value
                    .split(',')
                    .map(crate::LandLeg::of_word)
                    .collect::<Option<Vec<_>>>()?;
                legs = Some(
                    crate::LandLeg::ALL
                        .into_iter()
                        .filter(|leg| named.contains(leg))
                        .collect(),
                );
            }
            "open" => {
                land.open = value
                    .split(',')
                    .map(crate::wire::pct_decode)
                    .filter(|w| !w.is_empty())
                    .take(crate::LAND_OPEN_WORDS)
                    .collect();
                counted = true;
            }
            "changed" | "untracked" | "stashes" | "unpushed" | "unlanded" => {
                let n = count()?;
                *match key {
                    "changed" => &mut land.changed,
                    "untracked" => &mut land.untracked,
                    "stashes" => &mut land.stashes,
                    "unpushed" => &mut land.unpushed,
                    _ => &mut land.unlanded,
                } = n;
                counted = true;
            }
            _ => return None,
        }
    }
    let read = match (read, legs) {
        (true, Some(legs)) => {
            land.legs = legs;
            Some(land)
        }
        (false, None) if !counted => None,
        _ => return None,
    };
    Some(crate::LandReport {
        since_ms: since?,
        nudges: nudges?,
        read,
    })
}

/// `status`'s Land hold fields as aterm's `land_fields` writes them: the legs read (`-` where
/// none were), the nudges and `since` in seconds; every field `-` for none, and for a hold the
/// tab's face does not show ([`State::land_shown`]).
fn land_fields(land: Option<&crate::LandReport>) -> String {
    let Some(land) = land else {
        return " land_held=- land_nudges=- land_since=-".to_string();
    };
    let legs = land.read.as_ref().map_or_else(
        || "-".to_string(),
        |read| {
            let words: Vec<&str> = read.legs.iter().map(|leg| leg.word()).collect();
            words.join(",")
        },
    );
    format!(
        " land_held={legs} land_nudges={} land_since={}",
        land.nudges,
        land.since_ms / 1000
    )
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

/// aterm's `notice` usage before `later` (aterm-messages `NOTICE_USAGE` until f17fb9bcaf).
const OLD_NOTICE_USAGE: &str = "ERR usage: notice <post|progress|done|ask|dismiss|act> \u{2026}\n";
const NOTICE_USAGE: &str =
    "ERR usage: notice <post|progress|done|ask|later|dismiss|act> \u{2026}\n";
const LATER_USAGE: &str = "ERR usage: notice later <key> [tag=<tag>] [url=<https-url>] <title> -- <reason> -- <step>[ -- <detail>]\n";
const DONE_USAGE: &str = "ERR usage: notice done <key> [ok|warn|withdraw] [<words>]\n";
const KEY_REFUSAL: &str = "ERR notice: keys are 1-40 of a-z 0-9 . _ - (they live as wire.<key>)\n";
const MESSAGES_USAGE: &str =
    "ERR usage: messages [<n>] [since=<id>] [tag=<tag>] [sev=<sev>] [live]\n";

/// A wire key aterm takes (`wire::wire_key`): `[a-z0-9._-]{1,40}`.
fn wire_key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 40
        && key.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// aterm's `WIRE_LATER_CAP`: open `wire.*` entries of the digest.
const LATER_CAP: usize = 4;

/// aterm's `harness` usage before `harness fact` (its `harness_recovery_verb::USAGE`): what an
/// aterm before the verb answers it ([`FakeAterm::support_harness_fact`] off).
const OLD_HARNESS_USAGE: &str = "ERR usage: harness policy | recovery\n";
/// aterm's `harness` usage with `harness fact` (gap 23's wire): what a malformed fact line gets.
const HARNESS_USAGE: &str = "ERR usage: harness policy | recovery | fact for=<rule> reader=<id> \
                             kinds=<k> wall=<w> | fact told episode=<hex>\n";
/// aterm's `policy::turn_end::FACT_RIDES`: the acts a fact may ride.
const FACT_RIDES: [&str; 4] = [
    "continue@v1",
    "continue-done-check@v1",
    "land@v1",
    "land-nudge@v1",
];
/// aterm's `FactKind::word`s: the kinds a `kinds=` may name.
const FACT_KINDS: [&str; 4] = ["no-progress", "program-check", "hold", "wake"];
/// How long an offered episode is leased to its reader by default (aterm's `OFFER_MS`).
const FACT_LEASE: Duration = Duration::from_secs(30);

/// aterm's limit's-place rules among [`FACT_RIDES`] (`facts_offer::limits_place_rule`: the done
/// check and the land acts; the go-ahead and the answers ride no fact anyway).
fn limits_place_rule(rule: &str) -> bool {
    matches!(rule, "continue-done-check@v1" | "land@v1" | "land-nudge@v1")
}

/// One no-progress call the fake names ([`FakeAterm::plant_fact`]).
#[derive(Debug, Clone)]
struct FakeFact {
    text: String,
    /// The reading's time pinned ([`FakeAterm::plant_fact_as_of`]); `None`: each read's own.
    as_of_ms: Option<i64>,
}

/// An episode offered to one reader of one proven claim (aterm's `Offer`): the claim's proof
/// is its pid here.
#[derive(Debug, Clone)]
struct FakeOffer {
    pid: u32,
    reader: String,
    at: tokio::time::Instant,
}

/// aterm's facts account for the session's proven self-claim (its `SelfClaimFacts`, gap 23):
/// the calls it names, the episodes told, each offer leased to its (claim, reader).
#[derive(Debug)]
struct FakeFacts {
    /// Whether this aterm has `harness fact` ([`FakeAterm::support_harness_fact`]).
    verb: bool,
    lease: Duration,
    next: u64,
    /// The calls named now, by episode, in the order planted (the fake's priority).
    open: BTreeMap<u64, FakeFact>,
    told: BTreeSet<u64>,
    offers: BTreeMap<u64, FakeOffer>,
    /// Each episode an ack told, in order.
    told_log: Vec<u64>,
    /// The next reads' answers, as sent ([`FakeAterm::fact_answer_once`]).
    answer_once: VecDeque<String>,
    refuse_told: bool,
    /// Reads are never answered ([`FakeAterm::stall_fact_reply`]).
    stall: bool,
    /// Rules this aterm's own loop lets no fact ride, answered `why=rule`
    /// ([`FakeAterm::narrow_fact_rides`]).
    narrowed: BTreeSet<String>,
}

impl Default for FakeFacts {
    fn default() -> Self {
        FakeFacts {
            verb: true,
            lease: FACT_LEASE,
            next: 0,
            open: BTreeMap::new(),
            told: BTreeSet::new(),
            offers: BTreeMap::new(),
            told_log: Vec::new(),
            answer_once: VecDeque::new(),
            refuse_told: false,
            stall: false,
            narrowed: BTreeSet::new(),
        }
    }
}

/// A `harness fact for=…` line's (for, reader), where `line` is one (any selector).
fn fact_read_of(line: &str) -> Option<(String, String)> {
    let rest = line
        .strip_prefix('@')
        .and_then(|r| r.split_once(' '))
        .map_or(line, |(_, rest)| rest);
    let words = rest.strip_prefix("harness fact ")?;
    let mut rule = None;
    let mut reader = None;
    for word in words.split_ascii_whitespace() {
        match word.split_once('=') {
            Some(("for", v)) => rule = Some(v.to_string()),
            Some(("reader", v)) => reader = Some(v.to_string()),
            _ => {}
        }
    }
    Some((rule?, reader?))
}

/// A read's keys as aterm's verb parses them: `for`, `reader`, `kinds` and `wall`, each exactly
/// once in any order, `for` a rule id, `reader` a reader token, every kind a known word, `wall`
/// a word or `-`; `None`: the usage.
fn parse_fact_read(words: &[&str]) -> Option<(String, String, Vec<String>, String)> {
    let mut keys: BTreeMap<&str, &str> = BTreeMap::new();
    for word in words {
        let (key, value) = word.split_once('=')?;
        if !matches!(key, "for" | "reader" | "kinds" | "wall") || keys.insert(key, value).is_some()
        {
            return None;
        }
    }
    let rule = keys
        .get("for")
        .copied()
        .filter(|r| crate::facts::valid_rule(r))?;
    let reader = keys
        .get("reader")
        .copied()
        .filter(|r| crate::facts::valid_reader(r))?;
    let kinds: Vec<String> = keys.get("kinds")?.split(',').map(str::to_string).collect();
    if !kinds.iter().all(|k| FACT_KINDS.contains(&k.as_str())) {
        return None;
    }
    let wall = keys
        .get("wall")
        .copied()
        .filter(|w| crate::facts::valid_wall(w))?;
    Some((
        rule.to_string(),
        reader.to_string(),
        kinds,
        wall.to_string(),
    ))
}

const SETTLE_USAGE: &str = "ERR usage: notice settle <key> <id> <rev>\n";

/// One `notice later` line the fake took, and how it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeLater {
    /// The key as sent (aterm keeps it as `wire.<key>`).
    pub key: String,
    pub tag: Option<String>,
    pub url: Option<String>,
    pub title: String,
    pub reason: String,
    pub step: String,
    /// The detail, split on its literal `\n`.
    pub detail: Vec<String>,
    /// `new`, `restated`, `duplicate` or `quiet`.
    pub how: String,
}

/// The digest's entry for one key.
#[derive(Debug, Clone)]
struct FakeEntry {
    id: u64,
    confirmation: u64,
    tag: Option<String>,
    url: Option<String>,
    title: String,
    words: String,
    dismissed: bool,
}

struct State {
    mode: Mode,
    /// The release `version` answers ([`FakeAterm::set_version`]).
    version: String,
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
    /// Answer every `status` with this `ERR …` line ([`FakeAterm::answer_status_with`]).
    status_err: Option<String>,
    attention: BTreeMap<String, String>,
    /// The `kind=` each attention entry was set with, where one was (`ask`, `wait`).
    attention_kind: BTreeMap<String, String>,
    /// The owners of the attention entries, the most recently set last (aterm shows that one).
    attention_order: Vec<String>,
    description: Option<String>,
    descriptions: Vec<String>,
    acts: Vec<String>,
    refuse_next_act: Option<String>,
    refuse_acts: Option<String>,
    act_clock: tokio::time::Instant,
    act_deadlines: bool,
    /// Whether this aterm has `harness_held` ([`FakeAterm::support_harness_held`]).
    held_verb: bool,
    /// The typing stop's arm a proven claim set (`status held=`), with the pid that claim was
    /// proven for: aterm keys it by the claim's proof, so it lapses with that claim and a
    /// re-claim by the same pid keeps it.
    held: Option<(u32, String)>,
    /// Whether this aterm has `harness_land` ([`FakeAterm::support_harness_land`]).
    land_verb: bool,
    /// Land's hold a proven claim set, with the pid that claim was proven for: keyed by the
    /// claim's proof as [`State::held`] is.
    land: Option<(u32, crate::LandReport)>,
    /// Whether this aterm has `harness_needs` ([`FakeAterm::support_harness_needs`]).
    needs_verb: bool,
    /// The worker's needs a proven claim told whole, with the pid that claim was proven for:
    /// keyed by the claim's proof as [`State::held`] is, and kept apart from any set aterm's own
    /// loop tells, as aterm keeps a self-claim's.
    told_needs: Option<(u32, FakeToldNeeds)>,
    /// aterm's facts account for the proven self-claim ([`FakeFacts`], gap 23).
    facts: FakeFacts,
    stories: Vec<(String, String)>,
    notices: Vec<String>,
    laters: Vec<FakeLater>,
    dones: Vec<String>,
    settles: Vec<String>,
    digest: BTreeMap<String, FakeEntry>,
    digest_revision: u64,
    reply_once: BTreeMap<String, String>,
    /// This session's task record's state (`working`, `done`, `stalled`, `exited`), as
    /// `@self task` answers it; `None` answers `OK task -` (no record).
    task: Option<String>,
    next_later: u64,
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
    /// Connections still to refuse at admission, as aterm's control server does when every
    /// request lane is taken ([`FakeAterm::busy_next`]).
    busy_next: u32,
    /// Verbs whose requests wait, read but unanswered, until the gate opens.
    gated: HashSet<String>,
    /// Requests that met a closed gate, in arrival order.
    gated_seen: Vec<String>,
    /// Command prefixes whose next applied request loses its reply and connection.
    drop_replies: HashSet<String>,
    /// The connections whose close goes unread while closes are deferred
    /// ([`FakeAterm::defer_closes`]; `None`: read at once).
    deferred_closes: Option<Vec<u64>>,
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
        // The typing stop's arm is the proven claimant's, as aterm's `SessionMeta::set_supervisor`
        // keys it by the claim's proof: it lapses when that claim ends or another replaces it,
        // and a re-claim by the same pid keeps it (its renewal on its own connection, a
        // reconnect's claim taken before the old connection's close).
        let proven = claim.as_ref().and_then(|c| c.proven_pid);
        if self
            .held
            .as_ref()
            .is_some_and(|(pid, _)| proven != Some(*pid))
        {
            self.held = None;
        }
        // Land's hold and the needs told whole are the proven claimant's by the same rule.
        if self
            .land
            .as_ref()
            .is_some_and(|(pid, _)| proven != Some(*pid))
        {
            self.land = None;
        }
        if self
            .told_needs
            .as_ref()
            .is_some_and(|(pid, _)| proven != Some(*pid))
        {
            self.told_needs = None;
        }
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
    /// is announced; a write of `owner=needs` settles an outgoing set (`SessionMeta::settle_self_needs`).
    fn put_attention(&mut self, owner: &str, text: &str) {
        if self.attention.get(owner).map(String::as_str) == Some(text) {
            if owner == crate::NEEDS_OWNER {
                self.settle_told_needs();
            }
            return;
        }
        let before = self.shown_attention();
        self.attention.insert(owner.to_string(), text.to_string());
        self.attention_order.retain(|o| o != owner);
        self.attention_order.push(owner.to_string());
        if owner == crate::NEEDS_OWNER {
            self.settle_told_needs();
        }
        self.announce_attention(before);
        self.changed();
    }

    /// Settle the self-claim's told needs against the `owner=needs` entry standing now
    /// (aterm's `SessionMeta::settle_self_needs`): an outgoing set goes when its entry goes or
    /// is replaced.
    fn settle_told_needs(&mut self) {
        let standing = self.attention.get(crate::NEEDS_OWNER).map(String::as_str);
        if let Some((_, held)) = &mut self.told_needs {
            held.settle(standing);
            if *held == FakeToldNeeds::default() {
                self.told_needs = None;
            }
        }
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

    /// Connection `conn` closed: its claim ends now, or once closes are read again where they
    /// are deferred ([`FakeAterm::defer_closes`]).
    fn close_conn(&mut self, conn: u64) {
        match &mut self.deferred_closes {
            Some(deferred) => deferred.push(conn),
            None => self.release_conn(conn),
        }
    }

    fn handle(&mut self, line: &str, conn: u64, peer_pid: Option<u32>) -> String {
        self.requests.push(line.to_string());
        self.changed();
        if let Some(reply) = self.reply_once.remove(line) {
            return reply;
        }
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
            "version" => format!("OK version={}\n", self.version),
            "whoami" => format!("OK {} 0123456789abcdef0123456789abcdef owner\n", self.sid),
            "status" => self.status_line(),
            "meta" => self.meta(tail, conn, peer_pid),
            "harness" if tail.trim() == "policy" && self.mode == Mode::Current => {
                self.policy_line()
            }
            "harness"
                if self.mode == Mode::Current
                    && tail.split_ascii_whitespace().next() == Some("fact") =>
            {
                self.fact(tail, conn, peer_pid)
            }
            // aterm's `cmd_appnotice`: a text past its 512 bytes is refused, never shortened.
            "appnotice" => match tail.trim().split_once(char::is_whitespace) {
                Some(("harness" | "toolchain" | "update", text)) if !text.trim().is_empty() => {
                    let text = text.trim();
                    if text.len() > crate::client::NOTICE_MAX_BYTES {
                        format!(
                            "ERR appnotice: text is longer than {} bytes\n",
                            crate::client::NOTICE_MAX_BYTES
                        )
                    } else {
                        self.notices.push(text.to_string());
                        "OK recorded\n".to_string()
                    }
                }
                _ => "ERR usage: appnotice <toolchain|update|harness> <text>\n".to_string(),
            },
            "story" if self.mode == Mode::Current => self.story(tail),
            "open" => OPEN_USAGE.to_string(),
            "notice" if self.mode == Mode::Old => OLD_NOTICE_USAGE.to_string(),
            "notice" => match tail.split_once(' ').unwrap_or((tail, "")) {
                ("later", rest) => self.later(rest),
                ("done", rest) => self.done(rest),
                ("settle", rest) => self.settle(rest),
                _ => NOTICE_USAGE.to_string(),
            },
            "messages" => self.messages(tail),
            "inbox" => self.inbox(tail),
            "sessions" => format!("OK 1\n{} program=codex\n", self.sid),
            "task" if tail.trim().is_empty() => match &self.task {
                Some(state) => format!(
                    "OK task rev=1 state={state} why=- result_bytes=0 result_path=- agent=idle \
                     program_state=- attention=-\n"
                ),
                None => "OK task -\n".to_string(),
            },
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

    /// `meta set autopilot <words>`: the session's word (`cruise`, `land` or `off`) and, with
    /// cruise only, one `until=<when>`, as aterm's `meta_set_autopilot_until` takes them; the
    /// word and the timer go into `status` and the move is announced, as aterm records it.
    fn set_autopilot(&mut self, words: &[&str]) -> String {
        let (until, word): (Vec<&str>, Vec<&str>) =
            words.iter().partition(|word| word.starts_with("until="));
        let word = match word.as_slice() {
            [word @ ("cruise" | "land" | "off")] => *word,
            _ => return AUTOPILOT_USAGE.to_string(),
        };
        let land_at = match until.as_slice() {
            [] => None,
            [when] if word == "cruise" && when.len() > "until=".len() => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs());
                Some(now + 3_600)
            }
            _ => return AUTOPILOT_USAGE.to_string(),
        };
        for (key, value) in [
            ("autopilot", word.to_string()),
            (
                "land_at",
                land_at.map_or_else(|| "-".to_string(), |at| at.to_string()),
            ),
        ] {
            match self.status.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => self.status.push((key.to_string(), value)),
            }
        }
        self.emit(&format!("meta field=autopilot value={word}"));
        self.changed();
        match land_at {
            Some(at) => format!("OK land_at={at} local=-\n"),
            None => "OK\n".to_string(),
        }
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

    /// aterm's `notice later` (`wire::LaterRequest::parse`, then `apply_later`), its word
    /// rules aside: the key, `tag=` and `url=` options, three ` -- `-split fields within
    /// their caps and a detail of at most 3 lines of 240, then the digest's answer.
    fn later(&mut self, rest: &str) -> String {
        let rest = rest.trim_start();
        let (key, mut rest) = rest.split_once(' ').unwrap_or((rest, ""));
        if !wire_key_ok(key) {
            return KEY_REFUSAL.to_string();
        }
        let (mut tag, mut url) = (None, None);
        loop {
            rest = rest.trim_start();
            let word = rest.split_whitespace().next().unwrap_or("");
            if let Some(t) = word.strip_prefix("tag=") {
                tag = Some(t.to_string());
            } else if let Some(u) = word.strip_prefix("url=") {
                if !u.starts_with("https://") || url.replace(u.to_string()).is_some() {
                    return LATER_USAGE.to_string();
                }
            } else {
                break;
            }
            rest = &rest[word.len()..];
        }
        let fields: Vec<&str> = rest.splitn(4, " -- ").map(str::trim).collect();
        let [title, reason, step, detail @ ..] = fields.as_slice() else {
            return LATER_USAGE.to_string();
        };
        if title.is_empty() || reason.is_empty() || step.is_empty() {
            return LATER_USAGE.to_string();
        }
        if reason.chars().count() > 90 {
            return "ERR notice: reason over 90 characters\n".to_string();
        }
        if step.chars().count() > 90 {
            return "ERR notice: step over 90 characters\n".to_string();
        }
        let detail: Vec<String> = detail
            .first()
            .map(|d| {
                d.split("\\n")
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if detail.len() > 3 || detail.iter().any(|l| l.chars().count() > 240) {
            return "ERR notice: detail over 3 lines (split by \\n) of 240 characters\n"
                .to_string();
        }
        let words = rest.to_string();
        let open = self.digest.values().filter(|e| !e.dismissed).count();
        let (id, how) = match self.digest.get_mut(key) {
            Some(entry) if entry.dismissed => (entry.id, "quiet"),
            Some(entry) if entry.words == words && entry.tag == tag && entry.url == url => {
                entry.confirmation = entry.confirmation.saturating_add(1);
                (entry.id, "duplicate")
            }
            Some(entry) => {
                entry.confirmation = entry.confirmation.saturating_add(1);
                entry.words = words;
                entry.title = title.to_string();
                entry.tag = tag.clone();
                entry.url = url.clone();
                self.digest_revision += 1;
                (entry.id, "restated")
            }
            None if open >= LATER_CAP => {
                return format!(
                    "ERR notice: {LATER_CAP} wire entries are open for later; end one with notice done\n"
                );
            }
            None => {
                self.next_later += 1;
                self.digest_revision += 1;
                let id = self.next_later;
                self.digest.insert(
                    key.to_string(),
                    FakeEntry {
                        id,
                        confirmation: 1,
                        tag: tag.clone(),
                        url: url.clone(),
                        title: title.to_string(),
                        words,
                        dismissed: false,
                    },
                );
                (id, "new")
            }
        };
        self.laters.push(FakeLater {
            key: key.to_string(),
            tag,
            url,
            title: title.to_string(),
            reason: reason.to_string(),
            step: step.to_string(),
            detail,
            how: how.to_string(),
        });
        self.changed();
        format!("OK later={id} how={how}\n")
    }

    /// aterm's `notice done <key> [ok|warn|withdraw] [<words>]` (`wire::apply_done`) on the
    /// digest: an open entry ends (the key's next `later` is news); of a dismissed one aterm
    /// forgets the dismissal (`forget_filing`) and answers `how=gone`, as for no entry.
    fn done(&mut self, rest: &str) -> String {
        let mut toks = rest.split_whitespace();
        let key = toks.next().unwrap_or("");
        if key.is_empty() {
            return DONE_USAGE.to_string();
        }
        if !wire_key_ok(key) {
            return KEY_REFUSAL.to_string();
        }
        let how = match toks.next() {
            None | Some("ok") => "resolved-ok",
            Some("warn") => "resolved-warn",
            Some("withdraw") => "withdrawn",
            Some(_) => return DONE_USAGE.to_string(),
        };
        self.dones.push(rest.trim().to_string());
        self.changed();
        match self.digest.remove(key) {
            Some(entry) if !entry.dismissed => {
                self.digest_revision += 1;
                format!("OK done={} how={how}\n", entry.id)
            }
            _ => "OK done=- how=gone\n".to_string(),
        }
    }

    /// The native atomic settle never clears a dismissal or ends a replacement entry.
    fn settle(&mut self, rest: &str) -> String {
        let words: Vec<&str> = rest.split_ascii_whitespace().collect();
        let [key, id, revision] = words.as_slice() else {
            return SETTLE_USAGE.to_string();
        };
        if !wire_key_ok(key) {
            return KEY_REFUSAL.to_string();
        }
        let number = |value: &str| {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                value.parse::<u64>().ok()
            }
        };
        let (Some(id), Some(revision)) = (number(id).filter(|id| *id > 0), number(revision)) else {
            return SETTLE_USAGE.to_string();
        };
        self.settles.push(rest.to_string());
        if revision == u64::MAX
            || !self.digest.get(*key).is_some_and(|entry| {
                entry.id == id && entry.confirmation == revision && !entry.dismissed
            })
        {
            return "OK settled=- how=gone\n".to_string();
        }
        self.digest.remove(*key);
        self.digest_revision += 1;
        self.changed();
        format!("OK settled={id} how=resolved-ok\n")
    }

    /// aterm's `messages`, in the one form a tool reads its own open digest entries with,
    /// `live need=later tag=<tag>` (any order): a row per open entry of that tag, in
    /// `wire::message_row`'s grammar. Any other form is aterm's usage.
    fn messages(&self, tail: &str) -> String {
        let mut words: Vec<&str> = tail.split_whitespace().collect();
        words.sort_unstable();
        let tag = match words.as_slice() {
            ["live", "need=later", tag] => tag.strip_prefix("tag="),
            _ => None,
        };
        let Some(tag) = tag else {
            return MESSAGES_USAGE.to_string();
        };
        let rows: Vec<String> = self
            .digest
            .iter()
            .filter(|(_, e)| !e.dismissed && e.tag.as_deref().unwrap_or("script") == tag)
            .map(|(key, e)| {
                format!(
                    "message {} at=0 ago_ms=0 tag={tag} sev=info origin=wire state=review glass=- \
                     rep=1 key=wire.{key} title={} detail=- actions=- need=later rev={}",
                    e.id,
                    pct_encode(&e.title),
                    e.confirmation
                )
            })
            .collect();
        if rows.is_empty() {
            return "OK 0\n".to_string();
        }
        format!("OK {}\n{}\n", rows.len(), rows.join("\n"))
    }

    fn status_line(&self) -> String {
        if let Some(err) = &self.status_err {
            return format!("{err}\n");
        }
        let mut line = String::from("OK");
        for (k, v) in &self.status {
            if k == "hash" {
                self.push_claim_fields(&mut line);
            }
            line.push_str(&format!(" {k}={v}"));
            if k == "hold" && self.held_field() {
                let held = self.held.as_ref().map_or("-", |(_, arm)| arm.as_str());
                line.push_str(&format!(" held={held}"));
                line.push_str(&land_fields(self.land_shown()));
            }
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

    /// Whether `status` says `held=`: aterm's own loop's stop from aterm 0.143.0 (00c9da783a)
    /// on, which the fake stands for wherever the proven claim's act deadlines are.
    fn held_field(&self) -> bool {
        self.mode == Mode::Current && self.act_deadlines
    }

    /// Whether this aterm has `harness_held`: only one with the proven claim's act deadlines.
    fn held_supported(&self) -> bool {
        self.held_field() && self.held_verb
    }

    /// Whether this aterm has `harness_land`: only one with the proven claim's act deadlines.
    fn land_supported(&self) -> bool {
        self.held_field() && self.land_verb
    }

    /// Whether this aterm has `harness_needs`: only one with the proven claim's act deadlines.
    fn needs_supported(&self) -> bool {
        self.held_field() && self.needs_verb
    }

    /// `meta set harness_needs <tokens>` (`set`) or `meta unset harness_needs`, `words` the
    /// line's words after the field: read as the wire says ([`parse_needs`]), the proven
    /// caller's only, as `harness_held` is, and told beside the `owner=needs` entry standing
    /// now, as aterm tells its own loop's set ([`FakeToldNeeds::tell`]).
    fn set_needs(&mut self, set: bool, words: &[&str], conn: u64, peer_pid: Option<u32>) -> String {
        let told = match (set, words) {
            (true, words) => match parse_needs(words) {
                Some(told) => Some(told),
                None => return NEEDS_USAGE.to_string(),
            },
            (false, []) => None,
            (false, _) => return NEEDS_USAGE.to_string(),
        };
        let proven_by_caller = self.claim.as_ref().and_then(|c| {
            c.proven_pid
                .filter(|pid| c.conn == Some(conn) || Some(*pid) == peer_pid)
        });
        let Some(pid) = proven_by_caller else {
            return "ERR denied\n".to_string();
        };
        let standing = self.attention.get(crate::NEEDS_OWNER).cloned();
        let mut held = match self.told_needs.take() {
            Some((kept, held)) if kept == pid => held,
            _ => FakeToldNeeds::default(),
        };
        held.tell(told, standing.as_deref());
        self.told_needs = (held != FakeToldNeeds::default()).then_some((pid, held));
        self.changed();
        "OK\n".to_string()
    }

    /// Land's hold as `status` says it: the one kept ([`State::land`]) only while the session's
    /// Autopilot word reads `land` (aterm's `shown_hold` says it only under the `⤓ land` face).
    /// The face's ends under that word (a landing's, a person's own exchange's) are not read
    /// here: codex-alab takes its hold down at both itself.
    fn land_shown(&self) -> Option<&crate::LandReport> {
        let land_word = self
            .status
            .iter()
            .any(|(key, word)| key == "autopilot" && word == "land");
        self.land
            .as_ref()
            .map(|(_, land)| land)
            .filter(|_| land_word)
    }

    /// `meta set harness_land <tokens>` (`set`) or `meta unset harness_land`, `words` the
    /// line's words after the field: read as the wire says ([`parse_land`]), the proven
    /// caller's only, as `harness_held` is.
    fn set_land(&mut self, set: bool, words: &[&str], conn: u64, peer_pid: Option<u32>) -> String {
        let land = match (set, words) {
            (true, words) => match parse_land(words) {
                Some(land) => Some(land),
                None => return LAND_USAGE.to_string(),
            },
            (false, []) => None,
            (false, _) => return LAND_USAGE.to_string(),
        };
        let proven_by_caller = self.claim.as_ref().and_then(|c| {
            c.proven_pid
                .filter(|pid| c.conn == Some(conn) || Some(*pid) == peer_pid)
        });
        let Some(pid) = proven_by_caller else {
            return "ERR denied\n".to_string();
        };
        self.land = land.map(|land| (pid, land));
        self.changed();
        "OK\n".to_string()
    }

    /// `meta set harness_held <arm>` (`set`) or `meta unset harness_held`, `words` the line's
    /// words after the field: the proven caller's only, as `harness_act` is.
    fn set_held(&mut self, set: bool, words: &[&str], conn: u64, peer_pid: Option<u32>) -> String {
        let well_formed = match words {
            [word] => set && crate::HELD_ARMS.contains(word),
            [] => !set,
            _ => false,
        };
        if !well_formed {
            return HELD_USAGE.to_string();
        }
        let proven_by_caller = self.claim.as_ref().and_then(|c| {
            c.proven_pid
                .filter(|pid| c.conn == Some(conn) || Some(*pid) == peer_pid)
        });
        let Some(pid) = proven_by_caller else {
            return "ERR denied\n".to_string();
        };
        self.held = words.first().map(|word| (pid, (*word).to_string()));
        self.changed();
        "OK\n".to_string()
    }

    /// `harness fact …` (gap 23), `tail` the words after `harness`: a read or an ack, served as
    /// aterm's verb serves them ([`Self::fact_read`], [`Self::fact_told`]); an aterm before the
    /// verb answers its older usage.
    fn fact(&mut self, tail: &str, conn: u64, peer_pid: Option<u32>) -> String {
        if !self.facts.verb {
            return OLD_HARNESS_USAGE.to_string();
        }
        let words: Vec<&str> = tail.split_ascii_whitespace().skip(1).collect();
        match words.split_first() {
            Some((&"told", rest)) => self.fact_told(rest, conn, peer_pid),
            _ => self.fact_read(&words, conn, peer_pid),
        }
    }

    /// The pid of the standing proven `codex-alab@<pid>` claim, where this connection is bound to
    /// it or its process is the claim's (aterm's `self_claim_write` with a codex-alab holder).
    fn fact_claimant(&self, conn: u64, peer_pid: Option<u32>) -> Option<u32> {
        self.claim.as_ref().and_then(|c| {
            c.proven_pid.filter(|pid| {
                c.holder.starts_with("codex-alab@")
                    && (c.conn == Some(conn) || Some(*pid) == peer_pid)
            })
        })
    }

    /// `harness fact for=<rule> reader=<id> kinds=<k>[,<k>…] wall=<w>`, its `why=` judged in
    /// aterm's order: the request alone (`rule`, `limit`, `kinds`), then the account (`none`,
    /// `offered`, `told`), then, only for a candidate, the policy (`off`, `facts-off`). A
    /// candidate is leased to (the claim's pid, `reader`) and offered.
    fn fact_read(&mut self, words: &[&str], conn: u64, peer_pid: Option<u32>) -> String {
        let Some((rule, reader, kinds, wall)) = parse_fact_read(words) else {
            return HARNESS_USAGE.to_string();
        };
        if let Some(line) = self.facts.answer_once.pop_front() {
            return format!("{line}\n");
        }
        let Some(pid) = self.fact_claimant(conn, peer_pid) else {
            return "ERR denied\n".to_string();
        };
        let none = |why: &str| format!("OK fact=- why={why}\n");
        if !FACT_RIDES.contains(&rule.as_str()) || self.facts.narrowed.contains(&rule) {
            return none("rule");
        }
        if limits_place_rule(&rule) && matches!(wall.as_str(), "usage" | "entitlement") {
            return none("limit");
        }
        if !kinds.iter().any(|k| k == crate::facts::FACT_NO_PROGRESS) {
            return none("kinds");
        }
        if self.facts.open.is_empty() {
            return none("none");
        }
        let now = tokio::time::Instant::now();
        let lease = self.facts.lease;
        let spoken_for = |offer: &FakeOffer| {
            (offer.pid != pid || offer.reader != reader) && now.duration_since(offer.at) < lease
        };
        let untold: Vec<u64> = self
            .facts
            .open
            .keys()
            .copied()
            .filter(|e| !self.facts.told.contains(e))
            .collect();
        let candidate = untold
            .iter()
            .copied()
            .find(|e| !self.facts.offers.get(e).is_some_and(spoken_for));
        let Some(episode) = candidate else {
            return none(if untold.is_empty() { "told" } else { "offered" });
        };
        if !self.effective {
            return none("off");
        }
        if !self
            .policy_keys
            .iter()
            .any(|(k, v)| k == "facts" && v == "true")
        {
            return none("facts-off");
        }
        let Some(fact) = self.facts.open.get(&episode).cloned() else {
            return none("none");
        };
        self.facts.offers.insert(
            episode,
            FakeOffer {
                pid,
                reader,
                at: now,
            },
        );
        self.changed();
        let as_of_ms = fact
            .as_of_ms
            .unwrap_or_else(|| i64::try_from(codex_alab_core::session::now_ms()).unwrap_or(0));
        format!(
            "OK fact={} kind={} episode={episode:016x} as_of_ms={as_of_ms} rider={}\n",
            pct_encode(&fact.text),
            crate::facts::FACT_NO_PROGRESS,
            pct_encode(&crate::facts::rider(&fact.text))
        )
    }

    /// `harness fact told episode=<016x>`: told where an offer of that open, untold episode stands
    /// under this claim's pid (by any reader: a late ack after a lapse still counts), the offer
    /// dropped; else `ERR stale fact`, nothing changed.
    fn fact_told(&mut self, words: &[&str], conn: u64, peer_pid: Option<u32>) -> String {
        let episode = match words {
            [word] => word
                .strip_prefix("episode=")
                .and_then(crate::facts::episode_of),
            _ => None,
        };
        let Some(episode) = episode else {
            return HARNESS_USAGE.to_string();
        };
        if self.facts.refuse_told {
            return "ERR denied\n".to_string();
        }
        let Some(pid) = self.fact_claimant(conn, peer_pid) else {
            return "ERR denied\n".to_string();
        };
        let offered = self
            .facts
            .offers
            .get(&episode)
            .is_some_and(|offer| offer.pid == pid);
        if !offered || !self.facts.open.contains_key(&episode) || self.facts.told.contains(&episode)
        {
            return "ERR stale fact\n".to_string();
        }
        self.facts.offers.remove(&episode);
        self.facts.told.insert(episode);
        self.facts.told_log.push(episode);
        self.changed();
        "OK\n".to_string()
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
                const USAGE: &str =
                    "ERR usage: meta set attention owner=<k> [kind=ask|wait] <text...>\n";
                let rest: Vec<&str> = toks.collect();
                let Some((owner, mut words)) = rest
                    .split_first()
                    .and_then(|(o, t)| Some((o.strip_prefix("owner=")?, t)))
                else {
                    return USAGE.to_string();
                };
                // aterm's optional `kind=ask|wait` token, right after the owner.
                let mut kind = None;
                if let Some((first, more)) = words.split_first()
                    && let Some(k) = first.strip_prefix("kind=")
                {
                    if !matches!(k, "ask" | "wait") {
                        return USAGE.to_string();
                    }
                    kind = Some(k.to_string());
                    words = more;
                }
                let text = words.join(" ");
                if text.is_empty() || text.len() > 200 {
                    return USAGE.to_string();
                }
                match kind {
                    Some(k) => self.attention_kind.insert(owner.to_string(), k),
                    None => self.attention_kind.remove(owner),
                };
                self.put_attention(owner, &text);
                "OK\n".to_string()
            }
            (Some("unset"), Some("attention")) => {
                let owner = toks
                    .next()
                    .and_then(|o| o.strip_prefix("owner="))
                    .unwrap_or("-");
                let before = self.shown_attention();
                self.attention_kind.remove(owner);
                if self.attention.remove(owner).is_some() {
                    self.attention_order.retain(|o| o != owner);
                    if owner == crate::NEEDS_OWNER {
                        self.settle_told_needs();
                    }
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
            // An aterm before the verb knows no such field: `META_FIELDS`, below.
            (Some("set"), Some("autopilot")) => self.set_autopilot(&toks.collect::<Vec<_>>()),
            (Some("set"), Some("harness_held")) if self.held_supported() => {
                let words: Vec<&str> = toks.collect();
                self.set_held(true, &words, conn, peer_pid)
            }
            (Some("unset"), Some("harness_held")) if self.held_supported() => {
                let words: Vec<&str> = toks.collect();
                self.set_held(false, &words, conn, peer_pid)
            }
            (Some("set"), Some("harness_land")) if self.land_supported() => {
                let words: Vec<&str> = toks.collect();
                self.set_land(true, &words, conn, peer_pid)
            }
            (Some("unset"), Some("harness_land")) if self.land_supported() => {
                let words: Vec<&str> = toks.collect();
                self.set_land(false, &words, conn, peer_pid)
            }
            (Some("set"), Some("harness_needs")) if self.needs_supported() => {
                let words: Vec<&str> = toks.collect();
                self.set_needs(true, &words, conn, peer_pid)
            }
            (Some("unset"), Some("harness_needs")) if self.needs_supported() => {
                let words: Vec<&str> = toks.collect();
                self.set_needs(false, &words, conn, peer_pid)
            }
            (Some("prepare"), Some("harness_act")) if self.mode == Mode::Current => {
                if !self.act_deadlines {
                    // An aterm before 0.110.0 (bf9d401e40): its `meta` arm knows `set` and
                    // `unset` only, and answers anything else with its plain usage.
                    return OLD_META_USAGE.to_string();
                }
                if toks.next().is_some() {
                    return "ERR usage: meta prepare harness_act\n".to_string();
                }
                if !self.claim.as_ref().is_some_and(|claim| {
                    claim.proven_pid.is_some()
                        && (claim.conn == Some(conn) || claim.proven_pid == peer_pid)
                }) {
                    return "ERR denied\n".to_string();
                }
                let now = u64::try_from(self.act_clock.elapsed().as_micros()).unwrap_or(u64::MAX);
                let Some(until) = now.checked_add(5_000_000) else {
                    return "ERR clock\n".to_string();
                };
                format!("OK until_us={until} lease_ms=5000\n")
            }
            (Some("set"), Some("harness_act")) if self.mode == Mode::Current => {
                if let Some(err) = &self.refuse_acts {
                    return format!("{err}\n");
                }
                if let Some(err) = self.refuse_next_act.take() {
                    return format!("{err}\n");
                }
                let Some(act) = toks.next() else {
                    return "ERR usage: meta set harness_act <act>\n".to_string();
                };
                if let Some(until) = toks.next() {
                    let Some(until) = until
                        .strip_prefix("until_us=")
                        .and_then(|value| value.parse::<u64>().ok())
                    else {
                        return "ERR usage: meta set harness_act <act> [until_us=<n>]\n"
                            .to_string();
                    };
                    let now =
                        u64::try_from(self.act_clock.elapsed().as_micros()).unwrap_or(u64::MAX);
                    if now >= until || until.saturating_sub(now) > 5_000_000 {
                        return "ERR expired harness_act\n".to_string();
                    }
                }
                if toks.next().is_some() {
                    return "ERR usage: meta set harness_act <act> [until_us=<n>]\n".to_string();
                }
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
        ("autopilot", "cruise"),
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
        ("probe_api", "true"),
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
            version: "0.0.0-fake".to_string(),
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
            status_err: None,
            attention: BTreeMap::new(),
            attention_kind: BTreeMap::new(),
            attention_order: Vec::new(),
            description: None,
            descriptions: Vec::new(),
            acts: Vec::new(),
            refuse_next_act: None,
            refuse_acts: None,
            act_clock: tokio::time::Instant::now(),
            act_deadlines: true,
            held_verb: true,
            held: None,
            land_verb: true,
            land: None,
            needs_verb: true,
            told_needs: None,
            facts: FakeFacts::default(),
            stories: Vec::new(),
            notices: Vec::new(),
            laters: Vec::new(),
            dones: Vec::new(),
            settles: Vec::new(),
            digest: BTreeMap::new(),
            digest_revision: 0,
            reply_once: BTreeMap::new(),
            task: None,
            next_later: 0,
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
            busy_next: 0,
            gated: HashSet::new(),
            gated_seen: Vec::new(),
            drop_replies: HashSet::new(),
            deferred_closes: None,
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

    /// Rotate the AUTH identity new connections see, as a native listener replacement
    /// does, while retaining this fixture's endpoint, peer pid and digest state. The old
    /// token file is kept beside its replacement for the caller's evidence.
    pub fn replace_listener_identity(&self) -> std::io::Result<()> {
        let n = NEXT_FAKE.fetch_add(1, Ordering::Relaxed);
        let token = format!("{:032x}", u128::from(n) * 0x9e37_79b9_7f4a_7c15 + 7);
        let path = self.endpoint.sock.with_extension("token");
        let retained = path.with_extension(format!("token.before-rebind-{n}"));
        std::fs::rename(&path, retained)?;
        write_private(&path, &format!("{token}\n"))?;
        lock(&self.state).token = token;
        Ok(())
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

    /// Drop one owner's key without an attention event, modelling a change the subscriber
    /// did not see. Read-only metadata still observes the resulting projection.
    pub fn clear_attention_quietly(&self, owner: &str) {
        let mut state = lock(&self.state);
        state.attention_kind.remove(owner);
        if state.attention.remove(owner).is_some() {
            state.attention_order.retain(|key| key != owner);
            if owner == crate::NEEDS_OWNER {
                state.settle_told_needs();
            }
            state.changed();
        }
    }

    /// Keyed attention entries.
    pub fn attention(&self) -> BTreeMap<String, String> {
        lock(&self.state).attention.clone()
    }

    /// The `kind=` each standing attention entry was set with (owners set without one are
    /// absent).
    pub fn attention_kinds(&self) -> BTreeMap<String, String> {
        lock(&self.state).attention_kind.clone()
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

    /// Refuse the next act record without appending it, as a control write that fails.
    pub fn refuse_next_harness_act(&self, err: &str) {
        lock(&self.state).refuse_next_act = Some(err.to_string());
    }

    /// Refuse every act without appending it until the control channel recovers.
    pub fn refuse_harness_acts(&self, err: Option<&str>) {
        lock(&self.state).refuse_acts = err.map(str::to_string);
    }

    /// Whether this proven-claim-capable aterm also understands act deadlines.
    pub fn support_act_deadlines(&self, supported: bool) {
        lock(&self.state).act_deadlines = supported;
    }

    /// Whether this aterm has `meta set|unset harness_held` (on by default in
    /// [`Mode::Current`]): off, it answers both as a field it does not know (`ERR unknown meta
    /// field (…)`) and its `status` says `held=-`, as aterm 0.143.0 to 0.145.0 do.
    pub fn support_harness_held(&self, supported: bool) {
        lock(&self.state).held_verb = supported;
    }

    /// The typing stop's arm the session's proven claim set, as `status held=` says it (`None`
    /// for `-`).
    pub fn held(&self) -> Option<String> {
        lock(&self.state).held.as_ref().map(|(_, arm)| arm.clone())
    }

    /// Whether this aterm has `meta set|unset harness_land` (on by default in [`Mode::Current`]):
    /// off, it answers both as a field it does not know (`ERR unknown meta field (…)`) and its
    /// `status` says `land_held=- land_nudges=- land_since=-`, as an aterm before the verb says
    /// of a codex-alab tab.
    pub fn support_harness_land(&self, supported: bool) {
        lock(&self.state).land_verb = supported;
    }

    /// Land's hold the session's proven claim set (`None` for none).
    pub fn land(&self) -> Option<crate::LandReport> {
        lock(&self.state)
            .land
            .as_ref()
            .map(|(_, land)| land.clone())
    }

    /// Leave a proven claim of `pid` standing with Land's hold `land`, bound to no connection
    /// the fake serves, as [`Self::leave_held`] leaves an arm: the next image's claim keeps it.
    pub fn leave_land(&self, pid: u32, land: crate::LandReport) {
        let mut s = lock(&self.state);
        s.set_claim(Some(FakeClaim {
            holder: crate::holder_for(pid),
            conn: None,
            proven_pid: Some(pid),
        }));
        s.land = Some((pid, land));
        s.changed();
    }

    /// NAME A NO-PROGRESS CALL (gap 23), as aterm's jobs board names a stalled background call:
    /// a new episode whose sentence is `text`, its reading taken at each read (fresh, as aterm's
    /// board is posted every few seconds). Answers the episode.
    pub fn plant_fact(&self, text: &str) -> u64 {
        self.plant(text, None)
    }

    /// [`Self::plant_fact`] with its reading pinned at `as_of_ms` (unix ms). The fake keeps no
    /// board, so it serves the pinned reading without aterm's own `tells_at` check: a reading
    /// aterm judged fresh at its read that aged before the act's start (codex-alab's last step).
    pub fn plant_fact_as_of(&self, text: &str, as_of_ms: i64) -> u64 {
        self.plant(text, Some(as_of_ms))
    }

    fn plant(&self, text: &str, as_of_ms: Option<i64>) -> u64 {
        let mut s = lock(&self.state);
        s.facts.next += 1;
        let episode = 0xfac7_0000_0000_0000 | s.facts.next;
        s.facts.open.insert(
            episode,
            FakeFact {
                text: text.to_string(),
                as_of_ms,
            },
        );
        s.changed();
        episode
    }

    /// The call of `episode` made progress or ended: a fresh reading no longer names it (aterm's
    /// `observe`), so its episode, its told mark and its offer are forgotten.
    pub fn close_fact(&self, episode: u64) {
        let mut s = lock(&self.state);
        s.facts.open.remove(&episode);
        s.facts.told.remove(&episode);
        s.facts.offers.remove(&episode);
        s.changed();
    }

    /// Every episode an ack told, in order.
    pub fn facts_told(&self) -> Vec<u64> {
        lock(&self.state).facts.told_log.clone()
    }

    /// Every `harness fact for=…` read that reached the fake (answered or not), as (`for`,
    /// `reader`), in order.
    pub fn fact_reads(&self) -> Vec<(String, String)> {
        lock(&self.state)
            .requests
            .iter()
            .filter_map(|line| fact_read_of(line))
            .collect()
    }

    /// How long an offered episode stays leased to its reader (aterm's `OFFER_MS`, 30 s by
    /// default).
    pub fn fact_lease(&self, lease: Duration) {
        lock(&self.state).facts.lease = lease;
    }

    /// Whether this aterm has `harness fact` (on by default in [`Mode::Current`]): off, it answers
    /// the verb with its older `harness` usage (`ERR usage: harness policy | recovery`), as every
    /// aterm before gap 23 does.
    pub fn support_harness_fact(&self, supported: bool) {
        lock(&self.state).facts.verb = supported;
    }

    /// Answer the next `harness fact for=…` read with `line` (`OK fact=- why=busy`, `ERR aterm is
    /// showing a dialog`), applying nothing.
    pub fn fact_answer_once(&self, line: &str) {
        lock(&self.state)
            .facts
            .answer_once
            .push_back(line.to_string());
    }

    /// Answer every ack `ERR denied` (on) or as aterm does (off).
    pub fn refuse_fact_told(&self, refuse: bool) {
        lock(&self.state).facts.refuse_told = refuse;
    }

    /// Let no fact ride an act under `rule`, as a later aterm whose own loop narrowed its
    /// `FACT_RIDES` does: its reads are answered `OK fact=- why=rule`.
    pub fn narrow_fact_rides(&self, rule: &str) {
        lock(&self.state).facts.narrowed.insert(rule.to_string());
    }

    /// Never answer a `harness fact for=…` read (on) or answer them again (off); acks and every
    /// other `harness` line are still answered.
    pub fn stall_fact_reply(&self, stall: bool) {
        lock(&self.state).facts.stall = stall;
    }

    /// Whether this aterm has `meta set|unset harness_needs` (on by default in
    /// [`Mode::Current`]): off, it answers both as a field it does not know (`ERR unknown meta
    /// field (…)`), as an aterm before the verb does, whose readers read a codex-alab tab's
    /// entry's first need and its count only.
    pub fn support_harness_needs(&self, supported: bool) {
        lock(&self.state).needs_verb = supported;
    }

    /// The needs aterm's readers read whole now (the menu bar's row one level in, the hover's
    /// list): the set the proven claim told whose entry is the `owner=needs` entry standing,
    /// else the set told before it while its entry stands; `None` for neither.
    pub fn needs_told(&self) -> Option<Vec<String>> {
        let s = lock(&self.state);
        let standing = s.attention.get(crate::NEEDS_OWNER).map(String::as_str);
        s.told_needs
            .as_ref()
            .and_then(|(_, held)| held.words_for(standing))
            .map(<[String]>::to_vec)
    }

    /// The set the proven claim told last, whatever entry stands (`None`: none told, or an
    /// unset taken last; aterm's `ToldNeeds::current`).
    pub fn needs_told_last(&self) -> Option<FakeNeeds> {
        lock(&self.state)
            .told_needs
            .as_ref()
            .and_then(|(_, held)| held.current.clone())
    }

    /// Leave a proven claim of `pid` standing with the needs `needs` told beside the entry
    /// `entry`, the entry standing too, bound to no connection the fake serves, as
    /// [`Self::leave_held`] leaves an arm: the next image's claim keeps the set.
    pub fn leave_needs(&self, pid: u32, entry: &str, needs: &[&str]) {
        let mut s = lock(&self.state);
        s.set_claim(Some(FakeClaim {
            holder: crate::holder_for(pid),
            conn: None,
            proven_pid: Some(pid),
        }));
        s.attention_kind
            .insert(crate::NEEDS_OWNER.to_string(), "ask".to_string());
        s.put_attention(crate::NEEDS_OWNER, entry);
        let mut held = FakeToldNeeds::default();
        held.tell(
            Some(FakeNeeds {
                entry: entry.to_string(),
                needs: needs.iter().map(|need| (*need).to_string()).collect(),
            }),
            Some(entry),
        );
        s.told_needs = Some((pid, held));
        s.changed();
    }

    /// Leave a proven claim of `pid` standing with the typing stop's arm `arm`, bound to no
    /// connection the fake serves: what an image of that process leaves when it execs before
    /// aterm takes its release (an exec keeps the pid and kernel start, aterm's proof), so the
    /// next image's claim keeps the arm.
    pub fn leave_held(&self, pid: u32, arm: &str) {
        let mut s = lock(&self.state);
        s.set_claim(Some(FakeClaim {
            holder: crate::holder_for(pid),
            conn: None,
            proven_pid: Some(pid),
        }));
        s.held = Some((pid, arm.to_string()));
        s.changed();
    }

    /// While `deferred`, a connection's close goes unread, as aterm leaves one whose handler is
    /// still busy: the claim bound to it stands until a re-claim by the same pid on another
    /// connection takes it over (keeping its arm), or until `false` reads the closes deferred.
    pub fn defer_closes(&self, deferred: bool) {
        let mut s = lock(&self.state);
        if deferred {
            s.deferred_closes.get_or_insert_with(Vec::new);
        } else if let Some(closed) = s.deferred_closes.take() {
            for conn in closed {
                s.release_conn(conn);
            }
        }
    }

    /// The release `version` answers from the next connection on (`0.0.0-fake` by default),
    /// as `version=<release>`.
    pub fn set_version(&self, release: &str) {
        lock(&self.state).version = release.to_string();
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

    /// Every `notice later` line taken, in order, with how it was answered.
    pub fn laters(&self) -> Vec<FakeLater> {
        lock(&self.state).laters.clone()
    }

    /// Every `notice done` line taken (the words after `notice done`), in order.
    pub fn dones(&self) -> Vec<String> {
        lock(&self.state).dones.clone()
    }

    /// Atomic `notice settle` requests, without the command prefix.
    pub fn settles(&self) -> Vec<String> {
        lock(&self.state).settles.clone()
    }

    /// Answer the next exact command with these raw wire bytes instead of applying it.
    pub fn reply_once(&self, command: &str, reply: &str) {
        lock(&self.state)
            .reply_once
            .insert(command.to_string(), reply.to_string());
    }

    /// Dismiss `key`'s digest entry as the owner does: the key is then quiet. Answers whether
    /// there was one.
    pub fn dismiss_later(&self, key: &str) -> bool {
        let mut s = lock(&self.state);
        let found = s.digest.get_mut(key).map(|e| e.dismissed = true).is_some();
        if found {
            s.digest_revision += 1;
        }
        s.changed();
        found
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

    /// End the push sockets while leaving request sockets and their claims
    /// alive, as a lost subscription does during a listener handoff.
    pub fn end_subscriptions(&self) {
        let mut s = lock(&self.state);
        s.subscribers.clear();
        s.mail_subscribers.clear();
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

    /// This session's task record (`Some(state)`: a lead's `spawn` stands behind it, as
    /// aterm's `@self task` answers it), or none.
    pub fn set_task(&self, state: Option<&str>) {
        lock(&self.state).task = state.map(str::to_string);
    }

    /// Answer `@self status` with this `ERR …` line instead (`None` restores it), as a read
    /// that fails does.
    pub fn answer_status_with(&self, err: Option<&str>) {
        lock(&self.state).status_err = err.map(str::to_string);
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

    /// Hold every request with this verb or command prefix, unanswered, until [`FakeAterm::open_gate`]
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

    /// Apply the next matching command, then close its request socket without
    /// acknowledging it. Unlike a stall, the server's actual mutation happens.
    pub fn drop_next_reply(&self, command: &str) {
        lock(&self.state).drop_replies.insert(command.to_string());
    }

    /// Refuse the next `connections` at admission with aterm's `ERR control server busy; retry
    /// retry_ms=50`, before their AUTH is read, as aterm does when every request lane is taken.
    pub fn busy_next(&self, connections: u32) {
        lock(&self.state).busy_next = connections;
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
    let (conn, token, busy) = {
        let mut s = lock(&state);
        s.next_conn += 1;
        let busy = s.busy_next > 0;
        s.busy_next = s.busy_next.saturating_sub(1);
        (s.next_conn, s.token.clone(), busy)
    };
    let (read, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read);
    if busy {
        let _ = writer
            .write_all(b"ERR control server busy; retry retry_ms=50\n")
            .await;
        // Take the client's hello before closing, so its write never meets a closed socket.
        let _ = read_line(&mut reader).await;
        return;
    }
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
                let request = line
                    .strip_prefix('@')
                    .and_then(|rest| rest.split_once(' '))
                    .map_or(line.as_str(), |(_, rest)| rest);
                if !s.gated.iter().any(|gate| {
                    request == gate
                        || request
                            .strip_prefix(gate)
                            .is_some_and(|tail| tail.starts_with(' '))
                }) {
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
        let stalled = {
            let s = lock(&state);
            s.stalled.contains(&verb) || (s.facts.stall && fact_read_of(&line).is_some())
        };
        if stalled {
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
        let (reply, drop_reply) = {
            let mut s = lock(&state);
            let reply = s.handle(&line, conn, peer_pid);
            let request = line
                .strip_prefix('@')
                .and_then(|rest| rest.split_once(' '))
                .map_or(line.as_str(), |(_, rest)| rest);
            let dropped = s
                .drop_replies
                .iter()
                .find(|command| {
                    request == command.as_str()
                        || request
                            .strip_prefix(command.as_str())
                            .is_some_and(|tail| tail.starts_with(' '))
                })
                .cloned();
            if let Some(command) = &dropped {
                s.drop_replies.remove(command);
            }
            (reply, dropped.is_some())
        };
        if drop_reply {
            break;
        }
        if writer.write_all(reply.as_bytes()).await.is_err() {
            break;
        }
    }
    lock(&state).close_conn(conn);
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

#[cfg(test)]
#[path = "fake_confirmation_tests.rs"]
mod confirmation_tests;
