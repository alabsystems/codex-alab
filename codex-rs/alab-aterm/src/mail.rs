// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! This session's aterm mail, with verbs aterm already has (`aterm ctl help inbox`, `post`):
//! the native inbox of CONTRACT §5.
//!
//! - [`read`]: the row a typed `Inbox: <kind> @<off>` names (`inbox --peek --meta`, which
//!   moves no listed state), then its body (`inbox get <id>`, a Bytes frame answered from the
//!   ring; never `inbox get @<off>`, which may park up to 10 s for the bridge).
//! - [`badge`]: what waits above the handled watermark, for the status line.
//! - [`seen`] and [`report`]: the turn's receipt (`inbox seen <id> handled|deferred`, which
//!   the manager's `post --wait-ack` waits for) and the stamped `report` back to the sender
//!   (`post to=<reply_to(from)> kind=report re=<off> len=<n>`, its body on the wire after the
//!   line, so the model's words never travel inside a request line).
//! - [`receipts_due`]: which owed receipts may go now. `inbox seen <id>` moves the HANDLED
//!   watermark to `max(seen, id)` ("done up to <id>"), so a receipt never goes past an older
//!   task or ask nobody has handled; it waits until that one is.
//!
//! Every reading fails closed: a row that is not there, of another kind, already handled, or
//! unreadable is an error, and the caller leaves the typed line as it is.

use codex_alab_core::mail::InboxMail;
use codex_alab_core::mail::MailBadge;
use codex_alab_core::mail::TriggerKind;
use codex_alab_core::mail::Verdict;

use crate::Client;
use crate::Error;
use crate::wire;

/// One `inbox` row (`msg <id> off=<n> t=<ms> from=<p> kind=<k> trust=<t> … len=<n>
/// [more=1] [truncated=1] [text=<pct>]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: u64,
    pub off: u64,
    pub from: String,
    pub kind: String,
    pub trust: String,
    pub len: usize,
    pub truncated: bool,
    pub deadline: Option<String>,
}

impl Row {
    /// One `inbox` row, or `None` for anything else (a `post` row, a malformed one).
    pub fn parse(line: &str) -> Option<Row> {
        let rest = line.strip_prefix("msg ")?;
        let (id, rest) = rest.split_once(' ')?;
        let fields = wire::parse_fields(rest);
        let raw = |k: &str| fields.get(k).filter(|v| !v.is_empty()).cloned();
        Some(Row {
            id: wire::canonical_u64(id)?,
            off: wire::canonical_u64(&raw("off")?)?,
            from: raw("from")?,
            kind: raw("kind")?,
            trust: raw("trust")?,
            len: usize::try_from(wire::canonical_u64(&raw("len")?)?).ok()?,
            truncated: fields.get("truncated").map(String::as_str) == Some("1"),
            deadline: raw("dl"),
        })
    }
}

/// The `inbox` header's handled watermark (`seen=<id>`).
pub fn seen_watermark(status: &str) -> Option<u64> {
    let rest = status.strip_prefix("OK ")?;
    wire::parse_fields(rest)
        .get("seen")
        .and_then(|s| wire::canonical_u64(s))
}

/// Every message row this session's inbox holds and its handled watermark, read without
/// listing anything (`inbox --peek --meta`: no listed state moves, no body is sent).
pub async fn rows(client: &Client) -> Result<(u64, Vec<Row>), Error> {
    let reply = client
        .request_lines(&format!("@{} inbox --peek --meta", client.endpoint().sid))
        .await?;
    if reply.is_err() {
        return Err(crate::conn::refusal(reply.status));
    }
    let seen = seen_watermark(&reply.status)
        .ok_or_else(|| Error::Malformed(format!("inbox header: {}", reply.status)))?;
    Ok((
        seen,
        reply.lines.iter().filter_map(|l| Row::parse(l)).collect(),
    ))
}

/// The message at broker offset `off`, which must be a `kind` row nobody has handled: its row,
/// then its body. `given` is this process's own verdict on a row id, if it gave one. A row this
/// process handled (its receipt sent or still owed) is refused, and so is any row at or below
/// the handled watermark, unless this process's last word on it was `deferred` (an
/// interrupted turn, which the line may run again). A line recalled from history or typed
/// twice never runs the mail, or reports, a second time.
pub async fn read(
    client: &Client,
    kind: TriggerKind,
    off: u64,
    given: &(dyn Fn(u64) -> Option<Verdict> + Send + Sync),
) -> Result<InboxMail, Error> {
    let (seen, rows) = rows(client).await?;
    let row = rows
        .into_iter()
        .find(|r| r.off == off)
        .ok_or_else(|| Error::Refused(format!("no inbox row at @{off}")))?;
    if row.kind != kind.word() {
        return Err(Error::Refused(format!(
            "the row at @{off} is a {}, not a {}",
            row.kind,
            kind.word()
        )));
    }
    match given(row.id) {
        Some(Verdict::Handled) => {
            return Err(Error::Refused(format!(
                "the mail @{off} was already handled"
            )));
        }
        Some(Verdict::Deferred) => {}
        None if row.id <= seen => {
            return Err(Error::Refused(format!(
                "the mail @{off} was already handled (seen={seen})"
            )));
        }
        None => {}
    }
    let reply = client
        .request_bytes(&format!("@{} inbox get {}", client.endpoint().sid, row.id))
        .await?;
    let Some(tail) = reply.ok() else {
        return Err(crate::conn::refusal(reply.status));
    };
    let fields = wire::parse_fields(tail);
    let cut = fields.get("truncated").map(String::as_str) == Some("1");
    let body = String::from_utf8(reply.bytes)
        .map_err(|_| Error::Malformed(format!("the body at @{off} is not UTF-8")))?;
    let cut_from = if cut || row.truncated {
        Some(
            fields
                .get("len")
                .and_then(|l| wire::canonical_u64(l))
                .and_then(|l| usize::try_from(l).ok())
                .unwrap_or(row.len),
        )
    } else {
        None
    };
    Ok(InboxMail {
        id: row.id,
        off: row.off,
        from: wire::pct_decode(&row.from),
        kind: row.kind,
        trust: row.trust,
        body,
        cut_from,
        deadline: row.deadline,
    })
}

/// What waits above the handled watermark (the status line's badge).
pub async fn badge(client: &Client) -> Result<MailBadge, Error> {
    let (seen, rows) = rows(client).await?;
    let mut badge = MailBadge::default();
    for row in rows.iter().filter(|r| r.id > seen) {
        badge.count(&row.kind);
    }
    Ok(badge)
}

/// The live screen's stamp a report opens with (`seq=<n> hash=<hex16>`, the pair `status`
/// prints and `history` keeps per turn), or `None` when aterm could not take it.
pub async fn stamp(client: &Client) -> Result<Option<String>, Error> {
    let status = client.status().await?;
    let seq = status
        .get("seq")
        .filter(|s| wire::canonical_u64(s).is_some());
    let hash = status
        .get("hash")
        .filter(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    Ok(match (seq, hash) {
        (Some(seq), Some(hash)) if !seq.is_empty() => Some(format!("seq={seq} hash={hash}")),
        _ => None,
    })
}

/// The receipt: `inbox seen <id> <verdict>`. It also lists every row at or below `id` (what
/// the sender's per-peer quota counts as read), as aterm says it does.
pub async fn seen(client: &Client, id: u64, verdict: Verdict) -> Result<(), Error> {
    let reply = client
        .request(&format!(
            "@{} inbox seen {id} {}",
            client.endpoint().sid,
            verdict.word()
        ))
        .await?;
    match reply.ok() {
        Some(_) => Ok(()),
        None => Err(crate::conn::refusal(reply.status)),
    }
}

/// The largest report body sent (aterm's `BODY_MAX` is 256 KiB).
pub const REPORT_MAX_BYTES: usize = 64 * 1024;

/// A report's body: the stamp line first when there is one (aterm's `drive supervise --mail`
/// reads it), then the turn's last agent message, or a line saying there was none.
pub fn report_body(stamp: Option<&str>, verdict: Verdict, last_message: Option<&str>) -> String {
    let mut body = String::new();
    if let Some(stamp) = stamp {
        body.push_str(stamp);
        body.push('\n');
    }
    match last_message.map(str::trim).filter(|m| !m.is_empty()) {
        Some(message) => body.push_str(message),
        None => body.push_str(match verdict {
            Verdict::Handled => "(the turn ended without a message)",
            Verdict::Deferred => "(the turn was interrupted or stopped at an error)",
        }),
    }
    if body.len() > REPORT_MAX_BYTES {
        let mut end = REPORT_MAX_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
    }
    body
}

/// An owed receipt that waits: its row id, and the older unhandled row it waits behind.
pub type Held = (u64, Row);

/// The owed receipts that may go now, in id order: each `(id, verdict)` of `owed` until the
/// first that an older unhandled `task` or `ask` row stands below (above `seen`, and not one of
/// `owed`), which is returned with it as `(held id, blocking row)`. `inbox seen <id>` marks
/// every row up to `id` handled, so going past that row would hide it from the badge and from
/// an update's refill (aterm: "batch consumers must not advance `seen=` across omitted older
/// rows they have not handled"). Other kinds (notes, reports, answers, acks) never get a turn
/// and hold nothing.
pub fn receipts_due(
    seen: u64,
    rows: &[Row],
    owed: &std::collections::BTreeMap<u64, Verdict>,
) -> (Vec<(u64, Verdict)>, Option<Held>) {
    let mut due = Vec::new();
    for (&id, &verdict) in owed {
        let blocker = rows
            .iter()
            .filter(|r| {
                r.id > seen
                    && r.id < id
                    && matches!(r.kind.as_str(), "task" | "ask")
                    && !owed.contains_key(&r.id)
            })
            .min_by_key(|r| r.id);
        if let Some(blocker) = blocker {
            return (due, Some((id, blocker.clone())));
        }
        due.push((id, verdict));
    }
    (due, None)
}

/// Where a reply to `from` (an `inbox` row's sender, decoded) is posted, as aterm's bridge
/// routes a `to=` (`aterm-link` `resolve_to`): a session sender (`s-<sid>` or
/// `s-<sid>@n-<node>`, as rows print it) only as `@s-<sid>[@n-<node>]`, a bare principal
/// (`h-…`, `a-…`, `n-…`) as it is. `None` for anything the bridge would retire as
/// unroutable, though the local `post` would still answer `OK`.
pub fn reply_to(from: &str) -> Option<String> {
    let session = from.strip_prefix('@').unwrap_or(from);
    if session.starts_with("s-") {
        let (sid, node) = match session.split_once('@') {
            Some((sid, node)) => (sid, Some(node)),
            None => (session, None),
        };
        let node_ok = node.is_none_or(|n| n.starts_with("n-") && principal(n));
        return (principal(sid) && node_ok).then(|| format!("@{session}"));
    }
    (!from.starts_with('@') && principal(from)).then(|| from.to_string())
}

/// aterm's principal grammar (`aterm-link` `subject::is_principal`): a class prefix (`s-`,
/// `n-`, `h-`, `a-`), then 1 to 32 of `[a-z0-9-]`.
fn principal(p: &str) -> bool {
    let Some(name) = ["s-", "n-", "h-", "a-"]
        .iter()
        .find_map(|c| p.strip_prefix(c))
    else {
        return false;
    };
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The report: `post to=<to> kind=report re=<off> len=<n>`, then the body; `to` is the
/// sender's [`reply_to`] address. Answers aterm's `OK <id> …`.
pub async fn report(client: &Client, to: &str, off: u64, body: &str) -> Result<String, Error> {
    if to.is_empty() || to.len() > 200 || !to.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
        return Err(Error::BadRequest(format!("post to={to}")));
    }
    let line = format!(
        "@{} post to={to} kind=report re={off} len={}",
        client.endpoint().sid,
        body.len()
    );
    let reply = client
        .request_with_body(&line, body.as_bytes().to_vec())
        .await?;
    match reply.ok() {
        Some(rest) => Ok(rest.to_string()),
        None => Err(crate::conn::refusal(reply.status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn rows_parse_as_aterm_prints_them() {
        let row = Row::parse(
            "msg 7 off=1203 t=1792130000000 from=s-abc%40n-lab kind=task trust=agent dl=1792130060000 len=12",
        )
        .expect("row");
        assert_eq!(row.id, 7);
        assert_eq!(row.off, 1203);
        assert_eq!(row.from, "s-abc%40n-lab");
        assert_eq!(row.kind, "task");
        assert_eq!(row.trust, "agent");
        assert_eq!(row.len, 12);
        assert!(!row.truncated);
        assert_eq!(row.deadline.as_deref(), Some("1792130060000"));
        let cut = Row::parse(
            "msg 8 off=9 t=1 from=h-andrew kind=ask trust=human len=900000 more=1 truncated=1",
        )
        .expect("row");
        assert!(cut.truncated);
        for line in [
            "post 3 to=@s-b kind=ask off=- len=4",
            "msg x off=1 t=1 from=a kind=task trust=human len=1",
            "msg 1 t=1 from=a kind=task trust=human len=1",
            "msg 1 off=1 t=1 kind=task trust=human len=1",
            "msg 1 off=1 t=1 from=a kind=task len=1",
        ] {
            assert_eq!(Row::parse(line), None, "{line}");
        }
        assert_eq!(
            seen_watermark(
                "OK 2 hold=0 holder=- seen=5 bus_head=9 oldest_on_bus=@1 dropped=0 pending=0"
            ),
            Some(5)
        );
        assert_eq!(seen_watermark("OK 2 hold=0"), None);
    }

    #[test]
    fn a_reply_goes_where_the_bridge_routes_it() {
        // A session sender as rows print it (`from=s-x%40n-y`, decoded): only `@`-led routes.
        assert_eq!(reply_to("s-lead@n-lab").as_deref(), Some("@s-lead@n-lab"));
        assert_eq!(reply_to("s-lead").as_deref(), Some("@s-lead"));
        assert_eq!(reply_to("@s-lead@n-lab").as_deref(), Some("@s-lead@n-lab"));
        assert_eq!(reply_to("h-andrew").as_deref(), Some("h-andrew"));
        assert_eq!(reply_to("a-ci").as_deref(), Some("a-ci"));
        for from in [
            "",
            "s-",
            "s-Lead@n-lab",
            "s-lead@lab",
            "s-lead@n-",
            "h-andrew@n-lab",
            "@h-andrew",
            "andrew",
            "s-lead@n-lab@n-x",
            &format!("h-{}", "a".repeat(33)),
        ] {
            assert_eq!(reply_to(from), None, "{from}");
        }
    }

    fn row(id: u64, kind: &str) -> Row {
        Row {
            id,
            off: 100 + id,
            from: "s-a@n-b".to_string(),
            kind: kind.to_string(),
            trust: "agent".to_string(),
            len: 1,
            truncated: false,
            deadline: None,
        }
    }

    #[test]
    fn a_receipt_never_passes_an_older_unhandled_task() {
        use std::collections::BTreeMap;
        let rows = [
            row(5, "task"),
            row(6, "note"),
            row(7, "task"),
            row(9, "ask"),
        ];
        let owed = |ids: &[(u64, Verdict)]| ids.iter().copied().collect::<BTreeMap<_, _>>();
        // @7's turn ended while the task at 5 waits: 7 is held behind 5.
        let (due, held) = receipts_due(4, &rows, &owed(&[(7, Verdict::Handled)]));
        assert_eq!(due, vec![]);
        assert_eq!(held.map(|(id, r)| (id, r.id)), Some((7, 5)));
        // 5 is handled too: both go, in order; the note at 6 holds nothing.
        let both = owed(&[(5, Verdict::Deferred), (7, Verdict::Handled)]);
        let (due, held) = receipts_due(4, &rows, &both);
        assert_eq!(due, vec![(5, Verdict::Deferred), (7, Verdict::Handled)]);
        assert!(held.is_none());
        // Someone else handled 5 (the watermark passed it): 7 goes.
        let (due, held) = receipts_due(5, &rows, &owed(&[(7, Verdict::Handled)]));
        assert_eq!(due, vec![(7, Verdict::Handled)]);
        assert!(held.is_none());
        // A newer ask (9) never holds an older receipt; 9's own waits behind 7 only if 7 is
        // unhandled.
        let (due, held) = receipts_due(
            4,
            &rows,
            &owed(&[(5, Verdict::Handled), (9, Verdict::Handled)]),
        );
        assert_eq!(due, vec![(5, Verdict::Handled)]);
        assert_eq!(held.map(|(id, r)| (id, r.id)), Some((9, 7)));
    }

    #[test]
    fn a_report_opens_with_its_stamp_and_is_bounded() {
        assert_eq!(
            report_body(
                Some("seq=41 hash=00112233445566aa"),
                Verdict::Handled,
                Some("Done: the inbox is native.\n")
            ),
            "seq=41 hash=00112233445566aa\nDone: the inbox is native."
        );
        assert_eq!(
            report_body(None, Verdict::Deferred, None),
            "(the turn was interrupted or stopped at an error)"
        );
        let long = "é".repeat(REPORT_MAX_BYTES);
        let body = report_body(None, Verdict::Handled, Some(&long));
        assert!(body.len() <= REPORT_MAX_BYTES);
        assert!(body.chars().all(|c| c == 'é'));
    }
}
